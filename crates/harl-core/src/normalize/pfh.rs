//! preferred-harness 协议日志 v1 → Event。零推断：turn / step 边界都是日志里显式写的。
//!
//! 文件：`<root>/.pfh/agent/protocol-events/<session>.jsonl`，
//! subagent：`<session>.sub-call_<toolCallId>.jsonl`。
//! installed 版的 root 是 ~/.pfh，开发版是各 workspace 自己的 .pfh；后者从 sessions-meta.json 里的 workspace 找。

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::{
    LineNormalizer, Provider, file_stem, join_texts, take_str, take_text_part, take_ts, take_u64, take_usage, tools,
};
use crate::event::{Event, Kind, Prov, Role, Scope, Tool};

pub static PROVIDER: Provider = Provider {
    name: "pfh",
    owns: |p| p.contains("/.pfh/agent/protocol-events/"),
    default_roots,
    new: |path| Box::new(Normalizer::new(path)),
    superseded_by: |_| None,
};

/// 从 ~/.pfh 出发，顺着每个 agent 目录的 sessions-meta.json 里记的 workspace 找下去，直到没有新的
fn default_roots(home: &Path) -> Vec<PathBuf> {
    let mut agents = BTreeSet::from([home.join(".pfh").join("agent")]);
    let mut queue: Vec<PathBuf> = agents.iter().cloned().collect();
    while let Some(agent) = queue.pop() {
        let Ok(meta) = fs::read_to_string(agent.join("sessions-meta.json")) else { continue };
        let Ok(Value::Object(m)) = serde_json::from_str::<Value>(&meta) else { continue };
        for ws in m.values().filter_map(|v| v.get("workspace").and_then(Value::as_str)) {
            let dir = PathBuf::from(ws).join(".pfh").join("agent");
            if dir.is_dir() && agents.insert(dir.clone()) {
                queue.push(dir);
            }
        }
    }
    agents.into_iter().map(|a| a.join("protocol-events")).collect()
}

pub struct Normalizer {
    session: String,
    parent: Option<String>,
}

impl Normalizer {
    pub fn new(path: &str) -> Self {
        let stem = file_stem(path);
        Self { session: stem.to_string(), parent: stem.split_once(".sub-call_").map(|(p, _)| p.to_string()) }
    }
}

impl LineNormalizer for Normalizer {
    fn identity(&self) -> (String, Option<String>) {
        (self.session.clone(), self.parent.clone())
    }

    fn line(&mut self, seq: u32, mut obj: Map<String, Value>, prov: Prov) -> Event {
        // type 不映射到任何字段，留在 ext 里；这里只读
        let ty = obj.get("type").and_then(Value::as_str).unwrap_or("").to_string();
        // 这几个每行都有、只是信封，留在 ext 里没意义
        for k in ["version", "sessionId", "runId"] {
            obj.remove(k);
        }
        let mut ev = Event {
            provider: PROVIDER.name,
            session: self.session.clone(),
            parent_session: self.parent.clone(),
            seq,
            ts: take_ts(&mut obj, "timestamp"),
            turn: take_str(&mut obj, "turnId").map(|id| Scope { id, n: take_u64(&mut obj, "turn").map(|n| n as u32) }),
            step: take_str(&mut obj, "stepId").map(|id| Scope { id, n: take_u64(&mut obj, "step").map(|n| n as u32) }),
            prov,
            ..Default::default()
        };

        let kind = ty.as_str();
        match kind {
            "turn_started" => {
                ev.kind = Kind::TurnStarted;
                ev.role = Some(Role::User);
                if let Some(Value::Object(mut m)) = obj.remove("message") {
                    ev.text = take_str(&mut m, "content");
                    obj.insert("message".into(), Value::Object(m));
                }
            }
            "turn_completed" | "step_completed" => {
                ev.kind = if kind == "turn_completed" { Kind::TurnCompleted } else { Kind::StepCompleted };
                ev.model = take_str(&mut obj, "modelId");
                ev.usage = take_usage(&mut obj, "usage");
            }
            "step_started" => ev.kind = Kind::StepStarted,
            "request_header" => ev.kind = Kind::RequestHeader,
            "message_committed" => {
                if let Some(Value::Object(mut m)) = obj.remove("message") {
                    match m.get("role").and_then(Value::as_str) {
                        Some("user") => {
                            ev.kind = Kind::Message;
                            ev.role = Some(Role::User);
                            ev.text = take_str(&mut m, "content");
                        }
                        Some("assistant") => {
                            ev.kind = Kind::Message;
                            ev.role = Some(Role::Assistant);
                            ev.reasoning = take_str(&mut m, "reasoning");
                            if let Some(Value::Array(parts)) = m.remove("content") {
                                let (text, reasoning, rest) = split_parts(parts);
                                ev.text = text;
                                ev.reasoning = ev.reasoning.take().or(reasoning);
                                if !rest.is_empty() {
                                    m.insert("content".into(), Value::Array(rest));
                                }
                            }
                        }
                        // tool 消息的内容 tool_call_finished 已经带了；这里只当元数据
                        _ => {}
                    }
                    obj.insert("message".into(), Value::Object(m));
                }
            }
            "tool_call_started" | "tool_call_finished" => {
                let finished = kind == "tool_call_finished";
                ev.kind = if finished { Kind::ToolResult } else { Kind::ToolCall };
                ev.role = Some(if finished { Role::Tool } else { Role::Assistant });
                let name = take_str(&mut obj, "toolName").unwrap_or_default();
                ev.tool = Some(Tool {
                    call_id: take_str(&mut obj, "toolCallId").unwrap_or_default(),
                    category: Some(tools::categorize(&name)),
                    name,
                    input: obj.remove("input"),
                    output: obj.remove("output"),
                    status: take_str(&mut obj, "status"),
                });
            }
            _ => {}
        }
        ev.ext = obj;
        ev
    }
}

/// assistant content parts：text 拼成正文，reasoning 拼成推理，其余（tool-call 等）原样留下；
/// text / reasoning part 上多出来的字段也留下
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const PATH: &str = "/home/me/.pfh/agent/protocol-events/sess-1.jsonl";
    const SUB_PATH: &str = "/home/me/.pfh/agent/protocol-events/sess-1.sub-call_abc.jsonl";

    fn run(lines: &[&str]) -> Vec<Event> {
        super::super::run(&PROVIDER, PATH, lines)
    }

    // 1. usage：alias 命中的键被吃掉，未映射的键留在 usage.ext；信封字段不进 ext
    #[test]
    fn usage_keeps_unknown_fields_in_ext() {
        let line = json!({"type":"step_completed","version":1,"sessionId":"s","runId":"r","turnId":"t1","turn":1,
            "stepId":"s1","step":2,"modelId":"m","usage":{"inputTokens":10,"outputTokens":5,"cacheReadTokens":2,
            "cacheWriteTokens":3,"reasoningTokens":7}})
        .to_string();
        let ev = run(&[&line]).remove(0);
        assert_eq!(ev.kind, Kind::StepCompleted);
        assert_eq!(ev.turn, Some(Scope { id: "t1".into(), n: Some(1) }));
        assert_eq!(ev.step, Some(Scope { id: "s1".into(), n: Some(2) }));
        assert_eq!(ev.model.as_deref(), Some("m"));
        let u = ev.usage.expect("usage 应被解析");
        assert_eq!((u.input, u.output, u.cache_read, u.cache_write), (Some(10), Some(5), Some(2), Some(3)));
        assert_eq!(u.ext, json!({"reasoningTokens":7}).as_object().unwrap().clone());
        assert_eq!(ev.ext, json!({"type":"step_completed"}).as_object().unwrap().clone());
    }

    // 1. usage 字段类型不对：解析失败，整个对象原样留回 ext
    #[test]
    fn usage_bad_shape_stays_in_ext() {
        let usage = json!({"inputTokens":"10"});
        let line = json!({"type":"turn_completed","usage":usage}).to_string();
        let ev = run(&[&line]).remove(0);
        assert_eq!(ev.kind, Kind::TurnCompleted);
        assert!(ev.usage.is_none());
        assert_eq!(ev.ext["usage"], usage);
    }

    // 4. text / reasoning part 的其余字段留在 ext，其他 part 原样留下
    #[test]
    fn part_rest_kept() {
        let line = json!({"type":"message_committed","message":{"role":"assistant","content":[
            {"type":"reasoning","text":"想","signature":"sig"},
            {"type":"text","text":"答","annotations":[]},
            {"type":"tool-call","toolCallId":"c1"}]}})
        .to_string();
        let ev = run(&[&line]).remove(0);
        assert_eq!(ev.kind, Kind::Message);
        assert_eq!(ev.role, Some(Role::Assistant));
        assert_eq!(ev.text.as_deref(), Some("答"));
        assert_eq!(ev.reasoning.as_deref(), Some("想"));
        assert_eq!(
            ev.ext["message"],
            json!({"role":"assistant","content":[{"signature":"sig"},{"annotations":[]},{"type":"tool-call","toolCallId":"c1"}]})
        );
    }

    // 7. 解析失败的行只有 _unparsed，不凭空加 type
    #[test]
    fn unparsed_line_has_no_type() {
        let evs = run(&["{not json", r#"{"type":"step_started"}"#]);
        assert_eq!(evs[0].kind, Kind::Meta);
        assert_eq!(evs[0].ext["_unparsed"], "{not json");
        assert!(!evs[0].ext.contains_key("type"), "{:?}", evs[0].ext);
        assert_eq!(evs[1].kind, Kind::StepStarted);
        assert_eq!(evs[1].ext["type"], "step_started");
    }

    // turn_started / user message 的正文；tool 调用两端
    #[test]
    fn turn_and_tool_calls() {
        let evs = run(&[
            &json!({"type":"turn_started","turnId":"t1","turn":1,"message":{"role":"user","content":"问"}}).to_string(),
            &json!({"type":"tool_call_started","toolCallId":"c1","toolName":"read","input":{"f":1}}).to_string(),
            &json!({"type":"tool_call_finished","toolCallId":"c1","toolName":"read","output":"x","status":"ok"})
                .to_string(),
            &json!({"type":"message_committed","message":{"role":"user","content":"再问"}}).to_string(),
        ]);
        assert_eq!(evs[0].kind, Kind::TurnStarted);
        assert_eq!(evs[0].text.as_deref(), Some("问"));
        assert_eq!(evs[0].ext["message"], json!({"role":"user"}));
        assert_eq!(evs[1].kind, Kind::ToolCall);
        assert_eq!(evs[1].role, Some(Role::Assistant));
        let t = evs[1].tool.as_ref().unwrap();
        assert_eq!((t.call_id.as_str(), t.name.as_str()), ("c1", "read"));
        assert_eq!(t.input, Some(json!({"f":1})));
        assert_eq!(evs[2].kind, Kind::ToolResult);
        assert_eq!(evs[2].role, Some(Role::Tool));
        let t = evs[2].tool.as_ref().unwrap();
        assert_eq!((t.output.as_ref(), t.status.as_deref()), (Some(&json!("x")), Some("ok")));
        assert_eq!(evs[3].kind, Kind::Message);
        assert_eq!(evs[3].text.as_deref(), Some("再问"));
    }

    // subagent 文件：`<session>.sub-call_<id>` 的 parent 是 session
    #[test]
    fn subagent_path_sets_parent() {
        let line = json!({"type":"step_started"}).to_string();
        let ev = super::super::run(&PROVIDER, SUB_PATH, &[&line]).remove(0);
        assert_eq!(ev.session, "sess-1.sub-call_abc");
        assert_eq!(ev.parent_session.as_deref(), Some("sess-1"));
        assert!(run(&[&line]).remove(0).parent_session.is_none());
    }
}
