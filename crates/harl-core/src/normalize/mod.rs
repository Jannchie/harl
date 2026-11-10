//! raw 行 → canonical Event。每个 harness 一个模块，这里放注册表和公用工具。
//!
//! 一个 provider 要回答三件事：哪些文件是我的（`owns`）、默认去哪找（`default_roots`）、
//! 怎么逐行归一化（`new`）。加一个 harness 只需在 `PROVIDERS` 里加一项。

pub mod claude;
pub mod codex;
pub mod dsh;
pub mod homes;
pub mod pfh;
pub mod tools;

use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use crate::event::{Event, Prov, Usage};
use crate::hash::{Hash, hex};
use crate::store::Record;

#[derive(Clone, Copy)]
pub struct Provider {
    pub name: &'static str,
    /// 这个路径（已换成 `/` 分隔）是不是本 provider 的会话文件
    owns: fn(&str) -> bool,
    /// 默认扫描根目录（只返回存在的）
    default_roots: fn(&Path) -> Vec<PathBuf>,
    /// Send：留状态接着算的检查点（`resume`）要放进跨线程的缓存
    new: fn(&str) -> Box<dyn LineNormalizer + Send>,
    /// 同一会话被迁移成新格式后，旧文件还留着：这里给出所有可能取代它的更新格式文件（可能跳过中间版本迁移），
    /// 其中任何一个也在存储里时旧的不进会话视图
    superseded_by: fn(&str) -> Vec<String>,
}

impl Provider {
    /// 这个文件的逐行 normalizer。和 `normalize` 一样从第 0 行开始喂（`line_event`）
    pub fn normalizer(&self, path: &str) -> Box<dyn LineNormalizer + Send> {
        (self.new)(path)
    }
}

pub static PROVIDERS: &[Provider] = &[pfh::PROVIDER, dsh::PROVIDER, codex::PROVIDER, claude::PROVIDER];

/// `source` 已是 `/` 分隔的绝对路径（见 store::source_key）
pub fn provider_of(source: &str) -> Option<&'static Provider> {
    PROVIDERS.iter().find(|pr| (pr.owns)(source))
}

pub fn by_name(name: &str) -> Option<&'static Provider> {
    PROVIDERS.iter().find(|pr| pr.name == name)
}

/// 各 provider 在本机 home 和跨 WSL / Windows 能看到的其他 home（见 `homes`）下的默认根，只留存在的
pub fn default_roots(home: &Path) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    homes::all(home)
        .iter()
        .flat_map(|h| PROVIDERS.iter().flat_map(|pr| (pr.default_roots)(h)))
        .filter(|r| r.is_dir() && seen.insert(r.clone()))
        .collect()
}

/// 去掉被同一会话的新格式文件取代的记录。raw 层照旧全存，只是会话列表 / 统计不重复算
pub fn live(records: impl IntoIterator<Item = Record>) -> Vec<Record> {
    let records: Vec<Record> = records.into_iter().collect();
    let have: HashSet<(String, String)> = records.iter().map(|r| (r.machine.clone(), r.source.clone())).collect();
    records
        .into_iter()
        .filter(|r| {
            let newer = by_name(&r.provider).map(|p| (p.superseded_by)(&r.source)).unwrap_or_default();
            !newer.into_iter().any(|n| have.contains(&(r.machine.clone(), n)))
        })
        .collect()
}

/// 逐行、带状态（Claude Code 要记住当前 turn/step）
pub trait LineNormalizer {
    /// 这个文件的会话 id 和父会话（子 agent 时有）
    fn identity(&self) -> (String, Option<String>);
    fn line(&mut self, seq: u32, obj: Map<String, Value>, prov: Prov) -> Event;
    /// 读过的行里看出来的 agent 身份。只放内容里写着的：会话头按内容（tree）缓存，同样的内容可能出现在不同路径下
    fn agent(&self) -> Agent {
        Agent::default()
    }
    /// 只从路径看得出的 agent 身份（Claude Code 的 `subagents/workflows/wf_<run>/`）。new 之后就定了，不用喂行
    fn path_agent(&self) -> PathAgent {
        PathAgent::default()
    }
    /// 本 provider 塞在用户提示前面的包装（斜杠命令回显、IDE 插件的上下文……）当标题时怎么算；
    /// 不认得的给 None，交给 `prompt_title` 的通用规则
    fn wrapper_title(&self, _text: &str) -> Option<Title> {
        None
    }
}

/// 一个会话作为 agent 的身份：哪一类、叫什么、属于哪一次编排。会话列表用它在根会话下面分组。
/// label / run 都可能拿不到
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Agent {
    /// normalizer 给的只是内容里写着的；路径上的类别和有没有父会话由会话头最后定（harl 的 head）
    pub kind: AgentKind,
    /// 短名字：Codex 的 agent 路径 / 昵称、workflow 里脚本给的 label
    pub label: Option<String>,
    /// 同一次编排（一次 workflow 运行）的分组键；只在同一个根会话下面有意义
    pub run: Option<Run>,
    /// 这次编排是谁、为什么发起的：workflow 转述的用户请求（第一行）
    pub run_title: Option<String>,
    /// workflow journal 记的 agentId → label（只有 journal 有，按出现顺序）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<(String, String)>,
}

/// 只从路径看得出的 agent 身份：类别和 run 目录
#[derive(Debug, Default, Clone, PartialEq)]
pub struct PathAgent {
    pub kind: Option<AgentKind>,
    pub run: Option<Run>,
}

/// agent 的类别。序列化成小写字符串（`main`、`subagent`……，`Other` 原样）
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum AgentKind {
    /// 根会话；内容里没写类别的也先是它，有父会话的由会话头改成 Subagent
    #[default]
    Main,
    Subagent,
    Fork,
    /// workflow 脚本起的 agent
    Workflow,
    /// workflow 的运行日志，不是对话
    Journal,
    /// provider 自己的类别：Codex 的 `review`、dsh 文件头的 `origin`
    Other(String),
}

impl AgentKind {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Main => "main",
            Self::Subagent => "subagent",
            Self::Fork => "fork",
            Self::Workflow => "workflow",
            Self::Journal => "journal",
            Self::Other(s) => s,
        }
    }
}

impl From<&str> for AgentKind {
    fn from(s: &str) -> Self {
        match s {
            "main" => Self::Main,
            "subagent" => Self::Subagent,
            "fork" => Self::Fork,
            "workflow" => Self::Workflow,
            "journal" => Self::Journal,
            _ => Self::Other(s.to_string()),
        }
    }
}

impl Serialize for AgentKind {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for AgentKind {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Self::from(String::deserialize(d)?.as_str()))
    }
}

/// 同一次编排的分组键：从哪来、键是什么。先按来源比可靠程度
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Run {
    pub source: RunSource,
    pub key: String,
}

/// 分组键的来源，越往后越可靠
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RunSource {
    /// 父会话发起它的那个 prompt（`prompt:<promptId>`）：平铺的镜像副本只有这个
    Prompt,
    /// run 目录名（`wf_<id>`），只从路径看得出
    Dir,
    /// workflow 转述的用户请求的哈希（`req:<12 位>`）：镜像副本里也一样
    Request,
}

/// 序列化的样子：`prompt:<id>`、run 目录名原样（`wf_<id>`）、`req:<hash>`
impl fmt::Display for Run {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.source {
            RunSource::Prompt => write!(f, "prompt:{}", self.key),
            RunSource::Dir => f.write_str(&self.key),
            RunSource::Request => write!(f, "req:{}", self.key),
        }
    }
}

impl Serialize for Run {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Run {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        let (source, key) = match (s.strip_prefix("prompt:"), s.strip_prefix("req:")) {
            (Some(k), _) => (RunSource::Prompt, k),
            (_, Some(k)) => (RunSource::Request, k),
            _ => (RunSource::Dir, s.as_str()),
        };
        Ok(Run { source, key: key.to_string() })
    }
}

/// 截到 max 个字符，截掉了就以 … 结尾
pub fn clip(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

// ---- 标题 ----

/// 标题（会话标题、workflow 的组名）最多这么多字符
pub const TITLE_LEN: usize = 120;

/// 一条 user 文字能不能当标题
#[derive(Debug, PartialEq)]
pub enum Title {
    /// 用户（或派活的父 agent）真正要做的事
    Real(String),
    /// 包装过的东西里还算有意义的一句（斜杠命令和参数、workflow 转述的用户请求），找不到 Real 时用
    Fallback(String),
    Skip,
}

/// 一条 user 文字当标题时怎么算：先问 provider 认不认得这种包装（`wrapper`，见 `LineNormalizer::wrapper_title`），再剥通用的：
/// - `<conversation_history>` 回放里的第一句 `<user>`、`<pasted_content>` 粘贴块的第一行
/// - 其余开头整块的 `<tag>…</tag>`（system-reminder 等）去掉后看剩下的
pub fn prompt_title(text: &str, wrapper: &dyn Fn(&str) -> Option<Title>) -> Title {
    let t = text.trim();
    if t.is_empty() {
        return Title::Skip;
    }
    if let Some(title) = wrapper(t) {
        return title;
    }
    if t.starts_with("<conversation_history>") {
        return xml_tag(t, "user").and_then(first_line).map_or(Title::Skip, Title::Real);
    }
    if let Some(rest) = t.strip_prefix("<pasted_content") {
        let body = rest.split_once('>').map_or("", |(_, b)| b);
        let body = body.split_once("</pasted_content").map_or(body, |(b, _)| b);
        return first_line(body).map_or(Title::Skip, Title::Real);
    }
    if let Some(rest) = strip_block(t) {
        return prompt_title(rest, wrapper);
    }
    first_line(t).map_or(Title::Skip, Title::Real)
}

/// 开头是一整块 `<name …>…</name>` 时，返回它后面的部分
fn strip_block(t: &str) -> Option<&str> {
    let name: String = t.strip_prefix('<')?.chars().take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')).collect();
    if name.is_empty() {
        return None;
    }
    let close = format!("</{name}");
    let end = t.find(&close)? + close.len();
    let end = t[end..].find('>')? + end + 1;
    Some(&t[end..])
}

/// 第一行有字的（图片占位 `[Image …]` 不算），去掉首尾空白
pub fn first_line(s: &str) -> Option<String> {
    s.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with("[Image")).map(str::to_string)
}

/// 把一个快照的每一行变成事件，交给 sink。不丢行：空行和解析不了的半行也各是一个 Meta 事件。
pub fn normalize(
    provider: &Provider,
    path: &str,
    lines: &[(Hash, Vec<u8>)],
    mut sink: impl FnMut(Event) -> Result<()>,
) -> Result<()> {
    let mut ctx = provider.normalizer(path);
    for (i, (h, line)) in lines.iter().enumerate() {
        sink(line_event(ctx.as_mut(), i as u32, h, line))?;
    }
    Ok(())
}

/// 一行 raw（可带结尾换行）→ 事件。`seq` 是行号，要按顺序、从 0 开始逐行喂同一个 normalizer；
/// 空行和解析不了的半行也各给一个事件（`_unparsed`）
pub fn line_event(norm: &mut dyn LineNormalizer, seq: u32, h: &Hash, line: &[u8]) -> Event {
    let body = trim_line(line);
    let obj = match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(m)) => m,
        _ => {
            let mut m = Map::new();
            m.insert("_unparsed".into(), Value::String(String::from_utf8_lossy(body).into_owned()));
            m
        }
    };
    norm.line(seq, obj, Prov { chunk: hex(h) })
}

fn trim_line(line: &[u8]) -> &[u8] {
    let mut end = line.len();
    while end > 0 && matches!(line[end - 1], b'\n' | b'\r') {
        end -= 1;
    }
    &line[..end]
}

// ---- 路径 ----

/// 统一成 `/` 分隔。不用 std::path：Windows 上记下的路径要能在 Linux 上解析。
pub fn slashed(path: &str) -> String {
    path.replace('\\', "/")
}

/// `<tag>value</tag>` 里的 value，去掉首尾空白；空的算没有
pub fn xml_tag<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let i = text.find(&open)? + open.len();
    let j = text[i..].find(&format!("</{tag}>"))?;
    Some(text[i..i + j].trim()).filter(|s| !s.is_empty())
}

/// 文件名去掉 .jsonl（以及压缩后缀）
pub fn file_stem(path: &str) -> &str {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let name = name.strip_suffix(".zstd").or_else(|| name.strip_suffix(".zst")).unwrap_or(name);
    name.strip_suffix(".jsonl").unwrap_or(name)
}

/// 所在目录名（dsh 这种一个会话一个目录的）
pub fn dir_name(path: &str) -> &str {
    let mut it = path.rsplit(['/', '\\']);
    it.next();
    it.next().unwrap_or("")
}

// ---- JSON 对象工具：映射过的字段从对象里拿走，剩下的留给 ext ----

/// 能解析成 T 才拿走；形状不对（比如 `input_tokens: "10"`）就原样留在对象里，不丢
pub fn take<T: DeserializeOwned>(obj: &mut Map<String, Value>, key: &str) -> Option<T> {
    let v = T::deserialize(obj.get(key)?).ok()?;
    obj.remove(key);
    Some(v)
}

pub fn take_str(obj: &mut Map<String, Value>, key: &str) -> Option<String> {
    take(obj, key)
}

pub fn take_u64(obj: &mut Map<String, Value>, key: &str) -> Option<u64> {
    take(obj, key)
}

/// RFC3339，转成 UTC
pub fn take_ts(obj: &mut Map<String, Value>, key: &str) -> Option<DateTime<Utc>> {
    take(obj, key)
}

pub fn take_usage(obj: &mut Map<String, Value>, key: &str) -> Option<Usage> {
    let mut u: Usage = take(obj, key)?;
    // 1h 那份是 cache_write 的子集；报错了也不能比总数多
    u.cache_write_1h = u.cache_write_1h.map(|h| h.min(u.cache_write.unwrap_or(0)));
    Some(u)
}

/// 一个文本类 content part：`key` 字段移进 out（不复制），`type` 丢掉，还有剩的字段留进 rest
pub fn take_text_part(mut p: Value, key: &str, out: &mut Vec<String>, rest: &mut Vec<Value>) {
    if let Some(m) = p.as_object_mut() {
        out.push(take_str(m, key).unwrap_or_default());
        m.remove("type");
        if !m.is_empty() {
            rest.push(p);
        }
    }
}

/// 把 part 里映射掉的键拿走；还有剩就留进 rest
pub fn keep_rest(mut p: Value, taken: &[&str], rest: &mut Vec<Value>) {
    if let Some(m) = p.as_object_mut() {
        for k in taken {
            m.remove(*k);
        }
        if !m.is_empty() {
            rest.push(p);
        }
    }
}

/// 多段文本拼成正文；没有就是 None
pub fn join_texts(mut v: Vec<String>) -> Option<String> {
    match v.len() {
        0 => None,
        1 => v.pop(),
        _ => Some(v.join("
")),
    }
}

/// 测试用：把内联的行喂给 `normalize`，收成事件列表
#[cfg(test)]
pub fn run(provider: &Provider, path: &str, lines: &[&str]) -> Vec<Event> {
    let lines: Vec<(Hash, Vec<u8>)> = lines
        .iter()
        .map(|l| {
            let b = format!("{l}\n").into_bytes();
            (crate::hash::hash(&b), b)
        })
        .collect();
    let mut out = Vec::new();
    normalize(provider, path, &lines, |ev| {
        out.push(ev);
        Ok(())
    })
    .unwrap();
    out
}

#[cfg(test)]
mod tests {
    use chrono::DateTime;

    use super::*;

    const DIR: &str = "/h/.dsh/sessions/--E--/session-a";

    fn rec(provider: &str, machine: &str, source: &str) -> Record {
        let t = DateTime::from_timestamp(0, 0).unwrap();
        Record {
            v: 3,
            provider: provider.into(),
            machine: machine.into(),
            source: source.into(),
            tree: String::new(),
            blob: String::new(),
            size: 0,
            file_size: None,
            lines: 0,
            mtime: t,
            captured_at: t,
        }
    }

    fn dsh(machine: &str, file: &str) -> Record {
        rec("dsh", machine, &format!("{DIR}/{file}"))
    }

    /// 同一台机器上同目录这几版 dsh 文件，live 后留下的文件名
    fn kept(files: &[&str]) -> Vec<String> {
        let recs = files.iter().map(|f| dsh("m", f));
        live(recs).into_iter().map(|r| r.source.rsplit('/').next().unwrap().to_string()).collect()
    }

    const V0: &str = "session.jsonl.zstd";
    const V3: &str = "session.v3.jsonl.zstd";
    const V4: &str = "session.v4.jsonl.zstd";

    // 只留同目录最新的那一版，不管迁移时有没有跳过中间版本
    #[test]
    fn keeps_only_newest_dsh_version() {
        assert_eq!(kept(&[V0, V4]), [V4]);
        assert_eq!(kept(&[V4, V0]), [V4]);
        assert_eq!(kept(&[V0, V3]), [V3]);
        assert_eq!(kept(&[V3, V4]), [V4]);
        assert_eq!(kept(&[V0, V3, V4]), [V4]);
        for f in [V0, V3, V4] {
            assert_eq!(kept(&[f]), [f]);
        }
    }

    // 只在同一台机器、同一目录里比；别的 provider 不受影响
    #[test]
    fn supersession_is_per_machine_and_dir() {
        let other_dir = rec("dsh", "m", &format!("/h/.dsh/sessions/--E--/session-b/{V4}"));
        let recs = vec![dsh("m", V0), dsh("n", V4), other_dir, rec("claude-code", "m", "/h/.claude/projects/p/s.jsonl")];
        assert_eq!(live(recs).len(), 4);
    }

    /// 各家的包装都认（测试里不分 provider）
    fn clean(t: &str) -> Title {
        prompt_title(t, &|t| claude::wrapper_title(t).or_else(|| codex::wrapper_title(t)))
    }

    fn real(s: &str) -> Title {
        Title::Real(s.into())
    }

    fn fallback(s: &str) -> Title {
        Title::Fallback(s.into())
    }

    #[test]
    fn workflow_harness_preamble_is_stripped() {
        let task = "[Workflow harness — computed task] The task text below was computed at runtime. The computed task text follows:\n  \n  项目 E:\\code\\sbs 的修复\n  更多";
        assert_eq!(clean(task), real("项目 E:\\code\\sbs 的修复"));
        let req = "[Workflow harness — user request] The harness relays, verbatim and indented below, the user request:\n  仔细研究一下实现";
        assert_eq!(clean(req), fallback("仔细研究一下实现"));
        assert_eq!(clean("[Workflow harness — computed task] nothing follows"), Title::Skip);
    }

    #[test]
    fn codex_wrappers() {
        let ide = "# Context from my IDE setup:\n\n## Active file: a.md\n\n## Open tabs:\n- a.md\n\n## My request for Codex:\n修一下标题\n";
        assert_eq!(clean(ide), real("修一下标题"));
        assert_eq!(clean("# Context from my IDE setup:\n\n## Active file: a.md\n"), Title::Skip);
        assert_eq!(clean("# AGENTS.md instructions for /p\n\n<INSTRUCTIONS>"), Title::Skip);
        let clear = "<command-name>/clear</command-name>\n            <command-message>clear</command-message>\n            <command-args></command-args>";
        assert_eq!(clean(clear), Title::Skip);
        let goal = "<command-name>/goal</command-name>\n<command-message>goal</command-message>\n<command-args>继续整理</command-args>";
        assert_eq!(clean(goal), fallback("/goal 继续整理"));
        assert_eq!(clean("<command-message>simplify</command-message>\n<command-name>/simplify</command-name>"), fallback("/simplify"));
        let review = "<user_action>\n  <context>User initiated a review task.</context>\n  <action>review</action>\n</user_action>";
        assert_eq!(clean(review), fallback("/review"));
    }

    #[test]
    fn blocks_and_pastes() {
        assert_eq!(clean("<system-reminder>\nx\n</system-reminder>\n真正的问题"), real("真正的问题"));
        assert_eq!(clean("<system-reminder>x</system-reminder>"), Title::Skip);
        assert_eq!(clean("\n\n<pasted_content id=\"4c04\">\n题目 1/14\n选项 A\n</pasted_content id=\"4c04\">\n"), real("题目 1/14"));
        let hist = "<conversation_history>\nThis conversation began before this session.\n\n<user>\n制作 ppt\n</user>\n</conversation_history>";
        assert_eq!(clean(hist), real("制作 ppt"));
        assert_eq!(clean("[Image #1]\n看看这个"), real("看看这个"));
        assert_eq!(clean("  普通的一句\n第二行"), real("普通的一句"));
    }
}
