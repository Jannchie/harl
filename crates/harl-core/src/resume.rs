//! 追加写的会话接着算统计：同一个源文件的新快照只是在旧快照后面加了行时，从旧快照读完时的状态
//! （normalizer + stats::Folder）接着喂新行，不用把整个文件重新解析一遍。活跃会话每次 capture 换一个 tree，
//! 几十 MB 的 JSONL 从头归一化要好几秒，接着算只看新增的那几行。
//!
//! 结果和从头算逐字节一致：normalizer 和 Folder 都是逐行、只往前走的状态机，读完前 k 行的状态
//! 只取决于这 k 行；`finish` 在状态的副本上做，状态本身留着给下一次。
//!
//! 判断"是不是追加"靠 tree（见 store 模块注释，v3）：顶层是块 hash 列表，块是若干行 hash 的列表、按顺序拼起来就是全部行。
//! 旧快照除最后一块外的块 hash 都要原样出现在新 tree 的同一位置；最后一块要么 hash 相同，要么它的行 hash 是新块的前缀。
//! 这样只读新 tree 的顶层、变了的那一块和新行，前面的块（以及它们所在的 pack frame）不碰。
//! 末尾没换行的半行写完后 hash 会变，前缀对不上，自然退回从头算。

use anyhow::{Result, bail};

use crate::hash::Hash;
use crate::normalize::{self, LineNormalizer, Provider};
use crate::stats::{Folder, SessionStats};
use crate::store::{Reader, Record};

/// 一个快照读完时的状态，下一次从这里接着算
pub struct Checkpoint {
    provider: &'static str,
    source: String,
    /// 已经读过的行数
    lines: usize,
    /// 读过的快照的顶层块 hash 列表
    blocks: Vec<Hash>,
    /// 最后一块的行 hash
    last: Vec<Hash>,
    norm: Box<dyn LineNormalizer + Send>,
    folder: Folder,
}

impl Checkpoint {
    /// 检查点对应的快照有几行
    pub fn lines(&self) -> usize {
        self.lines
    }
}

/// 算一个快照的统计。`prev` 是同一个源文件旧快照的检查点：新快照是它的追加时接着算，否则从头算。
/// 返回统计和这个快照的检查点（v3 记录才有：v≤2 的平铺 tree 没有块，判断不了追加）。
pub fn stats(reader: &mut Reader<'_>, provider: &Provider, rec: &Record, prev: Option<Checkpoint>) -> Result<(SessionStats, Option<Checkpoint>)> {
    if rec.v < 3 {
        let lines = reader.lines(rec)?;
        let mut folder = Folder::default();
        normalize::normalize(provider, &rec.source, &lines, |ev| {
            folder.push(&ev);
            Ok(())
        })?;
        return Ok((folder.finish(), None));
    }
    let blocks = reader.blocks(rec)?;
    let (mut cp, first_block, skip) = match prev.filter(|p| p.provider == provider.name && p.source == rec.source) {
        Some(p) => match resume_point(reader, &p, &blocks)? {
            Some((b, skip)) => (p, b, skip),
            None => (start(provider, rec), 0, 0),
        },
        None => (start(provider, rec), 0, 0),
    };
    let last = reader.lines_from(&blocks, first_block, skip, |h, line| {
        let ev = normalize::line_event(cp.norm.as_mut(), cp.lines as u32, h, line);
        cp.folder.push(&ev);
        cp.lines += 1;
        Ok(())
    })?;
    if first_block < blocks.len() {
        cp.last = last;
    }
    // 否则没有新块要读（快照没变或是空文件）：最后一块还是原来那块
    if cp.lines != rec.lines as usize {
        bail!("snapshot {} has {} lines, read {}", rec.tree, rec.lines, cp.lines);
    }
    cp.blocks = blocks;
    let s = cp.folder.clone().finish();
    Ok((s, Some(cp)))
}

/// 从头算的起点
fn start(provider: &Provider, rec: &Record) -> Checkpoint {
    Checkpoint {
        provider: provider.name,
        source: rec.source.clone(),
        lines: 0,
        blocks: Vec::new(),
        last: Vec::new(),
        norm: provider.normalizer(&rec.source),
        folder: Folder::default(),
    }
}

/// 新快照（顶层 `blocks`）是不是 `p` 的追加；是的话返回从哪一块、块里第几行开始读新行
fn resume_point(reader: &mut Reader<'_>, p: &Checkpoint, blocks: &[Hash]) -> Result<Option<(usize, usize)>> {
    let n = p.blocks.len();
    if n == 0 {
        return Ok(Some((0, 0)));
    }
    if blocks.len() < n || blocks[..n - 1] != p.blocks[..n - 1] {
        return Ok(None);
    }
    if blocks[n - 1] == p.blocks[n - 1] {
        return Ok(Some((n, 0)));
    }
    let hs = reader.block_lines(&blocks[n - 1])?;
    Ok(hs.starts_with(&p.last).then_some((n - 1, p.last.len())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use chrono::Utc;
    use serde_json::{Value, json};

    fn temp_store(tag: &str) -> (std::path::PathBuf, Store) {
        let dir = std::env::temp_dir().join(format!("harl-resume-{tag}-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&dir);
        (dir.clone(), Store::open(dir).unwrap())
    }

    /// 从头算（normalize::normalize + Folder），对照组
    fn full(store: &Store, rec: &Record) -> String {
        let p = normalize::by_name(&rec.provider).unwrap();
        let lines = store.reader().lines(rec).unwrap();
        let mut f = Folder::default();
        normalize::normalize(p, &rec.source, &lines, |ev| {
            f.push(&ev);
            Ok(())
        })
        .unwrap();
        serde_json::to_string(&f.finish()).unwrap()
    }

    /// Claude Code：多个 turn、工具调用（含 shell 程序和失败）、skill、usage、压缩、空行和坏行
    fn claude_lines(n_turns: usize) -> Vec<String> {
        let mut out = Vec::new();
        out.push(json!({"type":"attachment","timestamp":"2026-09-19T00:00:00Z","attachment":{"type":"prompt_snapshot","systemPrompt":["You are a coding agent."],"tools":[{"name":"Bash","description":"run","input_schema":{}}]}}).to_string());
        for t in 0..n_turns {
            let ts = |s: usize| format!("2026-09-19T{:02}:{:02}:{:02}Z", (t / 60) % 24, t % 60, s);
            let p = format!("p{t}");
            out.push(json!({"type":"user","uuid":format!("u{t}"),"promptId":p,"cwd":"E:\\p","timestamp":ts(0),"message":{"role":"user","content":format!("do thing {t}")}}).to_string());
            out.push(json!({"type":"assistant","requestId":format!("r{t}a"),"effort":"high","timestamp":ts(1),"message":{"role":"assistant","model":"claude-opus-5","usage":{"input_tokens":10+t,"output_tokens":3,"cache_read_input_tokens":100*t},"content":[{"type":"text","text":"ok"}]}}).to_string());
            out.push(json!({"type":"assistant","requestId":format!("r{t}a"),"timestamp":ts(2),"message":{"role":"assistant","model":"claude-opus-5","usage":{"input_tokens":10+t,"output_tokens":9,"cache_read_input_tokens":100*t},"content":[{"type":"tool_use","id":format!("t{t}"),"name":"Bash","input":{"command":"cd x && cargo test | tee log"}}]}}).to_string());
            out.push(json!({"type":"user","promptId":p,"timestamp":ts(4),"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":format!("t{t}"),"content":"boom","is_error":t % 3 == 0}]}}).to_string());
            if t % 5 == 0 {
                out.push(json!({"type":"assistant","requestId":format!("r{t}s"),"timestamp":ts(5),"message":{"role":"assistant","model":"claude-sonnet-5","content":[{"type":"tool_use","id":format!("s{t}"),"name":"Skill","input":{"skill":"pdf"}}]}}).to_string());
                out.push(json!({"type":"user","promptId":p,"timestamp":ts(6),"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":format!("s{t}"),"content":"Launching skill: pdf"}]}}).to_string());
                out.push(json!({"type":"user","isMeta":true,"promptId":p,"timestamp":ts(6),"message":{"role":"user","content":[{"type":"text","text":"Base directory for this skill: C:\\s\\pdf\n\n# PDF"}]}}).to_string());
            }
            if t % 7 == 3 {
                out.push(json!({"type":"system","subtype":"compact_boundary","timestamp":ts(7),"compactMetadata":{"trigger":"auto"}}).to_string());
                out.push(String::new());
                out.push("{not json".into());
            }
            out.push(json!({"type":"assistant","requestId":format!("r{t}b"),"timestamp":ts(8),"message":{"role":"assistant","model":"claude-opus-5","usage":{"input_tokens":5,"output_tokens":5,"cache_creation_input_tokens":40},"content":[{"type":"text","text":"done"}]}}).to_string());
            out.push(json!({"type":"system","subtype":"turn_duration","durationMs":9000+t,"timestamp":ts(9)}).to_string());
        }
        out
    }

    /// dsh：显式 turn / step、request header、压缩遮蔽
    fn dsh_lines(n_turns: usize) -> Vec<String> {
        let mut out = Vec::new();
        let mut seq = 0u64;
        let mut line = |ty: &str, data: Value| {
            seq += 1;
            json!({"type":ty,"seq":seq,"time":1786885690000i64 + seq as i64 * 1000,"data":data}).to_string()
        };
        for t in 1..=n_turns {
            out.push(line("turn/start", json!({"turn":t})));
            out.push(line("request/header", json!({"turn":t,"header":{"system":"You are a coding agent.","tools":[{"name":"bash","description":"run","parameters":{}}]}})));
            out.push(line("user/message", json!({"turn":t,"source":{"kind":"user"},"content":[{"type":"text","text":format!("please {t}")}]})));
            out.push(line("step/start", json!({"turn":t,"step":1})));
            out.push(line("assistant/message", json!({"turn":t,"step":1,"message":{"role":"assistant","content":[],"id":format!("m{t}")},"usage":{"inputTokens":1000+t}})));
            out.push(line("tool/call", json!({"turn":t,"step":1,"callId":format!("c{t}"),"name":"bash","arguments":"{\"command\": \"git status\"}"})));
            out.push(line("tool/result", json!({"turn":t,"step":1,"message":{"source":{"kind":"tool","callId":format!("c{t}")},"content":[{"type":"tool-result","toolCallId":format!("c{t}"),"isError":false,"content":[{"type":"text","text":"clean"}]}]}})));
            if t % 4 == 0 {
                out.push(line("compaction/summary", json!({"turn":t,"shadowedSeqs":[3]})));
            }
        }
        out
    }

    fn bytes(lines: &[String]) -> Vec<u8> {
        lines.iter().flat_map(|l| format!("{l}\n").into_bytes()).collect()
    }

    /// 一个文件按 cuts 依次长大，每个快照都存进去；每次都从上一次的检查点接着算，和从头算比
    fn check_growth(provider: &str, source: &str, file: &[u8], cuts: &[usize]) -> (usize, usize) {
        let (dir, mut store) = temp_store(provider);
        let p = normalize::by_name(provider).unwrap();
        let mut cp: Option<Checkpoint> = None;
        let (mut resumed, mut fell_back) = (0, 0);
        for &cut in cuts {
            let part = &file[..cut];
            let mut batch = store.batch().unwrap();
            batch.put_snapshot(provider, source, Utc::now(), part.len() as u64, part).unwrap();
            batch.commit().unwrap();
            let rec = store.latest().unwrap().into_values().find(|r| r.source == source).unwrap();
            let mut reader = store.reader();
            let blocks = reader.blocks(&rec).unwrap();
            let resumes = cp.as_ref().map(|c| resume_point(&mut reader, c, &blocks).unwrap().is_some());
            let (s, next) = stats(&mut reader, p, &rec, cp.take()).unwrap();
            assert_eq!(serde_json::to_string(&s).unwrap(), full(&store, &rec), "{provider} cut at {cut}");
            let next = next.expect("v3 记录都有检查点");
            assert_eq!(next.lines(), rec.lines as usize);
            match resumes {
                Some(true) => resumed += 1,
                Some(false) => fell_back += 1,
                None => {}
            }
            cp = Some(next);
        }
        drop(store);
        let _ = std::fs::remove_dir_all(dir);
        (resumed, fell_back)
    }

    /// 行边界上的切点（跨过 1024 行的块边界），外加几个落在行中间的（半行，下一次要退回从头算）
    fn cuts(file: &[u8], every: usize) -> Vec<usize> {
        let ends: Vec<usize> = file.iter().enumerate().filter(|(_, c)| **c == b'\n').map(|(i, _)| i + 1).collect();
        let mut out: Vec<usize> = ends.iter().step_by(every).copied().collect();
        for i in [1usize, 1023, 1024, 1025, 2047, 2048, 2049] {
            if let Some(&e) = ends.get(i - 1) {
                out.push(e);
            }
        }
        // 半行：某行写到一半
        out.push(ends[500] - 7);
        out.push(ends[1500] + 3);
        out.push(file.len());
        out.sort_unstable();
        out.dedup();
        out
    }

    // 任意切点续算的结果和从头算逐字节一致；正常追加时确实走了续算
    #[test]
    fn resumed_equals_full_claude() {
        let file = bytes(&claude_lines(260));
        let (resumed, fell_back) = check_growth("claude-code", "/h/.claude/projects/p/s.jsonl", &file, &cuts(&file, 97));
        // 只有两个半行之后的那次退回从头算
        assert!(resumed > 10 && fell_back == 2, "resumed {resumed}, fell back {fell_back}");
    }

    #[test]
    fn resumed_equals_full_dsh() {
        let file = bytes(&dsh_lines(330));
        let (resumed, fell_back) = check_growth("dsh", "/h/.dsh/sessions/--p--/s1/session.v3.jsonl.zstd", &file, &cuts(&file, 131));
        assert!(resumed > 10 && fell_back == 2, "resumed {resumed}, fell back {fell_back}");
    }

    // 不是追加（中间改了一行）：退回从头算，结果照样对
    #[test]
    fn rewrite_falls_back_to_full() {
        let lines = claude_lines(150);
        let a = bytes(&lines);
        let mut changed = lines.clone();
        changed[3] = changed[3].replace("do thing", "do other thing");
        changed.extend(claude_lines(160).into_iter().skip(lines.len()));
        let b = bytes(&changed);
        let (dir, mut store) = temp_store("rewrite");
        let p = normalize::by_name("claude-code").unwrap();
        let src = "/h/.claude/projects/p/r.jsonl";
        let mut cp = None;
        for part in [&a, &b] {
            let mut batch = store.batch().unwrap();
            batch.put_snapshot("claude-code", src, Utc::now(), part.len() as u64, part).unwrap();
            batch.commit().unwrap();
            let rec = store.latest().unwrap().into_values().find(|r| r.source == src).unwrap();
            let (s, next) = stats(&mut store.reader(), p, &rec, cp.take()).unwrap();
            assert_eq!(serde_json::to_string(&s).unwrap(), full(&store, &rec));
            cp = next;
        }
        drop(store);
        let _ = std::fs::remove_dir_all(dir);
    }

    // 逐行喂 Provider::normalizer + line_event（续算走的路）和 normalize::normalize 逐个事件一致（含空行、坏行、CRLF）
    #[test]
    fn line_event_matches_normalize() {
        let mut lines: Vec<Vec<u8>> = claude_lines(12).into_iter().map(|l| format!("{l}\n").into_bytes()).collect();
        lines.push(b"{\"type\":\"user\"}\r\n".to_vec());
        lines.push(b"half a li".to_vec());
        let lines: Vec<(Hash, Vec<u8>)> = lines.into_iter().map(|l| (crate::hash::hash(&l), l)).collect();
        for p in normalize::PROVIDERS {
            let path = "/h/.claude/projects/p/s.jsonl";
            let mut want = Vec::new();
            normalize::normalize(p, path, &lines, |ev| {
                want.push(serde_json::to_string(&ev).unwrap());
                Ok(())
            })
            .unwrap();
            let mut norm = p.normalizer(path);
            let got: Vec<String> = lines
                .iter()
                .enumerate()
                .map(|(i, (h, l))| serde_json::to_string(&normalize::line_event(norm.as_mut(), i as u32, h, l)).unwrap())
                .collect();
            assert_eq!(got, want, "{}", p.name);
        }
    }
}
