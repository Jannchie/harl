//! 会话头：标题、父会话、工作目录、agent 身份。只看快照开头几行，按 tree 缓存在 index.db（快照不可变）。
//! 会话列表用它显示和分组，汇总用 cwd 当项目。

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use harl_core::event::{Kind, Role};
use harl_core::hash::Hash;
use harl_core::normalize::{self, Agent, Provider, claude, clip};
use harl_core::store::{Reader, Record, Store};

/// 开头 TITLE_SCAN 行里第一个像样的用户提示（见 `clean`）的第一行，截到 TITLE_LEN 字符；
/// 顺带拿事件上的父会话（dsh 的父会话只写在文件头里，光看路径不知道）和工作目录（各家都在某些行上带 `cwd`）
const TITLE_SCAN: usize = 40;
const TITLE_LEN: usize = 120;
/// head() 的口径版本，变了就 +1，index.db 里的旧缓存自动作废
pub(crate) const HEAD_VERSION: u32 = 3;

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Head {
    pub title: Option<String>,
    /// 路径上看得出的父会话优先，其次是事件里写的（dsh 只写在文件头，Claude Code 的子 agent 每行都带）
    pub parent: Option<String>,
    pub cwd: Option<String>,
    /// 作为 agent 的身份；head_for 出来的 kind 一定有（没写的按有没有父会话补成 main / subagent）
    #[serde(default)]
    pub agent: Agent,
}

fn head(reader: &mut Reader<'_>, provider: &Provider, rec: &Record) -> Result<Head> {
    let lines = reader.lines_prefix(rec, TITLE_SCAN)?;
    let h = scan(provider, &rec.source, &lines);
    // workflow journal 不长（几百行），整份读完才有全部 agent 的 label
    if h.agent.kind.as_deref() == Some("journal") && lines.len() == TITLE_SCAN {
        return Ok(scan(provider, &rec.source, &reader.lines(rec)?));
    }
    Ok(h)
}

fn scan(provider: &Provider, path: &str, lines: &[(Hash, Vec<u8>)]) -> Head {
    let mut norm = provider.normalizer(path);
    let (mut title, mut fallback, mut parent, mut cwd) = (None, None, None, None);
    for (i, (h, line)) in lines.iter().enumerate() {
        let ev = normalize::line_event(norm.as_mut(), i as u32, h, line);
        if parent.is_none() {
            parent = ev.parent_session.clone();
        }
        if cwd.is_none() {
            cwd = ev.ext.get("cwd").and_then(Value::as_str).map(normalize::slashed);
        }
        let Some(text) = ev.text.as_deref().filter(|_| title.is_none() && ev.role == Some(Role::User)) else { continue };
        let prompt = matches!(ev.kind, Kind::TurnStarted | Kind::Message);
        match clean(text) {
            // 注入的上下文（skill 正文、hook 说明……）也是 user 角色，只认真正的提示
            Title::Real(t) if prompt => title = Some(t),
            Title::Fallback(t) => {
                fallback.get_or_insert(t);
            }
            _ => {}
        }
    }
    let title = title.or(fallback).map(|t| clip(&t, TITLE_LEN));
    Head { title, parent, cwd, agent: norm.agent() }
}

/// 一条 user 文字能不能当标题
#[derive(Debug, PartialEq)]
enum Title {
    /// 用户（或派活的父 agent）真正要做的事
    Real(String),
    /// 包装过的东西里还算有意义的一句（斜杠命令和参数、workflow 转述的用户请求），找不到 Real 时用
    Fallback(String),
    Skip,
}

/// 剥掉各家塞在提示前面的包装：
/// - workflow harness 的说明段（`[Workflow harness — …]`，正文在下一行起）
/// - Codex IDE 插件的 `# Context from my IDE setup:`，真正的话在 `## My request for Codex:` 下面
/// - 斜杠命令回显（`<command-name>/goal</command-name>…<command-args>…`）、`<user_action>`（Codex 的 /review）
/// - `<conversation_history>` 回放里的第一句 `<user>`、`<pasted_content>` 粘贴块的第一行
/// - 其余开头整块的 `<tag>…</tag>`（system-reminder 等）去掉后看剩下的
fn clean(text: &str) -> Title {
    let t = text.trim();
    if t.is_empty() {
        return Title::Skip;
    }
    if let Some((request, body)) = claude::harness_body(t) {
        return match first_line(body) {
            Some(l) if request => Title::Fallback(l),
            Some(l) => Title::Real(l),
            None => Title::Skip,
        };
    }
    // Codex 的 /review 子任务把 AGENTS.md 当成第一条 user 消息
    if t.starts_with("# AGENTS.md instructions for") {
        return Title::Skip;
    }
    if let Some(rest) = t.strip_prefix("# Context from my IDE setup:") {
        return rest.split_once("## My request for Codex:").and_then(|(_, req)| first_line(req)).map_or(Title::Skip, Title::Real);
    }
    if t.starts_with("<command-name>") || t.starts_with("<command-message>") {
        let Some(name) = tag(t, "command-name").filter(|n| !n.is_empty()) else { return Title::Skip };
        return match tag(t, "command-args").filter(|a| !a.is_empty()) {
            Some(args) => Title::Fallback(format!("{name} {}", first_line(&args).unwrap_or_default())),
            None if matches!(name.as_str(), "/clear" | "/compact" | "/exit") => Title::Skip,
            None => Title::Fallback(name),
        };
    }
    if t.starts_with("<user_action>") {
        return tag(t, "action").filter(|a| !a.is_empty()).map_or(Title::Skip, |a| Title::Fallback(format!("/{a}")));
    }
    if t.starts_with("<conversation_history>") {
        return tag(t, "user").and_then(|u| first_line(&u)).map_or(Title::Skip, Title::Real);
    }
    if let Some(rest) = t.strip_prefix("<pasted_content") {
        let body = rest.split_once('>').map_or("", |(_, b)| b);
        let body = body.split_once("</pasted_content").map_or(body, |(b, _)| b);
        return first_line(body).map_or(Title::Skip, Title::Real);
    }
    if let Some(rest) = strip_block(t) {
        return clean(rest);
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
fn first_line(s: &str) -> Option<String> {
    s.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with("[Image")).map(str::to_string)
}

/// `<tag>value</tag>` 里的 value，去掉首尾空白
fn tag(s: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let start = s.find(&open)? + open.len();
    let end = s[start..].find(&format!("</{name}>"))? + start;
    Some(s[start..end].trim().to_string())
}

/// 进程内按 tree 记住从内容读出来的 head：index.db 里那份每次都要查表、反序列化，几千个会话加起来要一秒左右。
/// 活跃会话每次 capture 换一个 tree，旧的由 `retain` 清掉
static MEMO: Mutex<Option<HashMap<String, Head>>> = Mutex::new(None);

fn memo() -> std::sync::MutexGuard<'static, Option<HashMap<String, Head>>> {
    MEMO.lock().unwrap_or_else(|p| p.into_inner())
}

/// 进程内只留这些 tree 的 head
pub fn retain(live: &HashSet<&str>) {
    if let Some(m) = memo().as_mut() {
        m.retain(|tree, _| live.contains(tree.as_str()));
    }
}

/// 取或算一个快照的 head。缓存按 tree（内容）存，所以只缓存从内容读出来的部分；
/// 从源文件路径推出来的（父会话、pfh 的 workspace、workflow 的 run 目录）每次现补——同样的内容可能出现在不同路径下
pub fn head_for(store: &Store, reader: &mut Reader<'_>, provider: &Provider, rec: &Record) -> Option<Head> {
    let hit = memo().as_ref().and_then(|m| m.get(&rec.tree).cloned());
    let mut h = match hit {
        Some(h) => h,
        None => {
            match store.cached::<Head>("head", HEAD_VERSION, &rec.tree) {
                Ok(Some(h)) => {
                    memo().get_or_insert_default().insert(rec.tree.clone(), h.clone());
                    h
                }
                _ => match head(reader, provider, rec) {
                    Ok(h) => {
                        let _ = store.put_cached("head", HEAD_VERSION, &rec.tree, &h);
                        memo().get_or_insert_default().insert(rec.tree.clone(), h.clone());
                        h
                    }
                    // 内容读不出来（chunk 缺失等）不缓存，路径上看得出的父会话和类别照样给
                    Err(_) => Head::default(),
                },
            }
        }
    };
    h.parent = normalize::identity(provider, &rec.source).1.or(h.parent);
    // pfh 开发版的日志就在 workspace 下面
    h.cwd = h.cwd.or_else(|| rec.source.split_once("/.pfh/").map(|(ws, _)| ws.to_string()));
    h.agent = merge_path_agent(std::mem::take(&mut h.agent), normalize::path_agent(provider, &rec.source), h.parent.is_some());
    Some(h)
}

/// 路径说的类别优先（`subagents/workflows/` 下的就是 workflow）。分组键内容里转述的用户请求（`req:`）优先：
/// 它在镜像副本（平铺、没有 run 目录）里也一样，同一个 agent 的两份副本分在同一组；
/// 其次是 run 目录，最后才是发起它的 prompt（`prompt:`）
fn merge_path_agent(content: Agent, path: Agent, has_parent: bool) -> Agent {
    let kind = path.kind.clone().or(content.kind.clone()).unwrap_or_else(|| if has_parent { "subagent" } else { "main" }.into());
    let run = match content.run.clone() {
        Some(r) if r.starts_with("prompt:") => path.run.clone().or(Some(r)),
        r => r.or(path.run.clone()),
    };
    Agent { kind: Some(kind), run, ..content.or(path) }
}

/// 会话列表的一行：会话 id、会话头、路径上的 run 目录（`fill_agents` 用来找 journal）
pub struct ListRow {
    pub session: String,
    pub head: Head,
    pub dir: Option<String>,
}

/// 只看一个会话的头补不全、要看整张列表的部分：
/// - workflow agent 的 label 记在同一 run 目录的 journal 里：(父会话, run 目录) → agent 会话 → label
/// - 同一个会话 id 的几份副本（多机器、Claude Desktop 的 ssh 镜像）是同一个 agent：label 哪份有就用哪份，
///   分组键（连同组名）取最可靠的那份（`run_rank`）。平铺的镜像没有 journal、也没有 run 目录，靠原文件那份补
pub fn fill_agents(rows: &mut [ListRow]) {
    let mut journals: HashMap<(String, String), HashMap<String, String>> = HashMap::new();
    for r in rows.iter() {
        if let (Some("journal"), Some(parent), Some(dir)) = (r.head.agent.kind.as_deref(), &r.head.parent, &r.dir) {
            journals.entry((parent.clone(), dir.clone())).or_default().extend(r.head.agent.labels.iter().cloned());
        }
    }
    for r in rows.iter_mut() {
        if r.head.agent.label.is_some() {
            continue;
        }
        let (Some(parent), Some(dir)) = (&r.head.parent, &r.dir) else { continue };
        r.head.agent.label = journals.get(&(parent.clone(), dir.clone())).and_then(|m| m.get(&r.session)).cloned();
    }
    // journal 的会话 id 都叫 journal，不是同一个东西
    let copy = |r: &ListRow| r.head.agent.kind.as_deref() != Some("journal");
    type Shared = (Option<String>, Option<String>, Option<String>);
    let mut best: HashMap<&str, Shared> = HashMap::new();
    for r in rows.iter().filter(|r| copy(r)) {
        let a = &r.head.agent;
        let e = best.entry(r.session.as_str()).or_default();
        if e.0.is_none() {
            e.0.clone_from(&a.label);
        }
        if run_rank(a.run.as_deref()) > run_rank(e.1.as_deref()) {
            (e.1, e.2) = (a.run.clone(), a.run_title.clone());
        }
    }
    let best: HashMap<String, Shared> = best.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    for r in rows.iter_mut().filter(|r| copy(r)) {
        if let Some((label, run, title)) = best.get(&r.session) {
            let a = &mut r.head.agent;
            a.label = a.label.take().or_else(|| label.clone());
            (a.run, a.run_title) = (run.clone(), title.clone());
        }
    }
}

/// 分组键有多可靠：转述的用户请求（`req:`）> run 目录（`wf_`）> 发起它的 prompt（`prompt:`）> 没有
fn run_rank(run: Option<&str>) -> u8 {
    match run {
        None => 0,
        Some(r) if r.starts_with("prompt:") => 1,
        Some(r) if r.starts_with("req:") => 3,
        Some(_) => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn lines(ls: &[serde_json::Value]) -> Vec<(Hash, Vec<u8>)> {
        ls.iter()
            .map(|l| {
                let b = format!("{l}\n").into_bytes();
                (harl_core::hash::hash(&b), b)
            })
            .collect()
    }

    // 斜杠命令是 meta，只在找不到真正的提示时当标题；真正的提示在后面也要找到它
    #[test]
    fn scan_prefers_real_prompt_over_command() {
        use serde_json::json;
        let path = "/h/.claude/projects/p/s.jsonl";
        let cmd = json!({"type":"user","isMeta":true,"message":{"role":"user","content":"<command-name>/goal</command-name><command-args>扩展家具</command-args>"}});
        let real = json!({"type":"user","promptId":"p1","message":{"role":"user","content":"话说为啥还在匹配？"}});
        let h = scan(&claude::PROVIDER, path, &lines(&[cmd.clone()]));
        assert_eq!(h.title.as_deref(), Some("/goal 扩展家具"));
        let h = scan(&claude::PROVIDER, path, &lines(&[cmd, real]));
        assert_eq!(h.title.as_deref(), Some("话说为啥还在匹配？"));
        assert!(h.parent.is_none());
    }

    // ssh 镜像里平铺的 workflow agent：父会话来自 sessionId，组名来自转述的用户请求，标题是算出来的任务
    #[test]
    fn mirrored_workflow_agent() {
        use serde_json::json;
        let path = "C:/Users/me/.claude/projects/ssh-p1/workflow-agent-a01.jsonl";
        let line = |content: &str| {
            json!({"type":"user","isSidechain":true,"agentId":"a01","sessionId":"p1","promptId":"x","message":{"role":"user","content":content}})
        };
        let ls = lines(&[
            line("[Workflow harness — user request] relays:\n  仔细研究一下实现"),
            line("[Workflow harness — computed task] follows:\n  审查 PR #1654\n  细节"),
        ]);
        let h = scan(&claude::PROVIDER, path, &ls);
        assert_eq!(h.parent.as_deref(), Some("p1"));
        assert_eq!(h.title.as_deref(), Some("审查 PR #1654"));
        assert_eq!(h.agent.kind.as_deref(), Some("workflow"));
        assert_eq!(h.agent.run_title.as_deref(), Some("仔细研究一下实现"));
        let run = h.agent.run.clone().unwrap();
        assert!(run.starts_with("req:"));
        // 同一 agent 在 run 目录下的副本：内容一样，分组键也一样；类别和 run 目录的 fallback 来自路径
        let wf = "/h/.claude/projects/p/p1/subagents/workflows/wf_abc/agent-a01.jsonl";
        let merged = merge_path_agent(h.agent.clone(), normalize::path_agent(&claude::PROVIDER, wf), true);
        assert_eq!((merged.kind.as_deref(), merged.run.as_deref()), (Some("workflow"), Some(run.as_str())));
        let bare = merge_path_agent(Agent::default(), normalize::path_agent(&claude::PROVIDER, wf), true);
        assert_eq!((bare.kind.as_deref(), bare.run.as_deref()), (Some("workflow"), Some("wf_abc")));
        // 没有转述用户请求的：run 目录优先于发起它的 prompt，平铺的副本才用 prompt
        let task_only = scan(&claude::PROVIDER, path, &lines(&[line("[Workflow harness — computed task] follows:\n  审查")]));
        assert_eq!(task_only.agent.run.as_deref(), Some("prompt:x"));
        let merged = merge_path_agent(task_only.agent.clone(), normalize::path_agent(&claude::PROVIDER, wf), true);
        assert_eq!(merged.run.as_deref(), Some("wf_abc"));
        let flat = merge_path_agent(task_only.agent, normalize::path_agent(&claude::PROVIDER, path), true);
        assert_eq!(flat.run.as_deref(), Some("prompt:x"));
        assert_eq!(merge_path_agent(Agent::default(), Agent::default(), false).kind.as_deref(), Some("main"));
    }

    // journal 的 label 补到同一 run 目录的 agent 上；镜像副本从原文件那份拿 label 和分组键
    #[test]
    fn fill_from_journal_and_copies() {
        let row = |session: &str, kind: &str, dir: Option<&str>, run: Option<&str>| ListRow {
            session: session.into(),
            head: Head {
                parent: Some("p1".into()),
                agent: Agent { kind: Some(kind.into()), run: run.map(String::from), ..Default::default() },
                ..Default::default()
            },
            dir: dir.map(String::from),
        };
        let mut journal = row("journal", "journal", Some("wf_a"), Some("wf_a"));
        journal.head.agent.labels = vec![("agent-a01".into(), "fix:text".into())];
        let mut rows = vec![
            journal,
            row("agent-a01", "workflow", Some("wf_a"), Some("wf_a")),
            // 镜像副本：没有 run 目录，分组键只有 prompt
            row("agent-a01", "workflow", None, Some("prompt:x")),
            row("agent-a02", "workflow", Some("wf_b"), Some("wf_b")),
        ];
        fill_agents(&mut rows);
        let got: Vec<_> = rows[1..].iter().map(|r| (r.head.agent.label.as_deref(), r.head.agent.run.as_deref())).collect();
        assert_eq!(got, vec![(Some("fix:text"), Some("wf_a")), (Some("fix:text"), Some("wf_a")), (None, Some("wf_b"))]);
    }

    // journal：整份读完，started 行的 label 都在
    #[test]
    fn journal_labels() {
        use serde_json::json;
        let path = "/h/.claude/projects/p/p1/subagents/workflows/wf_abc/journal.jsonl";
        let ls = lines(&[
            json!({"type":"launched"}),
            json!({"type":"started","key":"k1","agentId":"a01","label":"fix:text","phase":"Fix"}),
            json!({"type":"result","key":"k1","agentId":"a01","result":{}}),
        ]);
        let h = scan(&claude::PROVIDER, path, &ls);
        assert_eq!(h.agent.kind.as_deref(), Some("journal"));
        assert_eq!(h.agent.labels, vec![("agent-a01".to_string(), "fix:text".to_string())]);
        assert!(h.title.is_none());
    }
}
