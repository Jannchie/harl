//! Codex CLI 的 rollout → Event。
//!
//! 文件：`~/.codex/sessions/YYYY/MM/DD/rollout-<时间>-<uuid>.jsonl`（归档的在 `archived_sessions/` 下平铺）。
//! 会话 id 是文件名末尾的 uuid。三种行形状：
//! - 现在的：`{timestamp, type, payload}`，type 是 session_meta / response_item / event_msg / turn_context /
//!   token_usage_record / compacted …，payload 里的 `type` 再分一层
//! - 2025-09 以前的：首行 `{id, timestamp, instructions}`，之后 response item 裸写一行（没有信封、没有时间戳），
//!   夹着 `{record_type: "state"}`
//!
//! turn 是显式的：`event_msg/task_started` … `task_complete` / `turn_aborted`，带 `turn_id`。
//! 老格式没有，按"真 user 消息开一个 turn"推断。
//!
//! step（一次模型调用）是推断的：日志不写调用边界，只在每次调用结束后报一次 usage。
//! 一步 = 上一份 usage 之后的第一条模型输出（reasoning / assistant 消息 / 工具调用）到下一份 usage；
//! 那条 usage 行就是 step_completed。usage 有两种来源：
//! - `token_usage_record`（0.153 起，带 response_id，每次调用一条）：有它就只认它
//! - `event_msg/token_count`：`last_token_usage` 是这次调用的，但同一份会重复报（恢复会话、限流刷新时），
//!   累计值 `total_token_usage` 没变的就是重复
//!
//! OpenAI 的 `input_tokens` 含缓存读写，这里换成 Anthropic 口径（input 只算新输入），和别家可比。
//!
//! 以 user 角色写进来但不是用户在说话的（AGENTS.md、environment_context、中断标记……）按前缀分到 request_header。
//! 子 agent 的文件：首个 session_meta 是自己（`parent_thread_id` 指父会话），接着是父会话的 session_meta
//! 和 fork 时带过来的父会话历史，到子 agent 自己的第一个 task_started 为止。这段是子 agent 收到的上下文，
//! 记为 request_header，不进 turn，`links: replay_of` 指父会话。

use chrono::{DateTime, Utc};
use serde_json::{Map, Value};

use super::{LineNormalizer, Provider, file_stem, join_texts, take, take_str, take_text_part, take_ts, tools};
use crate::event::{Event, Kind, Link, Prov, Role, Scope, Tool, Usage};

pub static PROVIDER: Provider = Provider {
    name: "codex",
    owns: |p| p.contains("/.codex/sessions/") || p.contains("/.codex/archived_sessions/"),
    default_roots: |home| vec![home.join(".codex").join("sessions"), home.join(".codex").join("archived_sessions")],
    new: |path| Box::new(Normalizer::new(path)),
    superseded_by: |_| None,
};

/// Codex 自己塞进 user 角色的上下文。名单来自对本机全部会话的统计（2026-09）和 codex-rs 的同类判断
const INJECTED_PREFIXES: &[&str] = &[
    "<environment_context>",
    "<user_instructions>",
    "# AGENTS.md instructions",
    "<INSTRUCTIONS>",
    "<turn_aborted>",
    "<user_shell_command>",
    "<subagent_notification>",
    "<skill>",
];

pub struct Normalizer {
    session: String,
    parent: Option<String>,
    /// 读到过自己的 session_meta
    seen_meta: bool,
    /// 从哪个会话 fork 来（子 agent 或用户手动 fork）
    forked_from: Option<String>,
    /// 自己的创建时间（秒），用来判断 fork 回放到哪里结束
    created_s: Option<i64>,
    /// 正在读 fork 带过来的父会话历史
    replay: bool,
    turn: Option<String>,
    /// 当前 turn 是按 user 消息推断的（老格式）
    turn_inferred: bool,
    step_n: u32,
    /// 上一步已经报过 usage，下一条模型输出开新的一步
    step_closed: bool,
    model: Option<String>,
    /// 见过 token_usage_record，token_count 就只是重复
    has_records: bool,
    last_total: Option<u64>,
}

impl Normalizer {
    pub fn new(path: &str) -> Self {
        let stem = file_stem(path);
        // rollout-<时间>-<uuid>：uuid 是最后 36 个字符
        let session = match stem.len().checked_sub(36).filter(|&i| stem.is_char_boundary(i)) {
            Some(i) if stem.starts_with("rollout-") => &stem[i..],
            _ => stem,
        };
        Self {
            session: session.to_string(),
            parent: None,
            seen_meta: false,
            forked_from: None,
            created_s: None,
            replay: false,
            turn: None,
            turn_inferred: false,
            step_n: 0,
            step_closed: true,
            model: None,
            has_records: false,
            last_total: None,
        }
    }

    fn open_turn(&mut self, id: String, inferred: bool) {
        self.turn = Some(id);
        self.turn_inferred = inferred;
        self.step_n = 0;
        self.step_closed = true;
    }

    /// 自己的 session_meta：父会话、创建时间
    fn own_meta(&mut self, p: &mut Map<String, Value>, ts: Option<DateTime<Utc>>) {
        self.seen_meta = true;
        self.created_s = take_ts(p, "timestamp").or(ts).map(|t| t.timestamp());
        self.forked_from = take_str(p, "forked_from_id");
        self.parent = take_str(p, "parent_thread_id").or_else(|| {
            let spawn = p.get("source")?.get("subagent")?.get("thread_spawn")?;
            spawn.get("parent_thread_id")?.as_str().map(str::to_string)
        });
    }
}

impl LineNormalizer for Normalizer {
    fn identity(&self) -> (String, Option<String>) {
        (self.session.clone(), self.parent.clone())
    }

    fn line(&mut self, seq: u32, mut obj: Map<String, Value>, prov: Prov) -> Event {
        let ts = take_ts(&mut obj, "timestamp");
        // 信封：payload 拿出来单独处理，剩下的放回 ext。老格式没有信封，整行就是 payload
        let enveloped = obj.contains_key("payload");
        let (outer, mut p) = if enveloped {
            let outer = obj.get("type").and_then(Value::as_str).unwrap_or("").to_string();
            match obj.remove("payload") {
                Some(Value::Object(m)) => (outer, m),
                Some(v) => {
                    obj.insert("payload".into(), v);
                    (outer, Map::new())
                }
                None => unreachable!(),
            }
        } else {
            let outer = if obj.contains_key("instructions") && obj.contains_key("id") {
                "session_meta"
            } else if obj.contains_key("type") {
                "response_item"
            } else {
                ""
            };
            (outer.to_string(), std::mem::take(&mut obj))
        };
        let ty = p.get("type").and_then(Value::as_str).unwrap_or("").to_string();

        let mut ev = Event {
            provider: PROVIDER.name,
            session: self.session.clone(),
            seq,
            ts,
            prov,
            ..Default::default()
        };

        if outer == "session_meta" {
            let is_own = !self.seen_meta || p.get("id").and_then(Value::as_str) == Some(self.session.as_str());
            if is_own && !self.replay {
                self.own_meta(&mut p, ts);
            } else {
                self.replay = true;
            }
            // cwd 提到顶层：会话头按 ext.cwd 取工作目录
            if let Some(cwd) = p.remove("cwd") {
                obj.insert("cwd".into(), cwd);
            }
        }

        // fork 回放：直到自己的第一个 task_started（开始时间不早于自己的创建时间）
        if self.replay {
            let own_start = outer == "event_msg"
                && ty == "task_started"
                && p.get("started_at").and_then(Value::as_i64).zip(self.created_s).is_some_and(|(s, c)| s >= c);
            if own_start {
                self.replay = false;
            } else {
                if outer == "response_item" {
                    ev.kind = Kind::RequestHeader;
                    ev.text = take_texts(&mut p);
                }
                let from = self.forked_from.clone().or_else(|| self.parent.clone());
                ev.links.push(Link { rel: "replay_of", session: from, inferred: true, ..Default::default() });
                return finish(ev, obj, p, enveloped, self.parent.clone());
            }
        }

        let mut usage_line = false;
        match (outer.as_str(), ty.as_str()) {
            ("event_msg", "task_started") => {
                ev.kind = Kind::TurnStarted;
                let id = take_str(&mut p, "turn_id").unwrap_or_else(|| format!("t{seq}"));
                self.open_turn(id, false);
            }
            ("event_msg", "task_complete" | "turn_aborted") => ev.kind = Kind::TurnCompleted,
            ("turn_context", _) => {
                ev.kind = Kind::RequestHeader;
                if let Some(m) = p.get("model").and_then(Value::as_str) {
                    self.model = Some(m.to_string());
                }
                ev.model = self.model.clone();
                if let Some(cwd) = p.remove("cwd") {
                    obj.insert("cwd".into(), cwd);
                }
            }
            ("token_usage_record", _) => {
                self.has_records = true;
                ev.usage = take_usage(&mut p, "usage");
                usage_line = ev.usage.is_some();
            }
            ("event_msg", "token_count") => {
                if let Some(Value::Object(info)) = p.get_mut("info") {
                    let total = info.get("total_token_usage").and_then(|t| t.get("total_tokens")).and_then(Value::as_u64);
                    let fresh = total.is_some() && total != self.last_total;
                    self.last_total = total.or(self.last_total);
                    if fresh && !self.has_records {
                        ev.usage = take_usage(info, "last_token_usage");
                        usage_line = ev.usage.is_some();
                    }
                }
            }
            ("response_item", "message") => match p.get("role").and_then(Value::as_str) {
                Some("assistant") => {
                    ev.kind = Kind::Message;
                    ev.role = Some(Role::Assistant);
                    ev.text = take_texts(&mut p);
                }
                Some("user") => {
                    ev.text = take_texts(&mut p);
                    let injected =
                        ev.text.as_deref().is_some_and(|t| INJECTED_PREFIXES.iter().any(|x| t.trim_start().starts_with(x)));
                    if injected {
                        ev.kind = Kind::RequestHeader;
                    } else {
                        ev.kind = Kind::Message;
                        ev.role = Some(Role::User);
                        if self.turn.is_none() || self.turn_inferred {
                            self.open_turn(format!("u{seq}"), true);
                        }
                    }
                }
                // developer / system
                _ => {
                    ev.kind = Kind::RequestHeader;
                    ev.text = take_texts(&mut p);
                }
            },
            // 子 agent 之间的消息：对收到的一方是上下文
            ("response_item", "agent_message") => {
                ev.kind = Kind::RequestHeader;
                ev.text = take_texts(&mut p);
            }
            ("response_item", "reasoning") => {
                ev.kind = Kind::Message;
                ev.role = Some(Role::Assistant);
                ev.reasoning = take_reasoning(&mut p);
            }
            ("response_item", "function_call" | "custom_tool_call") => {
                ev.kind = Kind::ToolCall;
                ev.role = Some(Role::Assistant);
                let name = take_str(&mut p, "name").unwrap_or_default();
                // function_call 的 arguments 是 JSON 字符串；custom_tool_call 的 input 是原文（apply_patch 的补丁）
                let input = p.remove("arguments").map(parse_json_string).or_else(|| p.remove("input"));
                ev.tool = Some(Tool {
                    call_id: take_str(&mut p, "call_id").unwrap_or_default(),
                    category: Some(tools::categorize(&name)),
                    name,
                    input,
                    output: None,
                    status: None,
                });
            }
            ("response_item", "function_call_output" | "custom_tool_call_output") => {
                ev.kind = Kind::ToolResult;
                ev.role = Some(Role::Tool);
                let (output, failed) = match p.remove("output") {
                    Some(o) => tool_output(o),
                    None => (None, false),
                };
                ev.tool = Some(Tool {
                    call_id: take_str(&mut p, "call_id").unwrap_or_default(),
                    output,
                    status: Some(if failed { "error" } else { "ok" }.into()),
                    ..Default::default()
                });
            }
            // 服务端工具：只有调用，没有单独的结果行
            ("response_item", "web_search_call" | "image_generation_call") => {
                ev.kind = Kind::ToolCall;
                ev.role = Some(Role::Assistant);
                let name = ty.trim_end_matches("_call").to_string();
                ev.tool = Some(Tool {
                    call_id: take_str(&mut p, "id").unwrap_or_default(),
                    category: Some(tools::categorize(&name)),
                    name,
                    input: p.remove("action"),
                    output: None,
                    status: take_str(&mut p, "status"),
                });
            }
            _ => {}
        }

        let model_output = ev.role == Some(Role::Assistant);
        if model_output {
            ev.model = self.model.clone();
        }
        if let Some(turn) = &self.turn {
            if (model_output || usage_line) && self.step_closed {
                self.step_n += 1;
                self.step_closed = false;
            }
            ev.turn = Some(Scope { id: turn.clone(), n: None });
            if self.turn_inferred {
                ev.inferred.push("turn");
            }
            if !self.step_closed {
                ev.step = Some(Scope { id: format!("{turn}.{}", self.step_n), n: Some(self.step_n) });
                ev.inferred.push("step");
            }
        }
        if usage_line {
            ev.kind = Kind::StepCompleted;
            ev.model = self.model.clone();
            self.step_closed = true;
        }
        if ev.kind == Kind::TurnCompleted {
            self.turn = None;
        }
        finish(ev, obj, p, enveloped, self.parent.clone())
    }
}

/// 没映射的 payload 放回信封（老格式直接就是 ext）
fn finish(
    mut ev: Event,
    mut obj: Map<String, Value>,
    p: Map<String, Value>,
    enveloped: bool,
    parent: Option<String>,
) -> Event {
    if enveloped {
        if !p.is_empty() {
            obj.insert("payload".into(), Value::Object(p));
        }
    } else {
        obj.extend(p);
    }
    ev.parent_session = parent;
    ev.ext = obj;
    ev
}

/// usage 换成 Anthropic 口径：OpenAI 的 input_tokens 含缓存读写，减掉
fn take_usage(m: &mut Map<String, Value>, key: &str) -> Option<Usage> {
    let Some(Value::Object(mut u)) = m.remove(key) else { return None };
    let input: Option<u64> = take(&mut u, "input_tokens");
    let cache_read: Option<u64> = take(&mut u, "cached_input_tokens");
    let cache_write: Option<u64> = take(&mut u, "cache_write_input_tokens");
    let cached = cache_read.unwrap_or(0) + cache_write.unwrap_or(0);
    Some(Usage {
        input: input.map(|i| i.saturating_sub(cached)),
        output: take(&mut u, "output_tokens"),
        cache_read,
        cache_write,
        ext: u,
    })
}

/// message 的 content：input_text / output_text 拼成正文，其余 part 留下
fn take_texts(m: &mut Map<String, Value>) -> Option<String> {
    let Some(Value::Array(parts)) = m.remove("content") else { return None };
    let (mut text, mut rest) = (Vec::new(), Vec::new());
    for part in parts {
        match part.get("type").and_then(Value::as_str) {
            Some("input_text" | "output_text" | "text") => take_text_part(part, "text", &mut text, &mut rest),
            _ => rest.push(part),
        }
    }
    if !rest.is_empty() {
        m.insert("content".into(), Value::Array(rest));
    }
    join_texts(text)
}

/// reasoning：summary 是给人看的摘要，content（有的话）是原文推理；加密的原样留着
fn take_reasoning(m: &mut Map<String, Value>) -> Option<String> {
    let mut texts = Vec::new();
    for key in ["summary", "content"] {
        match m.remove(key) {
            Some(Value::Array(parts)) => {
                let mut rest = Vec::new();
                for part in parts {
                    take_text_part(part, "text", &mut texts, &mut rest);
                }
                if !rest.is_empty() {
                    m.insert(key.into(), Value::Array(rest));
                }
            }
            Some(Value::Null) | None => {}
            Some(v) => {
                m.insert(key.into(), v);
            }
        }
    }
    join_texts(texts)
}

fn parse_json_string(v: Value) -> Value {
    match v {
        Value::String(s) => serde_json::from_str(&s).unwrap_or(Value::String(s)),
        v => v,
    }
}

/// 工具输出和是否失败。三种形状：
/// - 老版本：JSON 字符串 `{"output": "...", "metadata": {"exit_code": n}}`
/// - 现在：纯文本，命令类的带 `Process exited with code n` 头
/// - 图片等：content part 数组，原样
fn tool_output(o: Value) -> (Option<Value>, bool) {
    let Value::String(s) = o else { return (Some(o), false) };
    if let Ok(Value::Object(mut m)) = serde_json::from_str::<Value>(&s)
        && let Some(Value::String(out)) = m.remove("output")
    {
        let code = m.get("metadata").and_then(|x| x.get("exit_code")).and_then(Value::as_i64);
        return (Some(Value::String(out)), code.is_some_and(|c| c != 0));
    }
    let code = s
        .lines()
        .take(6)
        .find_map(|l| l.strip_prefix("Process exited with code "))
        .and_then(|c| c.trim().parse::<i64>().ok());
    (Some(Value::String(s)), code.is_some_and(|c| c != 0))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::stats::{Tokens, session_stats};

    const UUID: &str = "019e3914-64b7-7983-aae2-89785397cc58";

    fn path() -> String {
        format!("/home/me/.codex/sessions/2026/05/18/rollout-2026-05-18T12-14-58-{UUID}.jsonl")
    }

    fn run(lines: &[Value]) -> Vec<Event> {
        let lines: Vec<String> = lines.iter().map(Value::to_string).collect();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        super::super::run(&PROVIDER, &path(), &refs)
    }

    fn at(s: u32) -> String {
        format!("2026-05-18T03:15:{s:02}.000Z")
    }

    fn line(s: u32, ty: &str, payload: Value) -> Value {
        json!({"timestamp": at(s), "type": ty, "payload": payload})
    }

    fn token_count(s: u32, total: u64, input: u64, cached: u64, output: u64) -> Value {
        line(s, "event_msg", json!({"type":"token_count","info":{
            "total_token_usage":{"total_tokens":total},
            "last_token_usage":{"input_tokens":input,"cached_input_tokens":cached,"output_tokens":output,
                "reasoning_output_tokens":1,"total_tokens":input+output}}}))
    }

    /// 一个典型 turn：注入的上下文、用户消息、两次模型调用（中间一次工具），token_count 重复一次
    fn one_turn() -> Vec<Value> {
        vec![
            line(0, "session_meta", json!({"id":UUID,"timestamp":at(0),"cwd":"E:\\proj","originator":"codex-tui"})),
            line(1, "event_msg", json!({"type":"task_started","turn_id":"t1","started_at":1})),
            line(1, "response_item", json!({"type":"message","role":"developer","content":[{"type":"input_text","text":"<permissions>"}]})),
            line(1, "response_item", json!({"type":"message","role":"user","content":[{"type":"input_text","text":"# AGENTS.md instructions for E:\\proj"}]})),
            line(1, "turn_context", json!({"turn_id":"t1","cwd":"E:\\proj","model":"gpt-5.5","effort":"xhigh"})),
            line(1, "response_item", json!({"type":"message","role":"user","content":[{"type":"input_text","text":"看下 pod"}]})),
            line(2, "event_msg", json!({"type":"token_count","info":null})),
            line(5, "response_item", json!({"type":"reasoning","summary":[{"type":"summary_text","text":"想"}],"content":null,"encrypted_content":"x"})),
            line(6, "response_item", json!({"type":"function_call","name":"exec_command","arguments":"{\"cmd\":\"kubectl\"}","call_id":"c1"})),
            token_count(6, 100, 80, 50, 20),
            line(9, "response_item", json!({"type":"function_call_output","call_id":"c1","output":"Chunk ID: 1\nWall time: 1 seconds\nProcess exited with code 1\nOutput:\nboom"})),
            line(12, "response_item", json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"查不到"}]})),
            token_count(12, 190, 70, 60, 20),
            line(12, "event_msg", json!({"type":"task_complete","turn_id":"t1"})),
            // 下一轮开头重报一次同样的累计值：不是新的一步
            line(20, "event_msg", json!({"type":"task_started","turn_id":"t2","started_at":20})),
            token_count(20, 190, 70, 60, 20),
        ]
    }

    #[test]
    fn identity_from_file_name() {
        let n = Normalizer::new(&path());
        assert_eq!(n.identity(), (UUID.to_string(), None));
    }

    #[test]
    fn turn_steps_and_roles() {
        let evs = run(&one_turn());
        let kinds: Vec<Kind> = evs.iter().map(|e| e.kind).collect();
        use Kind::*;
        assert_eq!(
            kinds,
            vec![
                Meta,
                TurnStarted,
                RequestHeader,
                RequestHeader,
                RequestHeader,
                Message,
                Meta,
                Message,
                ToolCall,
                StepCompleted,
                ToolResult,
                Message,
                StepCompleted,
                TurnCompleted,
                TurnStarted,
                Meta
            ]
        );
        assert_eq!(evs[0].ext["cwd"], json!("E:\\proj"));
        assert_eq!((evs[5].role, evs[5].text.as_deref()), (Some(Role::User), Some("看下 pod")));
        assert_eq!(evs[7].reasoning.as_deref(), Some("想"));
        assert_eq!(evs[7].model.as_deref(), Some("gpt-5.5"));
        let t = evs[8].tool.as_ref().unwrap();
        assert_eq!((t.name.as_str(), t.call_id.as_str()), ("exec_command", "c1"));
        assert_eq!(t.input, Some(json!({"cmd":"kubectl"})));
        assert_eq!(evs[10].tool.as_ref().unwrap().status.as_deref(), Some("error"));
        // step：reasoning 开第 1 步，usage 关；工具结果在两步之间；assistant 消息开第 2 步
        let step = |i: usize| evs[i].step.as_ref().map(|s| s.id.clone());
        assert_eq!(step(7), Some("t1.1".into()));
        assert_eq!(step(9), Some("t1.1".into()));
        assert_eq!(step(10), None);
        assert_eq!(step(11), Some("t1.2".into()));
        assert_eq!(evs[11].turn.as_ref().unwrap().id, "t1");
        assert!(evs[11].inferred.contains(&"step") && !evs[11].inferred.contains(&"turn"));
        // input 去掉缓存读
        let u = evs[9].usage.as_ref().unwrap();
        assert_eq!((u.input, u.cache_read, u.output), (Some(30), Some(50), Some(20)));
        assert_eq!(u.ext["reasoning_output_tokens"], json!(1));
    }

    #[test]
    fn stats_count_each_call_once() {
        let s = session_stats(&run(&one_turn()));
        assert_eq!((s.turns, s.steps, s.tool_calls, s.tool_failures), (2, 2, 1, 1));
        assert_eq!(s.tokens, Tokens { input: 40, output: 40, cache_read: 110, cache_write: 0 });
        assert_eq!(s.by_model["gpt-5.5"].steps, 2);
        assert_eq!(s.by_tool["exec_command"].category, crate::event::ToolCategory::Shell);
        // 第 1 步从它前一个事件（00:02 的空 token_count）到 00:06，第 2 步 00:09 → 00:12
        assert_eq!(s.model_ms, 7000);
        assert_eq!(s.tool_ms, 3000);
    }

    // 有 token_usage_record 就只认它，token_count 不再算
    #[test]
    fn usage_records_win_over_token_count() {
        let rec = |s: u32, input: u64| {
            line(s, "token_usage_record", json!({"turn_id":"t1","response_id":format!("r{s}"),
                "usage":{"input_tokens":input,"cached_input_tokens":0,"output_tokens":1,"total_tokens":input+1}}))
        };
        let s = session_stats(&run(&[
            line(0, "event_msg", json!({"type":"task_started","turn_id":"t1","started_at":0})),
            line(0, "turn_context", json!({"model":"gpt-6"})),
            line(1, "response_item", json!({"type":"custom_tool_call","name":"apply_patch","input":"*** Begin Patch","call_id":"c1"})),
            rec(1, 10),
            line(2, "response_item", json!({"type":"custom_tool_call_output","call_id":"c1",
                "output":"{\"output\":\"Success\",\"metadata\":{\"exit_code\":0}}"})),
            token_count(2, 11, 10, 0, 1),
            line(3, "response_item", json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"好"}]})),
            rec(3, 20),
            token_count(3, 32, 20, 0, 1),
        ]));
        assert_eq!(s.steps, 2);
        assert_eq!(s.tokens.input, 30);
        assert_eq!(s.by_tool["apply_patch"].failures, 0);
    }

    // 子 agent：自己的 meta 给父会话；父会话的 meta 和历史是回放，到自己的 task_started 为止
    #[test]
    fn subagent_fork_replay() {
        let child = "01a07ac3-dae5-7772-a4b2-4ee85a953d1f";
        let parent = "01a07ac3-1ae1-78a3-850a-300a06752a25";
        let p = format!("/h/.codex/sessions/2026/09/07/rollout-2026-09-07T16-27-31-{child}.jsonl");
        let lines = [
            json!({"timestamp":"2026-09-07T07:27:31.265Z","type":"session_meta","payload":{"id":child,
                "timestamp":"2026-09-07T07:27:31.144Z","parent_thread_id":parent,"cwd":"E:\\k"}}),
            json!({"timestamp":"2026-09-07T07:27:31.265Z","type":"session_meta","payload":{"id":parent,"cwd":"E:\\k"}}),
            json!({"timestamp":"2026-09-07T07:27:31.265Z","type":"event_msg","payload":{"type":"task_started","turn_id":"pt","started_at":1788766025}}),
            json!({"timestamp":"2026-09-07T07:27:31.265Z","type":"response_item","payload":{"type":"message","role":"user",
                "content":[{"type":"input_text","text":"父会话的问题"}]}}),
            json!({"timestamp":"2026-09-07T07:27:31.312Z","type":"event_msg","payload":{"type":"task_started","turn_id":"ct","started_at":1788766051}}),
            json!({"timestamp":"2026-09-07T07:27:33.523Z","type":"response_item","payload":{"type":"agent_message",
                "content":[{"type":"input_text","text":"去看看"}]}}),
        ]
        .map(|v| v.to_string());
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let evs = super::super::run(&PROVIDER, &p, &refs);
        assert!(evs.iter().all(|e| e.session == child && e.parent_session.as_deref() == Some(parent)));
        assert_eq!(evs[3].kind, Kind::RequestHeader);
        assert_eq!(evs[3].text.as_deref(), Some("父会话的问题"));
        assert_eq!(evs[2].turn, None);
        assert_eq!(evs[2].links[0].rel, "replay_of");
        assert_eq!(evs[4].kind, Kind::TurnStarted);
        assert_eq!(evs[5].turn.as_ref().unwrap().id, "ct");
        assert!(evs[5].links.is_empty());
    }

    // 2025-09 以前：没有信封和时间戳，turn 按真 user 消息推断
    #[test]
    fn legacy_bare_lines() {
        let evs = run(&[
            json!({"id":UUID,"timestamp":"2025-09-07T15:40:42.693Z","instructions":null,"git":{}}),
            json!({"record_type":"state"}),
            json!({"type":"message","id":null,"role":"user","content":[{"type":"input_text","text":"<environment_context>\n</environment_context>"}]}),
            json!({"type":"message","id":null,"role":"user","content":[{"type":"input_text","text":"改一下"}]}),
            json!({"type":"function_call","id":"fc","name":"shell","arguments":"{\"command\":[\"ls\"]}","call_id":"c1"}),
            json!({"type":"function_call_output","call_id":"c1","output":"{\"output\":\"x\",\"metadata\":{\"exit_code\":2}}"}),
            json!({"type":"message","id":null,"role":"user","content":[{"type":"input_text","text":"再改"}]}),
        ]);
        assert_eq!(evs[0].ts.map(|t| t.timestamp_millis()), Some(1757259642693));
        assert_eq!(evs[2].kind, Kind::RequestHeader);
        assert_eq!(evs[2].turn, None);
        assert_eq!(evs[3].turn.as_ref().unwrap().id, "u3");
        assert_eq!(evs[3].inferred, vec!["turn"]);
        assert_eq!(evs[4].tool.as_ref().unwrap().category, Some(crate::event::ToolCategory::Shell));
        assert_eq!(evs[5].tool.as_ref().unwrap().output, Some(json!("x")));
        assert_eq!(evs[5].tool.as_ref().unwrap().status.as_deref(), Some("error"));
        assert_eq!(evs[6].turn.as_ref().unwrap().id, "u6");
        assert_eq!(evs[4].ext, json!({"type":"function_call","id":"fc"}).as_object().unwrap().clone());
    }
}
