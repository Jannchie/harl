//! 上下文的组成：重放事件流，估算每次请求时 prompt 里各类内容各占多少，再按 provider 报的 prompt 校准。
//!
//! 口径和会话页上下文视图（web/src/lib/context.ts 的 foldContext）一致，只是只留各类合计、不留节点：
//! - 七类：system、工具 schema、用户消息、注入的上下文、skill、助手回复、工具结果；other 是 provider 报的 prompt 里日志没记到的部分。
//! - 一次请求在它的第一条回复之前拍快照；压缩时 dsh 按 `shadowedSeqs` / `surfaceOp` 逐条移走，其他整面换掉。
//! - token 估算：ASCII 4 字符 1 个、其余 1.5 字符 1 个，每块 +4；JSON 按紧凑序列化的字符数估，不真的序列化。
//! - 校准：system 和工具 schema 都记在日志里（complete）就整体缩放到实际 prompt，否则差额单列 other。
//!
//! 和 web 端的已知偏差（都在校准时被吸收：complete 时按比例摊进各类，否则落进 other）：
//! - 图片不估（web 按尺寸估，这里拿不到尺寸以外的上下文，干脆不算）；
//! - 快照拍在 stats 认定的这一步第一条模型事件之前（assistant 角色或 step_completed），web 在第一条回复 / 工具调用之前，
//!   pfh 这类工具调用不带 assistant 角色的，会晚一条；
//! - JSON 的字符数按 serde_json 紧凑输出估，web 用 JSON.stringify，转义上可能差几个字符。
//!
//! 这里同时认出 skill 的加载（`SkillLoad`），口径同 dsh-context：skill 工具的结果、dsh 的 skill-invocation 注入，
//! 外加 Claude Code 的「Base directory for this skill:」（斜杠命令加载的 skill 只有这一条）和 Codex 的 `<skill>` 注入。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::event::{Event, Kind, Role, Signal};
use crate::normalize::xml_tag;

/// 各类 token 数
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mix {
    pub system: u64,
    pub tools: u64,
    pub user: u64,
    pub inject: u64,
    pub skill: u64,
    pub assistant: u64,
    pub tool: u64,
    /// provider 报的 prompt 里日志没记到的部分（system / 工具 schema / 框架开销）
    pub other: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cat {
    System,
    Tools,
    User,
    Inject,
    Skill,
    Assistant,
    Tool,
}

const CATS: [Cat; 7] = [Cat::System, Cat::Tools, Cat::User, Cat::Inject, Cat::Skill, Cat::Assistant, Cat::Tool];

impl Mix {
    pub fn get(&self, c: Cat) -> u64 {
        match c {
            Cat::System => self.system,
            Cat::Tools => self.tools,
            Cat::User => self.user,
            Cat::Inject => self.inject,
            Cat::Skill => self.skill,
            Cat::Assistant => self.assistant,
            Cat::Tool => self.tool,
        }
    }

    fn slot(&mut self, c: Cat) -> &mut u64 {
        match c {
            Cat::System => &mut self.system,
            Cat::Tools => &mut self.tools,
            Cat::User => &mut self.user,
            Cat::Inject => &mut self.inject,
            Cat::Skill => &mut self.skill,
            Cat::Assistant => &mut self.assistant,
            Cat::Tool => &mut self.tool,
        }
    }

    pub fn sum(&self) -> u64 {
        CATS.iter().map(|&c| self.get(c)).sum::<u64>() + self.other
    }

    pub fn add(&mut self, o: &Mix) {
        for c in CATS {
            *self.slot(c) += o.get(c);
        }
        self.other += o.other;
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// 等比缩放到合计为 target（模型过滤时按所选模型的 prompt 占比取一部分）；取整的零头给最大的一类
    pub fn scaled_to(&self, target: u64) -> Mix {
        let sum = self.sum();
        if sum == 0 || target == sum {
            return *self;
        }
        let k = target as f64 / sum as f64;
        let mut out = Mix::default();
        for c in CATS {
            *out.slot(c) = (self.get(c) as f64 * k).round() as u64;
        }
        out.other = (self.other as f64 * k).round() as u64;
        out.settle(target);
        out
    }

    /// 合计补齐到 target：差额记在最大的一类上（并列取靠前的）
    fn settle(&mut self, target: u64) {
        let diff = target as i64 - self.sum() as i64;
        let top = CATS.iter().copied().fold(Cat::System, |a, c| if self.get(c) > self.get(a) { c } else { a });
        let v = self.slot(top);
        *v = (*v as i64 + diff).max(0) as u64;
    }
}

/// 按 provider 报的 prompt 校准一份估算。complete：system 和工具都在日志里，整体缩放；否则差额单列 other
pub fn anchor(parts: &Mix, prompt: u64, complete: bool) -> Mix {
    let est = parts.sum() - parts.other;
    if prompt == 0 || est == 0 {
        return *parts;
    }
    if complete || prompt < est {
        let k = prompt as f64 / est as f64;
        let mut out = Mix::default();
        for c in CATS {
            *out.slot(c) = (parts.get(c) as f64 * k).round() as u64;
        }
        out.settle(prompt);
        return out;
    }
    Mix { other: prompt - est, ..*parts }
}

/// 字符计数：估算只看 ASCII 和非 ASCII（按 UTF-16 码元，和 web 端 JS 的 length 一致）各多少
#[derive(Default, Clone, Copy)]
struct Chars {
    ascii: u64,
    wide: u64,
    /// 有没有非空白字符
    solid: bool,
}

impl Chars {
    fn text(&mut self, s: &str) {
        self.scan(s, false);
    }

    /// 按字节数一遍：UTF-8 的首字节就是一个字符，4 字节的在 UTF-16 里占两个码元（和 JS 的 length 一致）。
    /// escapes：顺带数 JSON 里要多一个反斜杠的字符（控制字符的 \uXXXX 少见，不细算）
    fn scan(&mut self, s: &str, escapes: bool) {
        let (mut ascii, mut wide, mut esc) = (0u64, 0u64, 0u64);
        let mut solid = self.solid;
        for &b in s.as_bytes() {
            if b < 0x80 {
                ascii += 1;
                solid |= !b.is_ascii_whitespace();
                esc += (escapes && matches!(b, b'"' | b'\\' | b'\n' | b'\r' | b'\t')) as u64;
            } else if b >= 0xC0 {
                wide += if b >= 0xF0 { 2 } else { 1 };
                solid = true;
            }
        }
        self.ascii += ascii + esc;
        self.wide += wide;
        self.solid = solid;
    }

    /// 紧凑 JSON 序列化后的字符数（不真的序列化）
    fn json(&mut self, v: &Value) {
        match v {
            Value::Null => self.ascii += 4,
            Value::Bool(b) => self.ascii += if *b { 4 } else { 5 },
            Value::Number(n) => self.ascii += digits(n),
            Value::String(s) => self.json_str(s),
            Value::Array(a) => {
                self.ascii += 2 + a.len().saturating_sub(1) as u64;
                for x in a {
                    self.json(x);
                }
            }
            Value::Object(m) => self.json_obj(m),
        }
        self.solid = true;
    }

    fn json_obj(&mut self, m: &Map<String, Value>) {
        self.ascii += 2 + m.len().saturating_sub(1) as u64 + m.len() as u64;
        for (k, x) in m {
            self.json_str(k);
            self.json(x);
        }
    }

    fn json_str(&mut self, s: &str) {
        self.ascii += 2;
        self.scan(s, true);
    }

    /// `json(v)`：字符串原样，其余紧凑 JSON
    fn raw(&mut self, v: &Value) {
        match v {
            Value::String(s) => self.text(s),
            v => self.json(v),
        }
    }

    fn est(&self) -> u64 {
        (self.ascii as f64 / 4.0 + self.wide as f64 / 1.5).ceil() as u64
    }

    /// 一块内容：正文 + 4 的块开销；空的不算
    fn block(&self) -> u64 {
        if self.ascii + self.wide == 0 { 0 } else { self.est() + 4 }
    }
}

/// 数字序列化后的长度；整数不分配
fn digits(n: &serde_json::Number) -> u64 {
    let len = |mut v: u64| {
        let mut d = 1;
        while v >= 10 {
            v /= 10;
            d += 1;
        }
        d
    };
    match (n.as_u64(), n.as_i64()) {
        (Some(v), _) => len(v),
        (None, Some(v)) => 1 + len(v.unsigned_abs()),
        _ => n.to_string().len() as u64,
    }
}

fn chars_of(s: &str) -> Chars {
    let mut c = Chars::default();
    c.text(s);
    c
}

fn block(s: Option<&str>) -> u64 {
    s.map_or(0, |s| chars_of(s).block())
}

fn obj(v: Option<&Value>) -> Option<&Map<String, Value>> {
    v.and_then(Value::as_object)
}

fn str_at<'a>(m: Option<&'a Map<String, Value>>, key: &str) -> Option<&'a str> {
    m?.get(key)?.as_str().filter(|s| !s.is_empty())
}

/// 一次 skill 加载。count = false 只是补上之前那次的路径（Claude Code 的 Skill 工具结果后面跟着的那条）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillLoad {
    pub name: String,
    pub path: Option<String>,
    pub count: bool,
}

const SKILL_TOOLS: &[&str] = &["skill", "Skill"];
const CLAUDE_SKILL_DIR: &str = "Base directory for this skill:";

/// 不进上下文的 Claude Code attachment（只是给 UI / 记账的）
const CLAUDE_SILENT: &[&str] = &[
    "prompt_snapshot",
    "deferred_tools_record",
    "queued_command",
    "command_permissions",
    "credential_org",
    "auto_mode",
    "edited_text_file",
    "hook_success",
    "hook_non_blocking_error",
    "async_hook_response",
];

/// `<skill_content name="X"` 里的 X
fn skill_content_name(text: &str) -> Option<String> {
    let i = text.find("<skill_content")?;
    let rest = &text[i + "<skill_content".len()..];
    let rest = rest.strip_prefix(|c: char| c.is_whitespace())?.trim_start();
    let rest = rest.strip_prefix("name=\"")?;
    let end = rest.find('"')?;
    (end > 0).then(|| rest[..end].to_string())
}

#[derive(Default, Clone)]
pub struct Composer {
    /// 当前的 system + 工具 + 消息面
    sums: Mix,
    saw_system: bool,
    saw_tools: bool,
    /// dsh 的原始行号 → 还在面上的节点（压缩按行号移走）
    by_raw: HashMap<u64, (Cat, u64)>,
    /// 压缩事件在等它后面那条替换上来的摘要；这期间 dsh 的 surfaceOp replace 不再按区间移
    compaction_pending: bool,
    /// 这一轮已经从 turn_started 记下的提问：(turn id, 文本)
    turn_prompt: Option<(Option<String>, String)>,
    /// call_id → (工具名, 参数里的 skill 名)
    calls: HashMap<String, (String, Option<String>)>,
    /// Claude Code：刚从 Skill 工具加载、还在等路径的 skill
    awaiting_path: Option<String>,
}

impl Composer {
    /// 当前上下文的估算组成（还没校准）
    pub fn parts(&self) -> Mix {
        self.sums
    }

    /// system 和工具 schema 都记在日志里：校准时整体缩放
    pub fn complete(&self) -> bool {
        self.saw_system && self.saw_tools
    }

    fn add(&mut self, cat: Cat, est: u64, raw: Option<u64>) {
        *self.sums.slot(cat) += est;
        if let Some(r) = raw {
            self.by_raw.insert(r, (cat, est));
        }
        if cat != Cat::Assistant {
            self.compaction_pending = false;
        }
    }

    fn remove_raw(&mut self, r: u64) {
        if let Some((cat, est)) = self.by_raw.remove(&r) {
            let v = self.sums.slot(cat);
            *v = v.saturating_sub(est);
        }
    }

    /// 整面换掉：消息面清零，system 和工具留着
    fn clear_surface(&mut self) {
        self.sums = Mix { system: self.sums.system, tools: self.sums.tools, ..Default::default() };
        self.by_raw.clear();
    }

    fn set_system(&mut self, c: Chars) {
        self.saw_system = true;
        if c.solid {
            self.sums.system = c.est() + 4;
        }
    }

    fn set_tools(&mut self, list: &[Value]) {
        // 只有名字的列表（pfh 的 toolNames）没有 schema，不算「工具记全了」
        if list.iter().any(Value::is_object) {
            self.saw_tools = true;
        }
        self.sums.tools = list
            .iter()
            .map(|t| {
                let mut c = Chars::default();
                c.raw(t);
                c.est() + 4
            })
            .sum();
    }

    fn inject(&mut self, c: Chars, cat: Cat, raw: Option<u64>) {
        if c.solid {
            self.add(cat, c.block() + 4, raw);
        }
    }

    fn add_user(&mut self, text: &str, raw: Option<u64>) {
        if !text.is_empty() {
            self.add(Cat::User, block(Some(text)) + 4, raw);
        }
    }

    /// 放进一个事件；返回这个事件带来的 skill 加载
    pub fn push(&mut self, ev: &Event) -> Option<SkillLoad> {
        let ext = &ev.ext;
        let ty = ext.get("type").and_then(Value::as_str).unwrap_or("");
        let data = obj(ext.get("data"));
        let raw = if ev.provider == "dsh" { ext.get("seq").and_then(Value::as_u64) } else { None };
        let mut load = None;

        // dsh 的 surfaceOp replace：没有压缩事件预告时按区间换掉
        if let Some(op) = obj(ext.get("surfaceOp"))
            && op.get("op").and_then(Value::as_str) == Some("replace")
            && !self.compaction_pending
            && let (Some(a), Some(b)) = (op.get("startSeq").and_then(Value::as_u64), op.get("endSeq").and_then(Value::as_u64))
        {
            let hit: Vec<u64> = self.by_raw.keys().copied().filter(|k| (a..=b).contains(k)).collect();
            for k in hit {
                self.remove_raw(k);
            }
        }

        match ev.kind {
            Kind::TurnStarted => {
                if let Some(t) = ev.text.as_deref().filter(|t| !t.is_empty()) {
                    self.add_user(t, raw);
                    self.turn_prompt = Some((ev.turn.as_ref().map(|s| s.id.clone()), t.to_string()));
                }
            }
            Kind::ToolCall => {
                if let Some(tool) = &ev.tool {
                    let mut args = Chars::default();
                    if let Some(i) = &tool.input {
                        args.raw(i);
                    }
                    self.add(Cat::Assistant, chars_of(&tool.name).est() + args.block(), raw);
                    // dsh 的 skill 工具是 {name}，Claude Code 的 Skill 是 {skill}
                    let input = obj(tool.input.as_ref());
                    let skill = str_at(input, "name").or_else(|| str_at(input, "skill")).map(String::from);
                    self.calls.insert(tool.call_id.clone(), (tool.name.clone(), skill));
                }
            }
            Kind::ToolResult => {
                if let Some(tool) = &ev.tool {
                    let call = self.calls.remove(&tool.call_id);
                    let name = if tool.name.is_empty() { call.as_ref().map_or("", |c| c.0.as_str()) } else { tool.name.as_str() };
                    let (c, found) = output_chars(tool.output.as_ref());
                    let skill_tool = SKILL_TOOLS.contains(&name);
                    let m = if skill_tool || call.is_none() { found } else { None };
                    let skill = (skill_tool || m.is_some())
                        .then(|| m.or_else(|| call.as_ref().and_then(|c| c.1.clone())).unwrap_or_else(|| name.to_string()));
                    self.add(if skill.is_some() { Cat::Skill } else { Cat::Tool }, c.block() + 4, raw);
                    let failed = matches!(tool.status.as_deref(), Some("failed" | "error"));
                    if let Some(s) = skill
                        && !failed
                    {
                        if ev.provider == "claude-code" {
                            self.awaiting_path = Some(s.clone());
                        }
                        load = Some(SkillLoad { name: s, path: None, count: true });
                    }
                }
            }
            Kind::Message if ev.role == Some(Role::User) => {
                let text = ev.text.as_deref().unwrap_or("");
                if ext.get("isCompactSummary").and_then(Value::as_bool).unwrap_or(false) {
                    self.inject(chars_of(text), Cat::Inject, raw);
                } else if self.turn_prompt.as_ref().is_some_and(|(t, p)| *t == ev.turn.as_ref().map(|s| s.id.clone()) && p == text) {
                    self.turn_prompt = None;
                } else {
                    self.add_user(text, raw);
                }
            }
            Kind::Message if ev.role == Some(Role::Assistant) => {
                self.awaiting_path = None;
                if ev.text.is_some() || ev.reasoning.is_some() {
                    self.add(Cat::Assistant, block(ev.text.as_deref()) + block(ev.reasoning.as_deref()) + 4, raw);
                }
            }
            Kind::RequestHeader | Kind::Meta => load = self.header(ev, ty, data, raw),
            _ => {}
        }

        // 压缩：dsh 带被遮蔽的行号，其他整面换掉
        if ev.signal == Some(Signal::Compaction) {
            match data.and_then(|d| d.get("shadowedSeqs")).and_then(Value::as_array) {
                Some(seqs) => {
                    for r in seqs.iter().filter_map(Value::as_u64) {
                        self.remove_raw(r);
                    }
                }
                None => self.clear_surface(),
            }
            self.compaction_pending = true;
            // Codex 的 compacted 行自带替换后的历史
            let payload = obj(ext.get("payload"));
            match payload.and_then(|p| p.get("replacement_history")) {
                Some(Value::Array(h)) if !h.is_empty() => {
                    let mut c = Chars::default();
                    c.json(&Value::Array(h.clone()));
                    self.inject(c, Cat::Inject, raw);
                }
                _ => {
                    if let Some(m) = str_at(payload, "message") {
                        self.inject(chars_of(m), Cat::Inject, raw);
                    }
                }
            }
        }
        load
    }

    fn header(&mut self, ev: &Event, ty: &str, data: Option<&Map<String, Value>>, raw: Option<u64>) -> Option<SkillLoad> {
        let ext = &ev.ext;
        match ev.provider {
            "dsh" => match ty {
                "request/header" => {
                    let header = obj(data.and_then(|d| d.get("header")));
                    if let Some(s) = header.and_then(|h| h.get("system")).and_then(Value::as_str) {
                        self.set_system(chars_of(s));
                    }
                    if let Some(Value::Array(tools)) = header.and_then(|h| h.get("tools")) {
                        self.set_tools(tools);
                    }
                }
                "system/message" => self.set_system(chars_of(ev.text.as_deref().unwrap_or(""))),
                "user/message" | "developer/message" => {
                    let src = obj(data.and_then(|d| d.get("source")))
                        .or_else(|| obj(obj(data.and_then(|d| d.get("message"))).and_then(|m| m.get("source"))));
                    let kind = str_at(src, "kind");
                    let skill = matches!(kind, Some("skill-invocation" | "skill-catalog"));
                    let mut c = Chars::default();
                    match &ev.text {
                        Some(t) => c.text(t),
                        None => {
                            if let Some(v) = data.and_then(|d| d.get("content")) {
                                c.raw(v);
                            }
                        }
                    }
                    self.inject(c, if skill { Cat::Skill } else { Cat::Inject }, raw);
                    if kind == Some("skill-invocation")
                        && let Some(name) = str_at(src, "name")
                    {
                        return Some(SkillLoad { name: name.to_string(), path: str_at(src, "path").map(String::from), count: true });
                    }
                }
                _ => {}
            },
            "claude-code" => {
                if ty == "attachment" {
                    let a = obj(ext.get("attachment"))?;
                    let at = str_at(Some(a), "type").unwrap_or("");
                    if at == "prompt_snapshot" {
                        if let Some(Value::Array(sp)) = a.get("systemPrompt") {
                            let mut c = Chars::default();
                            for (i, x) in sp.iter().enumerate() {
                                if i > 0 {
                                    c.ascii += 2;
                                }
                                c.raw(x);
                            }
                            self.set_system(c);
                        }
                        if let Some(Value::Array(tools)) = a.get("tools") {
                            self.set_tools(tools);
                        }
                        return None;
                    }
                    if CLAUDE_SILENT.contains(&at) {
                        return None;
                    }
                    let skill = at == "skill_listing" || at == "invoked_skills";
                    self.inject(attachment_chars(a, ext), if skill { Cat::Skill } else { Cat::Inject }, raw);
                    return None;
                }
                if ty == "user"
                    && ev.signal.is_none()
                    && let Some(t) = &ev.text
                {
                    self.inject(chars_of(t), Cat::Inject, raw);
                    // 加载 skill 后注入的正文以「Base directory for this skill: <路径>」开头；
                    // 紧跟 Skill 工具结果的是补那次的路径，否则是斜杠命令直接加载的
                    if let Some(rest) = t.trim_start().strip_prefix(CLAUDE_SKILL_DIR) {
                        let path = rest.lines().next().unwrap_or("").trim().to_string();
                        if !path.is_empty() {
                            return Some(match self.awaiting_path.take() {
                                Some(name) => SkillLoad { name, path: Some(path), count: false },
                                None => {
                                    let name = path.rsplit(['/', '\\']).find(|s| !s.is_empty()).unwrap_or(&path).to_string();
                                    SkillLoad { name, path: Some(path), count: true }
                                }
                            });
                        }
                    }
                }
            }
            "codex" => {
                if ev.kind == Kind::RequestHeader
                    && let Some(t) = &ev.text
                {
                    self.inject(chars_of(t), Cat::Inject, raw);
                    let t = t.trim_start();
                    if t.starts_with("<skill>")
                        && let Some(name) = xml_tag(t, "name")
                    {
                        return Some(SkillLoad { name: name.to_string(), path: xml_tag(t, "path").map(String::from), count: true });
                    }
                }
            }
            "pfh" => {
                if ty == "request_header" {
                    if let Some(Value::Array(sections)) = ext.get("systemSections")
                        && !sections.is_empty()
                    {
                        let mut c = Chars::default();
                        for (i, s) in sections.iter().enumerate() {
                            if i > 0 {
                                c.ascii += 2;
                            }
                            if let Some(t) = str_at(s.as_object(), "content") {
                                c.text(t);
                            }
                        }
                        self.set_system(c);
                    }
                    if let Some(Value::Array(names)) = ext.get("toolNames") {
                        self.set_tools(names);
                    }
                } else if ty == "context_queued"
                    && let Some(t) = ext.get("content").and_then(Value::as_str).filter(|s| !s.is_empty())
                {
                    self.inject(chars_of(t), Cat::Inject, raw);
                }
            }
            _ => {
                if ev.kind == Kind::RequestHeader
                    && let Some(t) = &ev.text
                {
                    self.inject(chars_of(t), Cat::Inject, raw);
                }
            }
        }
        None
    }
}

/// 工具结果进上下文的文本：字符串原样，part 数组拼 text、图片跳过，其余 JSON。顺带找 `<skill_content name=…>`
fn output_chars(v: Option<&Value>) -> (Chars, Option<String>) {
    let mut c = Chars::default();
    let mut skill = None;
    let scan = |s: &str, skill: &mut Option<String>| {
        if skill.is_none() {
            *skill = skill_content_name(s);
        }
    };
    match v {
        None | Some(Value::Null) => {}
        Some(Value::String(s)) => {
            c.text(s);
            scan(s, &mut skill);
        }
        Some(Value::Array(parts)) => {
            let mut n = 0;
            for p in parts {
                let ty = p.get("type").and_then(Value::as_str);
                if matches!(ty, Some("image" | "input_image")) {
                    continue;
                }
                if n > 0 {
                    c.ascii += 1;
                }
                n += 1;
                match p.get("text").and_then(Value::as_str) {
                    Some(t) => {
                        c.text(t);
                        scan(t, &mut skill);
                    }
                    None => c.json(p),
                }
            }
        }
        Some(v) => c.json(v),
    }
    (c, skill)
}

/// Claude Code attachment 进上下文的文本：rendered → text / content → files 的内容 → 增量行 → 整个对象
fn attachment_chars(a: &Map<String, Value>, ext: &Map<String, Value>) -> Chars {
    let mut c = Chars::default();
    let join = |c: &mut Chars, items: &[Value], f: &dyn Fn(&Value) -> Option<&str>| {
        for (i, x) in items.iter().enumerate() {
            if i > 0 {
                c.ascii += 1;
            }
            if let Some(t) = f(x) {
                c.text(t);
            }
        }
    };
    if let Some(Value::Array(r)) = ext.get("rendered") {
        join(&mut c, r, &|x| x.get("content").and_then(Value::as_str));
        if c.ascii + c.wide > r.len().saturating_sub(1) as u64 {
            return c;
        }
        c = Chars::default();
    }
    if let Some(t) = a.get("text").and_then(Value::as_str).or_else(|| a.get("content").and_then(Value::as_str)) {
        c.text(t);
        return c;
    }
    for key in ["files", "addedLines", "addedBlocks"] {
        if let Some(Value::Array(items)) = a.get(key) {
            if key == "files" {
                join(&mut c, items, &|x| x.get("content").and_then(Value::as_str));
            } else {
                join(&mut c, items, &Value::as_str);
            }
            return c;
        }
    }
    c.json_obj(a);
    c.solid = true;
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchor_scales_or_splits_other() {
        let parts = Mix { system: 100, user: 50, tool: 50, ..Default::default() };
        // 记全了：按比例缩放到 400
        let a = anchor(&parts, 400, true);
        assert_eq!((a.system, a.user, a.tool, a.other), (200, 100, 100, 0));
        // 没记全：差额单列
        let a = anchor(&parts, 400, false);
        assert_eq!((a.system, a.other), (100, 200));
        // 估多了：照样缩小
        let a = anchor(&parts, 101, false);
        assert_eq!(a.sum(), 101);
        assert_eq!(a.other, 0);
        assert_eq!(anchor(&parts, 0, true), parts);
    }

    #[test]
    fn scaled_keeps_total() {
        let m = Mix { system: 3, user: 3, other: 1, ..Default::default() };
        let s = m.scaled_to(10);
        assert_eq!(s.sum(), 10);
        assert_eq!(m.scaled_to(0).sum(), 0);
    }

    #[test]
    fn chars_estimate() {
        // 8 个 ASCII = 2，3 个汉字 = 2
        assert_eq!(chars_of("abcdefgh").est(), 2);
        assert_eq!(chars_of("上下文").est(), 2);
        assert_eq!(block(Some("")), 0);
        let mut c = Chars::default();
        c.json(&serde_json::json!({"a": [1, "x\n"], "b": null}));
        assert_eq!(c.ascii as usize, serde_json::to_string(&serde_json::json!({"a": [1, "x\n"], "b": null})).unwrap().len());
    }

    #[test]
    fn skill_markers() {
        assert_eq!(skill_content_name("x <skill_content name=\"pdf\">body"), Some("pdf".into()));
        assert_eq!(skill_content_name("<skill_content>"), None);
        assert_eq!(xml_tag("<skill>\n<name>a</name>\n<path>/p/SKILL.md</path>", "path"), Some("/p/SKILL.md"));
    }
}
