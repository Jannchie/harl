//! 会话头：标题、父会话、工作目录、agent 身份。只看快照开头几行，按 tree 缓存在 index.db（快照不可变）。
//! 会话列表用它显示和分组，汇总用 cwd 当项目。

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use harl_core::event::{Kind, Role};
use harl_core::hash::Hash;
use harl_core::normalize::{self, Agent, AgentKind, PathAgent, Provider, Run, TITLE_LEN, Title, clip};
use harl_core::store::{Reader, Record, Store};

/// 开头 TITLE_SCAN 行里第一个像样的用户提示（见 `normalize::prompt_title`）的第一行，截到 TITLE_LEN 字符；
/// 顺带拿事件上的父会话（dsh 的父会话只写在文件头里，光看路径不知道）和工作目录（各家都在某些行上带 `cwd`）
const TITLE_SCAN: usize = 40;
/// head() 的口径版本，变了就 +1，index.db 里的旧缓存自动作废
pub(crate) const HEAD_VERSION: u32 = 4;

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Head {
    pub title: Option<String>,
    /// 路径上看得出的父会话优先，其次是事件里写的（dsh 只写在文件头，Claude Code 的子 agent 每行都带）
    pub parent: Option<String>,
    pub cwd: Option<String>,
    /// 作为 agent 的身份；缓存里的只有内容里写着的，head_for 出来的已按路径和父会话定好（`merge_path_agent`）
    #[serde(default)]
    pub agent: Agent,
}

/// workflow journal 不长（几百行），整份读完才有全部 agent 的 label；别的只读开头
fn head(reader: &mut Reader<'_>, provider: &Provider, rec: &Record, journal: bool) -> Result<Head> {
    let lines = if journal { reader.lines(rec)? } else { reader.lines_prefix(rec, TITLE_SCAN)? };
    Ok(scan(provider, &rec.source, &lines))
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
        match normalize::prompt_title(text, &|t| norm.wrapper_title(t)) {
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

/// 从内容读出来的 head：进程内的、index.db 里的，都没有才读快照。读不出来（chunk 缺失等）的不缓存
fn content_head(store: &Store, reader: &mut Reader<'_>, provider: &Provider, rec: &Record, journal: bool) -> Head {
    if let Some(h) = memo().as_ref().and_then(|m| m.get(&rec.tree).cloned()) {
        return h;
    }
    let h = match store.cached::<Head>("head", HEAD_VERSION, &rec.tree) {
        Ok(Some(h)) => h,
        _ => match head(reader, provider, rec, journal) {
            Ok(h) => {
                let _ = store.put_cached("head", HEAD_VERSION, &rec.tree, &h);
                h
            }
            Err(_) => return Head::default(),
        },
    };
    memo().get_or_insert_default().insert(rec.tree.clone(), h.clone());
    h
}

/// 会话列表的一行：会话 id、会话头、路径上的 run 目录（`fill_agents` 用来找 journal）
pub struct ListRow {
    pub session: String,
    pub head: Head,
    pub dir: Option<String>,
}

/// 取或算一个快照的会话头。缓存按 tree（内容）存，所以只缓存从内容读出来的部分；
/// 从源文件路径推出来的（会话 id、父会话、pfh 的 workspace、workflow 的类别和 run 目录）每次现补——同样的内容可能出现在不同路径下。
/// 内容读不出来时路径上看得出的照样给
pub fn row_for(store: &Store, reader: &mut Reader<'_>, provider: &Provider, rec: &Record) -> ListRow {
    let norm = provider.normalizer(&rec.source);
    let ((session, parent), path) = (norm.identity(), norm.path_agent());
    let mut h = content_head(store, reader, provider, rec, path.kind == Some(AgentKind::Journal));
    h.parent = parent.or(h.parent);
    // pfh 开发版的日志就在 workspace 下面
    h.cwd = h.cwd.or_else(|| rec.source.split_once("/.pfh/").map(|(ws, _)| ws.to_string()));
    let dir = path.run.as_ref().map(|r| r.key.clone());
    h.agent = merge_path_agent(std::mem::take(&mut h.agent), path, h.parent.is_some());
    ListRow { session, head: h, dir }
}

/// 只要会话头（见 `row_for`）
pub fn head_for(store: &Store, reader: &mut Reader<'_>, provider: &Provider, rec: &Record) -> Head {
    row_for(store, reader, provider, rec).head
}

/// 定下 agent 身份。类别：路径说的优先（`subagents/workflows/` 下的就是 workflow），哪里都没说的按有没有父会话分成 main / subagent。
/// 分组键取最可靠的（`RunSource`）：内容里转述的用户请求在镜像副本（平铺、没有 run 目录）里也一样，同一个 agent 的两份副本分在同一组；
/// 其次是 run 目录，最后才是发起它的 prompt
fn merge_path_agent(content: Agent, path: PathAgent, has_parent: bool) -> Agent {
    let kind = match path.kind.unwrap_or(content.kind) {
        AgentKind::Main if has_parent => AgentKind::Subagent,
        k => k,
    };
    let run = content.run.max(path.run);
    Agent { kind, run, ..content }
}

/// 只看一个会话的头补不全、要看整张列表的部分：
/// - workflow agent 的 label 记在同一 run 目录的 journal 里：父会话 → run 目录 → agent 会话 → label
/// - 同一个会话 id 的几份副本（多机器、Claude Desktop 的 ssh 镜像）是同一个 agent：label 哪份有就用哪份，
///   分组键（连同组名）取最可靠的那份（`RunSource`，一样可靠的取先见到的）。平铺的镜像没有 journal、也没有 run 目录，靠原文件那份补
pub fn fill_agents(rows: &mut [ListRow]) {
    let mut journals: HashMap<String, HashMap<String, HashMap<String, String>>> = HashMap::new();
    for r in rows.iter() {
        if let (AgentKind::Journal, Some(parent), Some(dir)) = (&r.head.agent.kind, &r.head.parent, &r.dir) {
            let labels = journals.entry(parent.clone()).or_default().entry(dir.clone()).or_default();
            labels.extend(r.head.agent.labels.iter().cloned());
        }
    }
    for r in rows.iter_mut() {
        if r.head.agent.label.is_some() {
            continue;
        }
        let (Some(parent), Some(dir)) = (&r.head.parent, &r.dir) else { continue };
        let label = journals.get(parent.as_str()).and_then(|m| m.get(dir.as_str())).and_then(|m| m.get(r.session.as_str()));
        r.head.agent.label = label.cloned();
    }
    // journal 的会话 id 都叫 journal，不是同一个东西
    let copy = |r: &ListRow| r.head.agent.kind != AgentKind::Journal;
    type Shared = (Option<String>, Option<Run>, Option<String>);
    let mut best: HashMap<String, Shared> = HashMap::new();
    for r in rows.iter().filter(|r| copy(r)) {
        let a = &r.head.agent;
        let e = best.entry(r.session.clone()).or_default();
        if e.0.is_none() {
            e.0.clone_from(&a.label);
        }
        if a.run.as_ref().map(|r| r.source) > e.1.as_ref().map(|r| r.source) {
            (e.1, e.2) = (a.run.clone(), a.run_title.clone());
        }
    }
    for r in rows.iter_mut().filter(|r| copy(r)) {
        if let Some((label, run, title)) = best.get(r.session.as_str()) {
            let a = &mut r.head.agent;
            a.label = a.label.take().or_else(|| label.clone());
            (a.run, a.run_title) = (run.clone(), title.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use harl_core::normalize::{RunSource, claude};

    use super::*;

    fn lines(ls: &[serde_json::Value]) -> Vec<(Hash, Vec<u8>)> {
        ls.iter()
            .map(|l| {
                let b = format!("{l}\n").into_bytes();
                (harl_core::hash::hash(&b), b)
            })
            .collect()
    }

    fn path_agent(path: &str) -> PathAgent {
        claude::PROVIDER.normalizer(path).path_agent()
    }

    fn run(source: RunSource, key: &str) -> Option<Run> {
        Some(Run { source, key: key.into() })
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
        assert_eq!(h.agent.kind, AgentKind::Workflow);
        assert_eq!(h.agent.run_title.as_deref(), Some("仔细研究一下实现"));
        let req = h.agent.run.clone();
        assert_eq!(req.as_ref().map(|r| r.source), Some(RunSource::Request));
        assert!(serde_json::to_string(&req).unwrap().starts_with("\"req:"));
        // 同一 agent 在 run 目录下的副本：内容一样，分组键也一样；类别和 run 目录的 fallback 来自路径
        let wf = "/h/.claude/projects/p/p1/subagents/workflows/wf_abc/agent-a01.jsonl";
        let merged = merge_path_agent(h.agent.clone(), path_agent(wf), true);
        assert_eq!((merged.kind, merged.run), (AgentKind::Workflow, req));
        let bare = merge_path_agent(Agent::default(), path_agent(wf), true);
        assert_eq!((bare.kind, bare.run), (AgentKind::Workflow, run(RunSource::Dir, "wf_abc")));
        // 没有转述用户请求的：run 目录优先于发起它的 prompt，平铺的副本才用 prompt
        let task_only = scan(&claude::PROVIDER, path, &lines(&[line("[Workflow harness — computed task] follows:\n  审查")]));
        assert_eq!(task_only.agent.run, run(RunSource::Prompt, "x"));
        let merged = merge_path_agent(task_only.agent.clone(), path_agent(wf), true);
        assert_eq!(merged.run, run(RunSource::Dir, "wf_abc"));
        let flat = merge_path_agent(task_only.agent, path_agent(path), true);
        assert_eq!(flat.run, run(RunSource::Prompt, "x"));
        assert_eq!(merge_path_agent(Agent::default(), PathAgent::default(), false).kind, AgentKind::Main);
        assert_eq!(merge_path_agent(Agent::default(), PathAgent::default(), true).kind, AgentKind::Subagent);
    }

    // 线上的样子和以前的字符串一样：kind 小写，run 带来源前缀（run 目录原样）
    #[test]
    fn agent_wire_format() {
        let a = Agent { kind: AgentKind::Other("review".into()), run: run(RunSource::Prompt, "x"), ..Default::default() };
        let s = serde_json::to_string(&a).unwrap();
        assert_eq!(s, r#"{"kind":"review","label":null,"run":"prompt:x","run_title":null}"#);
        assert_eq!(serde_json::from_str::<Agent>(&s).unwrap(), a);
        for (r, wire) in [(run(RunSource::Dir, "wf_ab"), "\"wf_ab\""), (run(RunSource::Request, "0123"), "\"req:0123\"")] {
            assert_eq!(serde_json::to_string(&r).unwrap(), wire);
            assert_eq!(serde_json::from_str::<Option<Run>>(wire).unwrap(), r);
        }
        assert_eq!(serde_json::to_string(&AgentKind::Journal).unwrap(), "\"journal\"");
    }

    // journal 的 label 补到同一 run 目录的 agent 上；镜像副本从原文件那份拿 label 和分组键
    #[test]
    fn fill_from_journal_and_copies() {
        let row = |session: &str, kind: AgentKind, dir: Option<&str>, run: Option<Run>| ListRow {
            session: session.into(),
            head: Head { parent: Some("p1".into()), agent: Agent { kind, run, ..Default::default() }, ..Default::default() },
            dir: dir.map(String::from),
        };
        let (run_a, run_b) = (run(RunSource::Dir, "wf_a"), run(RunSource::Dir, "wf_b"));
        let mut journal = row("journal", AgentKind::Journal, Some("wf_a"), run_a.clone());
        journal.head.agent.labels = vec![("agent-a01".into(), "fix:text".into())];
        let mut rows = vec![
            journal,
            row("agent-a01", AgentKind::Workflow, Some("wf_a"), run_a.clone()),
            // 镜像副本：没有 run 目录，分组键只有 prompt
            row("agent-a01", AgentKind::Workflow, None, run(RunSource::Prompt, "x")),
            row("agent-a02", AgentKind::Workflow, Some("wf_b"), run_b.clone()),
        ];
        fill_agents(&mut rows);
        let got: Vec<_> = rows[1..].iter().map(|r| (r.head.agent.label.as_deref(), r.head.agent.run.clone())).collect();
        assert_eq!(got, vec![(Some("fix:text"), run_a.clone()), (Some("fix:text"), run_a), (None, run_b)]);
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
        assert_eq!(h.agent.kind, AgentKind::Journal);
        assert_eq!(h.agent.labels, vec![("agent-a01".to_string(), "fix:text".to_string())]);
        assert!(h.title.is_none());
    }
}
