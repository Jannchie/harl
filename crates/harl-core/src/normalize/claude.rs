//! Claude Code 会话 JSONL → Event。
//!
//! turn / step 优先用日志里的显式信号，没有才推断，推断的在 `inferred` 里声明：
//! - turn：`promptId`（同一 turn 的 user 提示行和所有 tool_result 行共享）；老日志没有，退回"真 user 行的 uuid"。
//!   `system/turn_duration` 是显式的 turn 结束。
//! - step：assistant 行的 `requestId` 就是一次模型调用；同 requestId 的若干行同属一步。
//!
//! 以 user 角色写进日志但不是用户在说话的东西按 `origin.kind` / 前缀分流：
//! - `task-notification`：后台任务完成通知，`links` 指回原 tool_call（`<tool-use-id>`）
//! - `coordinator`：父会话 SendMessage 送来的消息，`links` 指向父会话
//! - `promptSource=queued`：用户中途插话（steering），留在当前 turn，不开新 turn
//! - 斜杠命令回显、`!` 命令输出、system-reminder、中断标记 → Meta
//!
//! Agent 调用的 tool_result 带 `toolUseResult.agentId`，`links: spawned` 指向子会话 `agent-<id>`。
//!
//! 文件：`~/.claude/projects/<cwd-slug>/<session>.jsonl`，
//! subagent：`<session>/subagents/agent-<id>.jsonl`。

use serde_json::{Map, Value};

use super::{
    LineNormalizer, Provider, file_stem, join_texts, keep_rest, slashed, take_str, take_text_part, take_ts, take_usage,
    tools,
};
use crate::event::{Event, Kind, Link, Prov, Role, Scope, Tool};

pub static PROVIDER: Provider = Provider {
    name: "claude-code",
    owns: |p| p.contains("/.claude/projects/"),
    default_roots: |home| vec![home.join(".claude").join("projects")],
    new: |path| Box::new(Normalizer::new(path)),
};

/// 这些 attachment 描述的是"模型这一步实际收到了什么"
const REQUEST_HEADER_ATTACHMENTS: &[&str] = &[
    "environment",
    "model",
    "instructions",
    "prompt_snapshot",
    "deferred_tools_delta",
    "deferred_tools_record",
    "skill_listing",
    "mcp_instructions_delta",
    "agent_listing_delta",
    "nested_memory",
];

/// 以 user 角色写进日志、但不是用户在说话的东西：斜杠命令回显、`!` 命令的输出、
/// system-reminder、中断标记。它们不开启 turn，也没有模型回复跟着。
/// 名单来自对本机全部会话的统计（2026-09）。
const INJECTED_PREFIXES: &[&str] = &[
    "<command-name>",
    "<command-message>",
    "<local-command-stdout>",
    "<local-command-caveat>",
    "<bash-input>",
    "<bash-stdout>",
    "<bash-stderr>",
    "<system-reminder>",
    "[Request interrupted",
];

pub struct Normalizer {
    session: String,
    parent: Option<String>,
    turn: Option<Scope>,
    turn_inferred: bool,
    turn_n: u32,
    step: Option<Scope>,
    step_n: u32,
}

impl Normalizer {
    pub fn new(path: &str) -> Self {
        let norm = slashed(path);
        let parent = norm.rsplit_once("/subagents/").map(|(dir, _)| file_stem(dir).to_string());
        Self {
            session: file_stem(path).to_string(),
            parent,
            turn: None,
            turn_inferred: false,
            turn_n: 0,
            step: None,
            step_n: 0,
        }
    }

    fn begin_turn(&mut self, id: String, inferred: bool) {
        self.turn_n += 1;
        self.turn = Some(Scope { id, n: Some(self.turn_n) });
        self.turn_inferred = inferred;
        self.step = None;
        self.step_n = 0;
    }

    fn enter_step(&mut self, request_id: Option<String>) {
        let Some(id) = request_id else { return };
        if self.step.as_ref().is_some_and(|s| s.id == id) {
            return;
        }
        self.step_n += 1;
        self.step = Some(Scope { id, n: Some(self.step_n) });
    }

    /// 把当前 turn/step 贴到事件上，并声明推断
    fn attach_scope(&self, ev: &mut Event, with_step: bool) {
        ev.turn = self.turn.clone();
        if ev.turn.is_some() && self.turn_inferred {
            ev.inferred.push("turn");
        }
        if with_step {
            ev.step = self.step.clone();
            if ev.step.is_some() {
                ev.inferred.push("step");
            }
        }
    }
}

impl LineNormalizer for Normalizer {
    fn identity(&self) -> (String, Option<String>) {
        (self.session.clone(), self.parent.clone())
    }

    fn line(&mut self, seq: u32, mut obj: Map<String, Value>, prov: Prov) -> Event {
        // type 不映射到任何字段，留在 ext 里；这里只读
        let ty = obj.get("type").and_then(Value::as_str).unwrap_or("").to_string();
        let mut ev = Event {
            provider: PROVIDER.name,
            session: self.session.clone(),
            parent_session: self.parent.clone(),
            seq,
            ts: take_ts(&mut obj, "timestamp"),
            prov,
            ..Default::default()
        };

        match ty.as_str() {
            "user" => self.user_line(&mut ev, &mut obj),
            "assistant" => self.assistant_line(&mut ev, &mut obj),
            "system" if obj.get("subtype").and_then(Value::as_str) == Some("turn_duration") => {
                // turn 显式结束：这一行还属于它，之后的行不再挂上来
                ev.kind = Kind::TurnCompleted;
                self.attach_scope(&mut ev, false);
                self.turn = None;
                self.step = None;
            }
            "attachment" => {
                let at = obj.get("attachment").and_then(|a| a.get("type")).and_then(Value::as_str).unwrap_or("");
                if REQUEST_HEADER_ATTACHMENTS.contains(&at) {
                    ev.kind = Kind::RequestHeader;
                    self.attach_scope(&mut ev, true);
                }
            }
            _ => {}
        }
        ev.ext = obj;
        ev
    }
}

impl Normalizer {
    fn user_line(&mut self, ev: &mut Event, obj: &mut Map<String, Value>) {
        let Some(Value::Object(mut m)) = obj.remove("message") else { return };
        m.remove("role");
        let parts = Parts::take(&mut m, Role::User);
        if !m.is_empty() {
            obj.insert("message".into(), Value::Object(m));
        }
        let text = join_texts(parts.text);

        if let Some(tr) = parts.tool {
            // Agent 调用的结果指向子会话
            if let Some(agent) = obj.get("toolUseResult").and_then(|r| r.get("agentId")).and_then(Value::as_str) {
                ev.links.push(Link {
                    rel: "spawned",
                    session: Some(format!("agent-{}", agent.trim_start_matches("agent-"))),
                    call_id: Some(tr.call_id.clone()),
                    ..Default::default()
                });
            }
            ev.kind = Kind::ToolResult;
            ev.role = Some(Role::Tool);
            ev.tool = Some(tr);
            // 同一行里夹带的 text part（少见）跟着 tool_result 走，不丢
            ev.text = text;
            self.attach_scope(ev, true);
            return;
        }

        let Some(text) = text else { return };
        ev.role = Some(Role::User);
        let origin = obj.get("origin").and_then(|o| o.get("kind")).and_then(Value::as_str).unwrap_or("");
        let t = text.trim_start();

        if origin == "task-notification" || t.starts_with("<task-notification>") {
            // 后台任务完成通知：指回原调用。老日志没有 origin，靠前缀认出来的标为推断。
            ev.role = None;
            ev.links.push(Link {
                rel: "notifies",
                call_id: xml_tag(t, "tool-use-id"),
                task: xml_tag(t, "task-id"),
                inferred: origin != "task-notification",
                ..Default::default()
            });
            ev.text = Some(text);
            self.attach_scope(ev, true);
            return;
        }
        if origin == "coordinator" {
            // 父会话 SendMessage 送来的消息
            ev.links.push(Link {
                rel: "delivered_from",
                session: self.parent.clone(),
                inferred: self.parent.is_none(),
                ..Default::default()
            });
            return self.user_message(ev, text);
        }
        let is_meta = obj.get("isMeta").and_then(Value::as_bool).unwrap_or(false);
        if is_meta || INJECTED_PREFIXES.iter().any(|p| t.starts_with(p)) {
            ev.text = Some(text);
            return;
        }
        let queued = obj.get("promptSource").and_then(Value::as_str) == Some("queued");
        if queued && self.turn.is_some() {
            // 中途插话：留在当前 turn
            return self.user_message(ev, text);
        }

        let prompt_id = obj.get("promptId").and_then(Value::as_str).map(String::from);
        let inferred = prompt_id.is_none();
        let id = prompt_id.unwrap_or_else(|| obj.get("uuid").and_then(Value::as_str).unwrap_or("").to_string());
        self.begin_turn(id, inferred);
        ev.kind = Kind::TurnStarted;
        ev.text = Some(text);
        self.attach_scope(ev, false);
    }

    /// 当前 turn 里的一条用户消息（插话、父会话送来的消息）
    fn user_message(&self, ev: &mut Event, text: String) {
        ev.kind = Kind::Message;
        ev.text = Some(text);
        self.attach_scope(ev, true);
    }

    fn assistant_line(&mut self, ev: &mut Event, obj: &mut Map<String, Value>) {
        // message 缺失就整行留在 ext，requestId 也不能先拿走
        let Some(Value::Object(mut m)) = obj.remove("message") else { return };
        let request_id = take_str(obj, "requestId");
        m.remove("role");
        ev.model = take_str(&mut m, "model");
        ev.usage = take_usage(&mut m, "usage");
        ev.role = Some(Role::Assistant);
        ev.kind = Kind::Message;
        let parts = Parts::take(&mut m, Role::Assistant);
        ev.text = join_texts(parts.text);
        ev.reasoning = join_texts(parts.thinking);
        if let Some(t) = parts.tool {
            ev.kind = Kind::ToolCall;
            ev.tool = Some(t);
        }
        if !m.is_empty() {
            obj.insert("message".into(), Value::Object(m));
        }
        self.enter_step(request_id);
        self.attach_scope(ev, true);
    }
}

/// `<tag>value</tag>` 里的 value
fn xml_tag(s: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let start = s.find(&open)? + open.len();
    let end = s[start..].find(&format!("</{tag}>"))? + start;
    Some(s[start..end].trim().to_string())
}

/// message.content 拆出来的部分。user 行和 assistant 行的信封规则一样，只是各自认的 part 类型不同。
#[derive(Default)]
struct Parts {
    text: Vec<String>,
    thinking: Vec<String>,
    /// assistant 行是第一个 tool_use，user 行是第一个 tool_result（Claude Code 每个 block 单独一行，多出来的留 ext）
    tool: Option<Tool>,
}

impl Parts {
    /// 拿走 message.content：字符串直接是正文；数组按 part 拆，没吃掉的 part 留回 content；别的形状原样留下
    fn take(m: &mut Map<String, Value>, role: Role) -> Self {
        let parts = match m.remove("content") {
            Some(Value::String(s)) => return Self { text: vec![s], ..Default::default() },
            Some(Value::Array(parts)) => parts,
            Some(other) => {
                m.insert("content".into(), other);
                return Self::default();
            }
            None => return Self::default(),
        };
        let (mut out, mut rest) = (Self::default(), Vec::new());
        for mut p in parts {
            match (role, p.get("type").and_then(Value::as_str)) {
                // cache_control 等留在 ext
                (_, Some("text")) => take_text_part(p, "text", &mut out.text, &mut rest),
                // signature 是不透明的密文，留在 ext
                (Role::Assistant, Some("thinking")) => take_text_part(p, "thinking", &mut out.thinking, &mut rest),
                (Role::Assistant, Some("tool_use")) if out.tool.is_none() => {
                    let m = p.as_object_mut().unwrap();
                    let name = take_str(m, "name").unwrap_or_default();
                    out.tool = Some(Tool {
                        call_id: take_str(m, "id").unwrap_or_default(),
                        category: Some(tools::categorize(&name)),
                        name,
                        input: m.remove("input"),
                        ..Default::default()
                    });
                    keep_rest(p, &["type"], &mut rest);
                }
                (Role::User, Some("tool_result")) if out.tool.is_none() => {
                    let m = p.as_object_mut().unwrap();
                    let failed = m.get("is_error").and_then(Value::as_bool).unwrap_or(false);
                    out.tool = Some(Tool {
                        call_id: take_str(m, "tool_use_id").unwrap_or_default(),
                        output: m.remove("content"),
                        status: Some(if failed { "failed" } else { "completed" }.into()),
                        ..Default::default()
                    });
                    keep_rest(p, &["type", "is_error"], &mut rest);
                }
                _ => rest.push(p),
            }
        }
        if !rest.is_empty() {
            m.insert("content".into(), Value::Array(rest));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::event::{Event, Usage};

    const PATH: &str = "C:\\Users\\me\\.claude\\projects\\E--code-x\\sess-1.jsonl";
    const SUB_PATH: &str = "C:\\Users\\me\\.claude\\projects\\E--code-x\\sess-1\\subagents\\agent-abc.jsonl";

    fn run(lines: &[&str]) -> Vec<Event> {
        super::super::run(&PROVIDER, PATH, lines)
    }

    fn scope_id(s: &Option<Scope>) -> Option<&str> {
        s.as_ref().map(|s| s.id.as_str())
    }

    fn user(prompt_id: &str, text: &str) -> String {
        json!({"type":"user","uuid":format!("u-{prompt_id}"),"promptId":prompt_id,
               "message":{"role":"user","content":text}})
        .to_string()
    }

    fn assistant(request_id: &str, content: Value) -> String {
        json!({"type":"assistant","requestId":request_id,
               "message":{"role":"assistant","model":"m","content":content}})
        .to_string()
    }

    // 1. usage：alias 命中的键被吃掉，未映射的键留在 usage.ext，不会两头都有
    #[test]
    fn usage_keeps_unknown_fields_in_ext() {
        let line = json!({"type":"assistant","requestId":"r1","message":{"role":"assistant","content":[],
            "usage":{"input_tokens":10,"output_tokens":5,"cache_creation_input_tokens":3,"cache_read_input_tokens":2,
                     "cache_creation":{"ephemeral_5m_input_tokens":3,"ephemeral_1h_input_tokens":0},
                     "service_tier":"standard"}}})
        .to_string();
        let ev = run(&[&line]).remove(0);
        let u = ev.usage.expect("usage 应被解析");
        assert_eq!((u.input, u.output, u.cache_read, u.cache_write), (Some(10), Some(5), Some(2), Some(3)));
        assert_eq!(u.ext["cache_creation"]["ephemeral_5m_input_tokens"], 3);
        assert_eq!(u.ext["service_tier"], "standard");
        assert!(!u.ext.contains_key("input_tokens"), "alias 命中的键不能再出现在 ext：{:?}", u.ext);
        assert!(!u.ext.contains_key("cache_creation_input_tokens"));
        // 序列化：映射字段用 canonical 名，ext 平铺
        let s = serde_json::to_value(&u).unwrap();
        assert_eq!(
            s,
            json!({"input":10,"output":5,"cache_read":2,"cache_write":3,
            "cache_creation":{"ephemeral_5m_input_tokens":3,"ephemeral_1h_input_tokens":0},"service_tier":"standard"})
        );
        // ext 为空时不出现
        let s = serde_json::to_value(Usage { input: Some(1), ..Default::default() }).unwrap();
        assert_eq!(s, json!({"input":1}));
        assert!(ev.ext.get("message").and_then(|m| m.get("usage")).is_none());
    }

    // 1. usage 字段类型不对：解析失败，整个对象原样留回 ext
    #[test]
    fn usage_bad_shape_stays_in_ext() {
        let usage = json!({"input_tokens":"10","output_tokens":5});
        let line = json!({"type":"assistant","requestId":"r1",
            "message":{"role":"assistant","content":[],"usage":usage}})
        .to_string();
        let ev = run(&[&line]).remove(0);
        assert!(ev.usage.is_none());
        assert_eq!(ev.ext["message"]["usage"], usage);
    }

    // 2. tool_result 行夹带 text part：text 不丢
    #[test]
    fn tool_result_line_keeps_text() {
        let line = json!({"type":"user","promptId":"p1","message":{"role":"user","content":[
            {"type":"tool_result","tool_use_id":"c1","content":"ok"},
            {"type":"text","text":"补充说明"}]}})
        .to_string();
        let ev = run(&[&line]).remove(0);
        assert_eq!(ev.kind, Kind::ToolResult);
        assert_eq!(ev.tool.as_ref().unwrap().call_id, "c1");
        assert_eq!(ev.text.as_deref(), Some("补充说明"));
    }

    // 3. assistant content 是 String 就当 text；其他类型放回 ext
    #[test]
    fn assistant_content_string_or_other() {
        let evs = run(&[&assistant("r1", json!("纯文本")), &assistant("r1", json!(42))]);
        assert_eq!(evs[0].text.as_deref(), Some("纯文本"));
        assert!(evs[0].ext.get("message").and_then(|m| m.get("content")).is_none());
        assert!(evs[1].text.is_none());
        assert_eq!(evs[1].ext["message"]["content"], 42);
    }

    // 4. text part 的其余字段（cache_control 等）留在 ext，user / assistant 都是
    #[test]
    fn text_part_rest_kept() {
        let a = assistant("r1", json!([{"type":"text","text":"hi","cache_control":{"type":"ephemeral"}}]));
        let u = json!({"type":"user","promptId":"p1","message":{"role":"user","content":[
            {"type":"text","text":"hello","cache_control":{"type":"ephemeral"}}]}})
        .to_string();
        let evs = run(&[&a, &u]);
        assert_eq!(evs[0].text.as_deref(), Some("hi"));
        assert_eq!(evs[0].ext["message"]["content"], json!([{"cache_control":{"type":"ephemeral"}}]));
        assert_eq!(evs[1].text.as_deref(), Some("hello"));
        assert_eq!(evs[1].ext["message"]["content"], json!([{"cache_control":{"type":"ephemeral"}}]));
        // 没有多余字段的 text part 不留空对象
        let evs = run(&[&assistant("r1", json!([{"type":"text","text":"hi"}]))]);
        assert!(evs[0].ext.get("message").and_then(|m| m.get("content")).is_none());
    }

    // 5. message 缺失时 requestId 留在 ext
    #[test]
    fn request_id_stays_when_message_missing() {
        let line = json!({"type":"assistant","requestId":"r1","uuid":"x"}).to_string();
        let ev = run(&[&line]).remove(0);
        assert_eq!(ev.kind, Kind::Meta);
        assert_eq!(ev.ext["requestId"], "r1");
        assert!(ev.step.is_none());
    }

    // 6. turn_duration 带上当前 turn，之后 attachment / 插话都不再挂到它
    #[test]
    fn turn_completed_clears_scope() {
        let evs = run(&[
            &user("p1", "第一问"),
            &assistant("r1", json!([{"type":"text","text":"答"}])),
            &json!({"type":"system","subtype":"turn_duration","durationMs":1}).to_string(),
            &json!({"type":"attachment","attachment":{"type":"environment"}}).to_string(),
            &json!({"type":"user","promptSource":"queued","uuid":"u-q","message":{"role":"user","content":"插话"}})
                .to_string(),
        ]);
        assert_eq!(evs[2].kind, Kind::TurnCompleted);
        assert_eq!(scope_id(&evs[2].turn), Some("p1"));
        assert_eq!(evs[3].kind, Kind::RequestHeader);
        assert!(evs[3].turn.is_none(), "turn 结束后的 attachment 不该挂到它：{:?}", evs[3].turn);
        assert!(evs[3].step.is_none());
        // 没有进行中的 turn，queued 也只能开新 turn；序号继续数
        assert_eq!(evs[4].kind, Kind::TurnStarted);
        assert_eq!(evs[4].turn, Some(Scope { id: "u-q".into(), n: Some(2) }));
        assert!(evs[4].inferred.contains(&"turn"));
    }

    // 7. 解析失败的行只有 _unparsed，不凭空加 type
    #[test]
    fn unparsed_line_has_no_type() {
        let evs = run(&["{not json", "", r#"{"type":"user","message":{"role":"user","content":"ok"}}"#]);
        assert_eq!(evs[0].ext["_unparsed"], "{not json");
        assert!(!evs[0].ext.contains_key("type"), "{:?}", evs[0].ext);
        assert!(!evs[1].ext.contains_key("type"));
        assert_eq!(evs[2].ext["type"], "user");
    }

    // promptId 分 turn，requestId 分 step；同 requestId 的多行同一步
    #[test]
    fn turn_by_prompt_id_step_by_request_id() {
        let evs = run(&[
            &user("p1", "问"),
            &assistant("r1", json!([{"type":"thinking","thinking":"想","signature":"sig"}])),
            &assistant("r1", json!([{"type":"tool_use","id":"c1","name":"Read","input":{"f":1}}])),
            &json!({"type":"user","promptId":"p1","message":{"role":"user","content":[
                {"type":"tool_result","tool_use_id":"c1","content":"内容","is_error":true}]}})
            .to_string(),
            &assistant("r2", json!([{"type":"text","text":"完"}])),
        ]);
        assert_eq!(evs[0].kind, Kind::TurnStarted);
        assert_eq!(evs[0].turn, Some(Scope { id: "p1".into(), n: Some(1) }));
        assert!(evs[0].inferred.is_empty());
        for ev in &evs[1..] {
            assert_eq!(scope_id(&ev.turn), Some("p1"));
            assert!(ev.inferred.contains(&"step") && !ev.inferred.contains(&"turn"));
        }
        assert_eq!(evs[1].reasoning.as_deref(), Some("想"));
        assert_eq!(evs[1].ext["message"]["content"], json!([{"signature":"sig"}]));
        assert_eq!(evs[1].step, Some(Scope { id: "r1".into(), n: Some(1) }));
        assert_eq!(evs[2].kind, Kind::ToolCall);
        assert_eq!(evs[2].step, evs[1].step);
        let t = evs[3].tool.as_ref().unwrap();
        assert_eq!((t.call_id.as_str(), t.status.as_deref()), ("c1", Some("failed")));
        assert_eq!(evs[3].step, evs[1].step);
        assert_eq!(evs[4].step, Some(Scope { id: "r2".into(), n: Some(2) }));
        assert_eq!(evs[4].model.as_deref(), Some("m"));
    }

    // 老日志没有 promptId：用 user 行 uuid，标为推断
    #[test]
    fn turn_inferred_from_uuid_without_prompt_id() {
        let line = json!({"type":"user","uuid":"u1","message":{"role":"user","content":"问"}}).to_string();
        let ev = run(&[&line]).remove(0);
        assert_eq!(ev.turn, Some(Scope { id: "u1".into(), n: Some(1) }));
        assert_eq!(ev.inferred, vec!["turn"]);
    }

    // task-notification：不是用户在说话，links 指回原调用
    #[test]
    fn task_notification_links_to_call() {
        let text = "<task-notification><task-id>t1</task-id><tool-use-id>c9</tool-use-id></task-notification>";
        let explicit =
            json!({"type":"user","origin":{"kind":"task-notification"},"message":{"role":"user","content":text}});
        let by_prefix = json!({"type":"user","message":{"role":"user","content":text}});
        let evs = run(&[&user("p1", "问"), &explicit.to_string(), &by_prefix.to_string()]);
        for ev in &evs[1..] {
            assert_eq!(ev.kind, Kind::Meta);
            assert!(ev.role.is_none());
            assert_eq!(scope_id(&ev.turn), Some("p1"));
            let l = &ev.links[0];
            assert_eq!((l.rel, l.call_id.as_deref(), l.task.as_deref()), ("notifies", Some("c9"), Some("t1")));
        }
        assert!(!evs[1].links[0].inferred);
        assert!(evs[2].links[0].inferred);
    }

    // coordinator：父会话送来的消息，留在当前 turn
    #[test]
    fn coordinator_message_links_to_parent() {
        let line = json!({"type":"user","origin":{"kind":"coordinator"},"message":{"role":"user","content":"继续"}})
            .to_string();
        let evs = super::super::run(&PROVIDER, SUB_PATH, &[&user("p1", "任务"), &line]);
        assert_eq!(evs[1].kind, Kind::Message);
        assert_eq!(evs[1].role, Some(Role::User));
        assert_eq!(scope_id(&evs[1].turn), Some("p1"));
        let l = &evs[1].links[0];
        assert_eq!((l.rel, l.session.as_deref(), l.inferred), ("delivered_from", Some("sess-1"), false));
    }

    // queued：turn 进行中的插话不开新 turn
    #[test]
    fn queued_prompt_stays_in_turn() {
        let line =
            json!({"type":"user","promptSource":"queued","uuid":"u2","message":{"role":"user","content":"顺便"}})
                .to_string();
        let evs = run(&[&user("p1", "问"), &assistant("r1", json!([])), &line]);
        assert_eq!(evs[2].kind, Kind::Message);
        assert_eq!(evs[2].turn, Some(Scope { id: "p1".into(), n: Some(1) }));
        assert_eq!(scope_id(&evs[2].step), Some("r1"));
    }

    // 注入前缀 / isMeta：Meta，不开 turn、不挂 turn
    #[test]
    fn injected_prefixes_are_meta() {
        let cmd = json!({"type":"user","uuid":"u0","message":{"role":"user","content":"<command-name>/clear</command-name>"}});
        let meta = json!({"type":"user","uuid":"u0","isMeta":true,"message":{"role":"user","content":"x"}});
        let evs = run(&[&cmd.to_string(), &meta.to_string(), &user("p1", "真问题")]);
        for ev in &evs[..2] {
            assert_eq!(ev.kind, Kind::Meta);
            assert_eq!(ev.role, Some(Role::User));
            assert!(ev.text.is_some());
            assert!(ev.turn.is_none());
        }
        assert_eq!(evs[2].turn, Some(Scope { id: "p1".into(), n: Some(1) }));
    }

    // Agent 调用的 tool_result → spawned 指向子会话
    #[test]
    fn agent_result_links_spawned() {
        let line =
            json!({"type":"user","promptId":"p1","toolUseResult":{"agentId":"abc"},"message":{"role":"user","content":[
            {"type":"tool_result","tool_use_id":"c1","content":"done"}]}})
            .to_string();
        let ev = run(&[&line]).remove(0);
        let l = &ev.links[0];
        assert_eq!((l.rel, l.session.as_deref(), l.call_id.as_deref()), ("spawned", Some("agent-abc"), Some("c1")));
        assert_eq!(ev.ext["toolUseResult"]["agentId"], "abc");
    }

    // subagent 文件：session 是 agent-<id>，parent 是上级目录名
    #[test]
    fn subagent_path_sets_parent() {
        let ev = super::super::run(&PROVIDER, SUB_PATH, &[&user("p1", "x")]).remove(0);
        assert_eq!(ev.session, "agent-abc");
        assert_eq!(ev.parent_session.as_deref(), Some("sess-1"));
        let ev = run(&[&user("p1", "x")]).remove(0);
        assert_eq!(ev.session, "sess-1");
        assert!(ev.parent_session.is_none());
    }
}
