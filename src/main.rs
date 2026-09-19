//! harl — raw 层（行级内容寻址 + zstd pack，逐字节可逆）和 canonical 层（跨 harness 的事件流）。

mod event;
mod hash;
mod normalize;
mod pack;
mod store;

use std::fs;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand};
use walkdir::WalkDir;

use store::{Outcome, Record, Store, Verified};

#[derive(Parser)]
#[command(name = "harl", version, about = "Append-only archive of coding-agent sessions")]
struct Cli {
    /// 数据目录（默认 $HARL_DATA_DIR 或 ~/.harl）
    #[arg(long, global = true, env = "HARL_DATA_DIR")]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 把会话文件快照进 raw 存储
    Capture {
        /// 扫描根目录，可重复；默认是各 provider 的标准位置
        #[arg(long = "root")]
        roots: Vec<PathBuf>,
    },
    /// 列出每个源文件的最新快照
    Ls,
    /// 存储总览
    Status,
    /// 验证每个快照能逐字节还原（默认只查最新快照）
    Check {
        /// 查 manifest 里的每一条记录，而不只是最新的
        #[arg(long)]
        all: bool,
    },
    /// 还原一个快照到 stdout（参数是 tree hash，或源文件路径）
    Cat { what: String },
    /// 从 packs 重建 index.db
    Reindex,
    /// 把一个会话归一化成 canonical 事件流（NDJSON）。参数匹配源文件路径的任意子串
    Events { needle: String },
}

fn home() -> Result<PathBuf> {
    dirs::home_dir().context("找不到 home 目录")
}

/// 扫到的一个会话文件：source key 和目录遍历时顺手拿到的 size/mtime（Windows 上不用再 stat 一次）
struct Found {
    source: String,
    size: u64,
    mtime: DateTime<Utc>,
}

/// 递归找出所有 .jsonl（包含子 agent 目录），按 source 去重：同一个文件不管从哪个 root、用什么写法找到都只算一个
fn discover(roots: &[PathBuf]) -> Vec<Found> {
    let mut files: Vec<Found> = roots
        .iter()
        .flat_map(|root| WalkDir::new(root).into_iter().filter_map(Result::ok))
        .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|x| x == "jsonl"))
        .filter_map(|e| {
            let source = store::source_key(e.path());
            match e.metadata() {
                Ok(m) => {
                    let mtime = m.modified().map(DateTime::<Utc>::from).unwrap_or_else(|_| Utc::now());
                    Some(Found { source, size: m.len(), mtime })
                }
                Err(err) => {
                    eprintln!("warning: 跳过 {source}: {err}");
                    None
                }
            }
        })
        .collect();
    files.sort_by(|a, b| a.source.cmp(&b.source));
    files.dedup_by(|a, b| a.source == b.source);
    files
}

fn capture(store: &mut Store, roots: &[PathBuf]) -> Result<()> {
    let files = discover(roots);
    let (mut new, mut changed, mut unchanged, mut unknown, mut failed) = (0usize, 0usize, 0usize, 0usize, 0usize);
    let mut batch = store.batch()?;

    for f in &files {
        let Some(provider) = normalize::provider_of(&f.source) else {
            unknown += 1;
            continue;
        };
        // size/mtime 没变就不读
        if batch.unchanged_by_stat(&f.source, f.size, f.mtime) {
            unchanged += 1;
            continue;
        }
        // 单个文件读不了（权限、正被独占写、已删除……）只跳过它，不让整批白做
        let bytes = match fs::read(&f.source) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("warning: 跳过 {}: {e}", f.source);
                failed += 1;
                continue;
            }
        };
        match batch.put_snapshot(provider.name, &f.source, f.mtime, &bytes)? {
            Outcome::New => new += 1,
            Outcome::Changed => changed += 1,
            Outcome::Unchanged => unchanged += 1,
        }
    }

    let done = batch.commit()?;
    println!(
        "discovered {}  new {new}  changed {changed}  unchanged {unchanged}  unknown {unknown}  failed {failed}  |  {} new chunks, {} raw → pack {}",
        files.len(),
        done.new_chunks,
        human_bytes(done.stored_bytes),
        done.pack
            .map(|(name, bytes)| format!("{name} ({} of {} logical)", human_bytes(bytes), human_bytes(done.logical)))
            .unwrap_or_else(|| "-".into())
    );
    Ok(())
}

fn ls(store: &Store) -> Result<()> {
    let latest = store.latest()?;
    for rec in latest.values() {
        println!(
            "{:>9} {:>6}  {}  {:<12} {}",
            human_bytes(rec.size),
            rec.lines,
            rec.captured_at.format("%Y-%m-%d %H:%M"),
            rec.provider,
            rec.source
        );
    }
    println!("{} sources", latest.len());
    Ok(())
}

fn status(store: &Store) -> Result<()> {
    let records = store.records()?;
    let n_records = records.len();
    let latest = store::latest_of(records);
    let logical: u64 = latest.values().map(|r| r.size).sum();
    let s = store.stats()?;
    println!("data dir   {}", store.root().display());
    println!("machine    {}", store.machine());
    println!("sources    {}   ({n_records} records)", latest.len());
    println!("logical    {}   (latest snapshots)", human_bytes(logical));
    println!("packs      {}   {}   {} chunks", s.packs, human_bytes(s.pack_bytes), s.chunks);
    println!("index.db   {}", human_bytes(s.index_bytes));
    if logical > 0 {
        println!("ratio      {:.1}%  (packs / logical)", s.pack_bytes as f64 * 100.0 / logical as f64);
    }
    Ok(())
}

fn check(store: &Store, all: bool) -> Result<()> {
    let records: Vec<Record> = if all { store.records()? } else { store.latest()?.into_values().collect() };
    let mut reader = store.reader();
    let (mut ok, mut bad) = (0usize, 0usize);
    for rec in &records {
        match reader.verify(rec) {
            Ok(Verified::Ok) => ok += 1,
            Ok(Verified::Mismatch { got_len }) => {
                bad += 1;
                eprintln!("MISMATCH {}  got {got_len} bytes, want {}", rec.source, rec.size);
            }
            Err(e) => {
                bad += 1;
                eprintln!("ERROR    {}  {e}", rec.source);
            }
        }
    }
    println!("{} snapshots checked: {ok} ok, {bad} bad", records.len());
    if bad > 0 {
        std::process::exit(1);
    }
    Ok(())
}

/// 按路径子串找唯一一个最新快照
fn find_one(store: &Store, needle: &str) -> Result<Record> {
    let latest = store.latest()?;
    let hits: Vec<&Record> = latest.values().filter(|r| r.source.contains(needle)).collect();
    match hits.as_slice() {
        [one] => Ok((*one).clone()),
        [] => bail!("没有源文件路径包含 {needle:?}"),
        many => bail!(
            "{needle:?} 匹配了 {} 个源文件，请更具体：\n{}",
            many.len(),
            many.iter().map(|r| format!("  {}", r.source)).collect::<Vec<_>>().join("\n")
        ),
    }
}

fn cat(store: &Store, what: &str) -> Result<()> {
    // 64 位 hex 当 tree hash；tree 的编码由记录版本决定，所以也得先找到引用它的记录（哪台机器的都行）
    let is_hash = what.len() == 64 && what.bytes().all(|b| b.is_ascii_hexdigit());
    let rec = if is_hash {
        store
            .record_by_tree(what)?
            .with_context(|| format!("manifest 里没有 tree 为 {what} 的记录，请改用源文件路径"))?
    } else {
        find_one(store, what)?
    };
    let bytes = store.reader().reconstruct(&rec)?;
    std::io::stdout().lock().write_all(&bytes)?;
    Ok(())
}

fn events(store: &Store, needle: &str) -> Result<()> {
    let rec = find_one(store, needle)?;
    let provider = normalize::by_name(&rec.provider).with_context(|| format!("没有 {} 的 normalizer", rec.provider))?;
    let lines = store.reader().lines(&rec)?;
    let mut out = BufWriter::new(std::io::stdout().lock());
    normalize::normalize(provider, &rec.source, &lines, |ev| {
        serde_json::to_writer(&mut out, &ev)?;
        out.write_all(b"\n")?;
        Ok(())
    })?;
    out.flush()?;
    Ok(())
}

fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{n} B") } else { format!("{v:.1} {}", UNITS[i]) }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let data_dir = match cli.data_dir {
        Some(d) => d,
        None => home()?.join(".harl"),
    };
    let mut store = Store::open(data_dir)?;
    match &cli.cmd {
        Cmd::Capture { roots } => {
            let roots = if roots.is_empty() { normalize::default_roots(&home()?) } else { roots.clone() };
            for r in roots.iter().filter(|r| !Path::new(r).is_dir()) {
                eprintln!("warning: root 不存在: {}", r.display());
            }
            capture(&mut store, &roots)
        }
        Cmd::Ls => ls(&store),
        Cmd::Status => status(&store),
        Cmd::Check { all } => check(&store, *all),
        Cmd::Cat { what } => cat(&store, what),
        Cmd::Events { needle } => events(&store, needle),
        Cmd::Reindex => {
            let (packs, chunks) = store.reindex()?;
            println!("reindexed {packs} packs, {chunks} chunks");
            Ok(())
        }
    }
}
