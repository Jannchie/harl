//! dsh（DeepSeek 的 harness）会话日志 → Event。turn / step 是显式编号的（`data.turn` / `data.step`）。
//!
//! 文件：`~/.dsh/sessions/<cwd 编码>/<session>/session.jsonl.zstd`（v0：流式 chunk 逐行记），
//! 或同目录的 `session.v3.jsonl.zstd`（v3：chunk 折进 assistant/message 的 `stream`）。两版事件名和字段相同；
//! `session.v4.jsonl.zstd` 也一样，只是 tool/result 的 message 不再包一层 tool-result part。
//! 老会话迁移成新格式后旧文件还留着（v0→v3、v0→v4、v3→v4 都见过）：同目录有更新版本（v4 > v3 > v0）时
//! 旧的不进会话视图（`superseded_by`）。
//! 会话 id 是目录名；子 agent 的目录不带 `session-` 前缀，父会话只写在首行 `session` 头的 `parentSession` 里。
//! `user/message`、`system/message` 等行不带编号，归到 turn/start…turn/end、step/start…step/end 之间的那一个，记为推断。

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde_json::{Map, Value};

use super::{Agent, LineNormalizer, Provider, dir_name, join_texts, take, take_str, take_text_part, take_u64, take_usage, tools};
use crate::event::{Event, Kind, Prov, Role, Scope, Signal, Tool};

/// 从旧到新；后面的取代前面的
const VERSIONS: &[&str] = &["/session.jsonl.zstd", "/session.v3.jsonl.zstd", "/session.v4.jsonl.zstd"];

/// 同目录里所有比它新的格式文件（新的在前）；不是已知格式或已是最新则为空
fn newer_versions(p: &str) -> Vec<String> {
    VERSIONS
        .iter()
        .position(|v| p.ends_with(v))
        .map(|i| {
            let dir = &p[..p.len() - VERSIONS[i].len()];
            VERSIONS[i + 1..].iter().rev().map(|v| format!("{dir}{v}")).collect()
        })
        .unwrap_or_default()
}

pub static PROVIDER: Provider = Provider {
    name: "dsh",
    owns: |p| p.contains("/.dsh/sessions/"),
    default_roots: |home: &Path| -> Vec<PathBuf> { vec![home.join(".dsh").join("sessions")] },
    new: |path| Box::new(Normalizer::new(path)),
    superseded_by: newer_versions,
};

pub struct Normalizer {
    session: String,
    /// 首行 session 头读到后才知道
    parent: Option<String>,
    /// session 头的 `origin`（`subagent` 等）
    origin: Option<String>,
    /// 正在进行的 turn / step（编号），给不带编号的行用
    turn: Option<u32>,
    step: Option<u32>,
}

impl Normalizer {
    pub fn new(path: &str) -> Self {
        Self { session: dir_name(path).to_string(), parent: None, origin: None, turn: None, step: None }
    }
}

/// 毫秒时间戳
fn take_ms(obj: &mut Map<String, Value>, key: &str) -> Option<DateTime<Utc>> {
    let ms: i64 = take(obj, key)?;
    DateTime::from_timestamp_millis(ms)
}

impl LineNormalizer for Normalizer {
    fn identity(&self) -> (String, Option<String>) {
        (self.session.clone(), self.parent.clone())
    }

    fn agent(&self) -> Agent {
        Agent { kind: self.origin.clone(), ..Default::default() }
    }

    fn line(&mut self, seq: u32, mut obj: Map<String, Value>, prov: Prov) -> Event {
        // type 不映射到任何字段，留在 ext 里；这里只读
        let ty = obj.get("type").and_then(Value::as_str).unwrap_or("").to_string();
        let mut ts = take_ms(&mut obj, "time").or_else(|| take_ms(&mut obj, "time0"));
        if ty == "session" {
            ts = take_ms(&mut obj, "createdAt");
            self.parent = take_str(&mut obj, "parentSession");
        }
        let mut data = match obj.remove("data") {
            Some(Value::Object(m)) => m,
            Some(v) => {
                obj.insert("data".into(), v);
                Map::new()
            }
            None => Map::new(),
        };
        if ty == "session" {
            let origin = |m: &Map<String, Value>| m.get("origin").and_then(Value::as_str).map(str::to_string);
            self.origin = origin(&obj).or_else(|| origin(&data));
        }
        let explicit_turn = take_u64(&mut data, "turn").map(|n| n as u32);
        let explicit_step = take_u64(&mut data, "step").map(|n| n as u32);
        match ty.as_str() {
            "turn/start" => (self.turn, self.step) = (explicit_turn, None),
            "step/start" => self.step = explicit_step,
            _ => {}
        }
        let mut inferred = Vec::new();
        let turn = explicit_turn.or_else(|| self.turn.inspect(|_| inferred.push("turn")));
        let step = match explicit_turn {
            Some(_) => explicit_step,
            None => self.step.inspect(|_| inferred.push("step")),
        };
        match ty.as_str() {
            "turn/end" => (self.turn, self.step) = (None, None),
            "step/end" => self.step = None,
            _ => {}
        }
        let mut ev = Event {
            provider: PROVIDER.name,
            session: self.session.clone(),
            parent_session: self.parent.clone(),
            seq,
            ts,
            turn: turn.map(|n| Scope { id: n.to_string(), n: Some(n) }),
            step: turn.zip(step).map(|(t, s)| Scope { id: format!("{t}.{s}"), n: Some(s) }),
            inferred,
            prov,
            ..Default::default()
        };

        match ty.as_str() {
            "turn/start" => ev.kind = Kind::TurnStarted,
            "turn/end" => ev.kind = Kind::TurnCompleted,
            "step/start" => ev.kind = Kind::StepStarted,
            "step/end" => ev.kind = Kind::StepCompleted,
            "request/header" | "request/context" => ev.kind = Kind::RequestHeader,
            // user/message 的 data 本身就是消息。只有 source.kind = user 的是人打的，
            // 其余（plugin 注入的运行时上下文、skill 目录、子 agent 回报……）是喂给模型的上下文
            "user/message" => {
                let by_user = data.get("source").and_then(|s| s.get("kind")).and_then(Value::as_str) == Some("user");
                if by_user {
                    ev.kind = Kind::Message;
                    ev.role = Some(Role::User);
                } else {
                    ev.kind = Kind::RequestHeader;
                }
                ev.text = take_texts(&mut data);
            }
            // 压缩 / 修剪：被遮蔽的行号（dsh 自己的 seq）和 token 数留在 data 里，紧跟的 replace 行换上摘要
            "compaction/summary" | "compaction/prune" => ev.signal = Some(Signal::Compaction),
            "system/message" | "developer/message" => {
                ev.kind = Kind::RequestHeader;
                if let Some(Value::Object(mut m)) = data.remove("message") {
                    ev.text = take_texts(&mut m);
                    data.insert("message".into(), Value::Object(m));
                }
            }
            "assistant/message" => {
                ev.kind = Kind::Message;
                ev.role = Some(Role::Assistant);
                ev.usage = take_usage(&mut data, "usage");
                if let Some(Value::Object(mut m)) = data.remove("message") {
                    if let Some(Value::Array(parts)) = m.remove("content") {
                        let (text, reasoning, rest) = split_parts(parts);
                        ev.text = text;
                        ev.reasoning = reasoning;
                        if !rest.is_empty() {
                            m.insert("content".into(), Value::Array(rest));
                        }
                    }
                    if let Some(Value::Object(src)) = m.get_mut("source") {
                        ev.model = take_str(src, "model");
                    }
                    data.insert("message".into(), Value::Object(m));
                }
            }
            "tool/call" => {
                ev.kind = Kind::ToolCall;
                ev.role = Some(Role::Assistant);
                let name = take_str(&mut data, "name").unwrap_or_default();
                // arguments 是模型吐的 JSON 字符串；解析不了就原样当字符串
                let input = data.remove("arguments").map(|a| match a {
                    Value::String(s) => serde_json::from_str(&s).unwrap_or(Value::String(s)),
                    v => v,
                });
                ev.tool = Some(Tool {
                    call_id: take_str(&mut data, "callId").unwrap_or_default(),
                    category: Some(tools::categorize(&name)),
                    name,
                    input,
                    output: None,
                    status: None,
                    review: None,
                });
            }
            "tool/result" => {
                ev.kind = Kind::ToolResult;
                ev.role = Some(Role::Tool);
                ev.tool = Some(tool_result(&mut data));
            }
            _ => {}
        }
        if !data.is_empty() {
            obj.insert("data".into(), Value::Object(data));
        }
        ev.ext = obj;
        ev
    }
}

/// 消息对象里 content 的 text part 拼成正文，其余 part 留下
fn take_texts(m: &mut Map<String, Value>) -> Option<String> {
    let Some(Value::Array(parts)) = m.remove("content") else { return None };
    let (mut text, mut rest) = (Vec::new(), Vec::new());
    for p in parts {
        match p.get("type").and_then(Value::as_str) {
            Some("text") => take_text_part(p, "text", &mut text, &mut rest),
            _ => rest.push(p),
        }
    }
    if !rest.is_empty() {
        m.insert("content".into(), Value::Array(rest));
    }
    join_texts(text)
}

/// assistant content parts：text 拼成正文，reasoning 拼成推理，其余（tool-call 等）原样留下
fn split_parts(parts: Vec<Value>) -> (Option<String>, Option<String>, Vec<Value>) {
    let (mut text, mut reasoning, mut rest) = (Vec::new(), Vec::new(), Vec::new());
    for p in parts {
        match p.get("type").and_then(Value::as_str) {
            Some("text") => take_text_part(p, "text", &mut text, &mut rest),
            Some("reasoning") => take_text_part(p, "text", &mut reasoning, &mut rest),
            _ => rest.push(p),
        }
    }
    (join_texts(text), join_texts(reasoning), rest)
}

/// tool/result：v0 / v3 是 `message.content` 里的 tool-result part 带 toolCallId / isError / content；
/// v4 把这三个字段提到 message 上，`message.content` 直接就是结果的 part（text、image……）。
/// 名字不在这一行上（统计按 call_id 从 tool/call 配回来）。`data.error` 或 isError 就是失败。
fn tool_result(data: &mut Map<String, Value>) -> Tool {
    let mut tool = Tool::default();
    let mut failed = data.contains_key("error");
    if let Some(Value::Object(mut m)) = data.remove("message") {
        if m.contains_key("toolCallId") {
            tool.call_id = take_str(&mut m, "toolCallId").unwrap_or_default();
            failed |= take::<bool>(&mut m, "isError").unwrap_or(false);
            tool.output = m.remove("content").map(flatten_text);
        } else if let Some(Value::Array(parts)) = m.remove("content") {
            let mut rest = Vec::new();
            for mut p in parts {
                let is_result = p.get("type").and_then(Value::as_str) == Some("tool-result") && tool.output.is_none();
                match p.as_object_mut().filter(|_| is_result) {
                    Some(pm) => {
                        tool.call_id = take_str(pm, "toolCallId").unwrap_or_default();
                        failed |= take::<bool>(pm, "isError").unwrap_or(false);
                        tool.output = pm.remove("content").map(flatten_text);
                        pm.remove("type");
                        if !pm.is_empty() {
                            rest.push(p);
                        }
                    }
                    None => rest.push(p),
                }
            }
            if !rest.is_empty() {
                m.insert("content".into(), Value::Array(rest));
            }
        }
        if tool.call_id.is_empty()
            && let Some(Value::Object(src)) = m.get_mut("source")
        {
            tool.call_id = take_str(src, "callId").unwrap_or_default();
        }
        data.insert("message".into(), Value::Object(m));
    }
    tool.status = Some(if failed { "error" } else { "ok" }.into());
    tool
}

/// 全是纯 text part 的数组拼成一个字符串（UI 直接显示）；有别的形状就原样
fn flatten_text(v: Value) -> Value {
    let Value::Array(parts) = &v else { return v };
    let texts: Option<Vec<&str>> = parts
        .iter()
        .map(|p| match p.as_object() {
            Some(m) if m.len() == 2 && m.get("type").and_then(Value::as_str) == Some("text") => {
                m.get("text").and_then(Value::as_str)
            }
            _ => None,
        })
        .collect();
    match texts {
        Some(t) => Value::String(t.join("\n")),
        None => v,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const PATH: &str = "/home/me/.dsh/sessions/--E-proj--/session-abc/session.v3.jsonl.zstd";

    fn run(lines: &[&str]) -> Vec<Event> {
        super::super::run(&PROVIDER, PATH, lines)
    }

    // turn / step 编号、毫秒时间戳、人打的 user 消息和注入的上下文分开
    #[test]
    fn turn_step_and_user_messages() {
        let evs = run(&[
            &json!({"type":"turn/start","seq":4,"time":1786885692630i64,"data":{"turn":1}}).to_string(),
            &json!({"type":"step/start","seq":6,"time":1786885692675i64,"data":{"turn":1,"step":2}}).to_string(),
            &json!({"type":"user/message","seq":8,"time":1786885692675i64,"surfaceOp":"append","data":{
                "content":[{"type":"text","text":"帮我写"}],"source":{"kind":"user"},"role":"user","id":"u1"}})
            .to_string(),
            &json!({"type":"user/message","seq":9,"data":{"content":[{"type":"text","text":"runtime ctx"}],
                "source":{"kind":"plugin"},"role":"user"}})
            .to_string(),
        ]);
        assert_eq!(evs[0].kind, Kind::TurnStarted);
        assert_eq!(evs[0].turn, Some(Scope { id: "1".into(), n: Some(1) }));
        assert_eq!(evs[0].ts, DateTime::from_timestamp_millis(1786885692630));
        assert_eq!(evs[1].kind, Kind::StepStarted);
        assert_eq!(evs[1].step, Some(Scope { id: "1.2".into(), n: Some(2) }));
        assert_eq!(evs[2].kind, Kind::Message);
        assert_eq!(evs[2].role, Some(Role::User));
        assert_eq!(evs[2].text.as_deref(), Some("帮我写"));
        assert_eq!(evs[2].ext["data"], json!({"source":{"kind":"user"},"role":"user","id":"u1"}));
        assert_eq!(evs[3].kind, Kind::RequestHeader);
        assert_eq!(evs[3].role, None);
        assert_eq!(evs[3].text.as_deref(), Some("runtime ctx"));
        // 不带编号的行归到当前 turn / step，并声明是推断的
        assert_eq!(evs[2].turn, Some(Scope { id: "1".into(), n: Some(1) }));
        assert_eq!(evs[2].step, Some(Scope { id: "1.2".into(), n: Some(2) }));
        assert_eq!(evs[2].inferred, vec!["turn", "step"]);
        assert!(evs[1].inferred.is_empty());
    }

    // turn/end、step/end 之后的行不再归进去
    #[test]
    fn scope_closes_at_end() {
        let evs = run(&[
            &json!({"type":"turn/start","data":{"turn":1}}).to_string(),
            &json!({"type":"step/start","data":{"turn":1,"step":1}}).to_string(),
            &json!({"type":"step/end","data":{"turn":1,"step":1}}).to_string(),
            &json!({"type":"user/message","data":{"source":{"kind":"user"}}}).to_string(),
            &json!({"type":"turn/end","data":{"turn":1}}).to_string(),
            &json!({"type":"agent/inbox/spliced","data":{}}).to_string(),
        ]);
        assert_eq!(evs[3].turn.as_ref().map(|t| t.id.as_str()), Some("1"));
        assert_eq!(evs[3].step, None);
        assert_eq!(evs[3].inferred, vec!["turn"]);
        assert_eq!(evs[5].turn, None);
    }

    // assistant：text / reasoning 拆开，tool-call part 留下，model 和 usage 拿到
    #[test]
    fn assistant_message() {
        let line = json!({"type":"assistant/message","seq":17,"time":1,"data":{"turn":1,"step":1,"message":{
            "role":"assistant","content":[{"type":"reasoning","text":"想"},{"type":"text","text":"答"},
            {"type":"tool-call","id":"c1","name":"skill","arguments":"{}"}],
            "source":{"kind":"model","provider":"deepseek-official","model":"deepseek-v4-pro"},"id":"m1"},
            "usage":{"inputTokens":8700,"outputTokens":118,"cacheReadTokens":1280,"reasoningTokens":72}}})
        .to_string();
        let ev = run(&[&line]).remove(0);
        assert_eq!((ev.kind, ev.role), (Kind::Message, Some(Role::Assistant)));
        assert_eq!(ev.text.as_deref(), Some("答"));
        assert_eq!(ev.reasoning.as_deref(), Some("想"));
        assert_eq!(ev.model.as_deref(), Some("deepseek-v4-pro"));
        let u = ev.usage.expect("usage");
        assert_eq!((u.input, u.output, u.cache_read, u.cache_write), (Some(8700), Some(118), Some(1280), None));
        assert_eq!(u.ext, json!({"reasoningTokens":72}).as_object().unwrap().clone());
        assert_eq!(
            ev.ext["data"]["message"],
            json!({"role":"assistant","content":[{"type":"tool-call","id":"c1","name":"skill","arguments":"{}"}],
                "source":{"kind":"model","provider":"deepseek-official"},"id":"m1"})
        );
    }

    // 走 Anthropic 路由时 adapter 报 cacheWrite1hTokens（cacheWriteTokens 的子集），超出总数的截掉
    #[test]
    fn long_cache_writes() {
        let usage = |w: u64, h: u64| {
            json!({"type":"assistant/message","seq":1,"time":1,"data":{"turn":1,"step":1,
                "message":{"role":"assistant","content":[],"id":"m1"},
                "usage":{"inputTokens":1,"cacheWriteTokens":w,"cacheWrite1hTokens":h}}})
            .to_string()
        };
        let u = run(&[&usage(100, 60)]).remove(0).usage.expect("usage");
        assert_eq!((u.cache_write, u.cache_write_1h), (Some(100), Some(60)));
        assert!(u.ext.is_empty());
        let u = run(&[&usage(100, 300)]).remove(0).usage.expect("usage");
        assert_eq!(u.cache_write_1h, Some(100));
    }

    // tool 调用两端：arguments 解析成 JSON，结果的 text part 拼成字符串，isError / data.error 是失败
    #[test]
    fn tool_call_and_result() {
        let evs = run(&[
            &json!({"type":"tool/call","seq":18,"time":1,"data":{"turn":1,"step":1,"callId":"c1","name":"pwsh",
                "arguments":"{\"command\": \"ls\"}"}})
            .to_string(),
            &json!({"type":"tool/result","seq":19,"time":2,"data":{"turn":1,"step":1,"message":{
                "source":{"kind":"tool","callId":"c1"},"content":[{"type":"tool-result","toolCallId":"c1",
                "isError":false,"content":[{"type":"text","text":"a"},{"type":"text","text":"b"}]}]}}})
            .to_string(),
            &json!({"type":"tool/result","seq":20,"time":3,"data":{"turn":1,"step":1,"error":{"kind":"x"},"message":{
                "content":[{"type":"tool-result","toolCallId":"c2","isError":false,"content":[{"type":"image"}]}]}}})
            .to_string(),
        ]);
        let t = evs[0].tool.as_ref().unwrap();
        assert_eq!((evs[0].kind, t.call_id.as_str(), t.name.as_str()), (Kind::ToolCall, "c1", "pwsh"));
        assert_eq!(t.input, Some(json!({"command":"ls"})));
        assert_eq!(t.category, Some(crate::event::ToolCategory::Shell));
        let t = evs[1].tool.as_ref().unwrap();
        assert_eq!((evs[1].kind, evs[1].role), (Kind::ToolResult, Some(Role::Tool)));
        assert_eq!((t.call_id.as_str(), t.output.as_ref(), t.status.as_deref()), ("c1", Some(&json!("a\nb")), Some("ok")));
        assert_eq!(evs[1].ext["data"]["message"], json!({"source":{"kind":"tool","callId":"c1"}}));
        let t = evs[2].tool.as_ref().unwrap();
        assert_eq!((t.call_id.as_str(), t.status.as_deref()), ("c2", Some("error")));
        assert_eq!(t.output, Some(json!([{"type":"image"}])));
    }

    // v4 的 tool/result：toolCallId / isError 在 message 上，content 直接是结果 part
    #[test]
    fn tool_result_v4() {
        let evs = run(&[
            &json!({"type":"tool/result","seq":33,"time":2,"data":{"turn":1,"step":1,"message":{"role":"tool",
                "source":{"kind":"tool","callId":"c1"},"toolCallId":"c1","content":[{"type":"text","text":"a"},
                {"type":"text","text":"b"}],"isError":false,"id":"r1"},"meta":{"cwd":"/p"}},
                "sourceEventSeqs":[30],"surfaceOp":"append"})
            .to_string(),
            &json!({"type":"tool/result","seq":40,"time":3,"data":{"turn":1,"step":2,"message":{"role":"tool",
                "source":{"kind":"tool","callId":"c2"},"toolCallId":"c2","content":[{"type":"text","text":"shot"},
                {"type":"image","attachment":{"attachmentId":"att1","mediaType":"image/png","width":9,"height":11}}],
                "isError":false,"id":"r2"}}})
            .to_string(),
            &json!({"type":"tool/result","seq":41,"time":4,"data":{"turn":1,"step":2,"message":{"role":"tool",
                "source":{"kind":"tool","callId":"c3"},"toolCallId":"c3","content":[{"type":"text","text":"denied"}],
                "isError":true,"id":"r3"},"error":{"name":"ToolError","code":"x"}}})
            .to_string(),
        ]);
        let t = evs[0].tool.as_ref().unwrap();
        assert_eq!((evs[0].kind, evs[0].role), (Kind::ToolResult, Some(Role::Tool)));
        assert_eq!((t.call_id.as_str(), t.output.as_ref(), t.status.as_deref()), ("c1", Some(&json!("a\nb")), Some("ok")));
        assert_eq!(
            evs[0].ext["data"]["message"],
            json!({"role":"tool","source":{"kind":"tool","callId":"c1"},"id":"r1"})
        );
        assert_eq!(evs[0].ext["data"]["meta"], json!({"cwd":"/p"}));
        let t = evs[1].tool.as_ref().unwrap();
        assert_eq!(t.call_id, "c2");
        assert_eq!(t.output.as_ref().and_then(Value::as_array).map(Vec::len), Some(2));
        assert_eq!(t.output.as_ref().unwrap()[1]["type"], json!("image"));
        let t = evs[2].tool.as_ref().unwrap();
        assert_eq!((t.call_id.as_str(), t.output.as_ref(), t.status.as_deref()), ("c3", Some(&json!("denied")), Some("error")));
    }

    // 压缩记录带 signal；developer/message 是注入的上下文
    #[test]
    fn compaction_and_developer_message() {
        let evs = run(&[
            &json!({"type":"compaction/summary","seq":40,"time":1,"data":{"shadowedSeqs":[8,9],"shadowedTokenCount":900}})
                .to_string(),
            &json!({"type":"developer/message","seq":41,"time":2,"data":{"message":{"role":"developer",
                "content":[{"type":"text","text":"plan mode on"}],"source":{"kind":"plan"}}}})
            .to_string(),
        ]);
        assert_eq!(evs[0].signal, Some(Signal::Compaction));
        assert_eq!(evs[0].ext["data"]["shadowedSeqs"], json!([8, 9]));
        assert_eq!(evs[1].kind, Kind::RequestHeader);
        assert_eq!(evs[1].text.as_deref(), Some("plan mode on"));
    }

    // 子 agent：会话 id 是目录名，父会话从首行头里拿，之后每行都带
    #[test]
    fn subagent_parent_from_header() {
        let path = "/home/me/.dsh/sessions/--E-p--/63ca/session.jsonl.zstd";
        let evs = super::super::run(&PROVIDER, path, &[
            &json!({"type":"session","version":0,"id":"63ca","createdAt":1786636196469i64,
                "parentSession":"session-cecd","origin":"subagent","delegationDepth":1}).to_string(),
            &json!({"type":"turn/start","seq":0,"time":1,"data":{"turn":1}}).to_string(),
        ]);
        assert_eq!(evs[0].session, "63ca");
        assert_eq!(evs[0].ts, DateTime::from_timestamp_millis(1786636196469));
        assert_eq!(evs[0].parent_session.as_deref(), Some("session-cecd"));
        assert_eq!(evs[1].parent_session.as_deref(), Some("session-cecd"));
    }

    // v0 可被 v4 / v3 取代，v3 可被 v4 取代，v4 是最新的；别的文件名不参与
    #[test]
    fn superseded_by_newer_versions() {
        let d = "/h/.dsh/sessions/--E--/session-a";
        let f = |v: &str| format!("{d}/{v}");
        assert_eq!((PROVIDER.superseded_by)(&f("session.jsonl.zstd")), vec![f("session.v4.jsonl.zstd"), f("session.v3.jsonl.zstd")]);
        assert_eq!((PROVIDER.superseded_by)(&f("session.v3.jsonl.zstd")), vec![f("session.v4.jsonl.zstd")]);
        assert!((PROVIDER.superseded_by)(&f("session.v4.jsonl.zstd")).is_empty());
        assert!((PROVIDER.superseded_by)(&f("other.jsonl.zstd")).is_empty());
    }
}
