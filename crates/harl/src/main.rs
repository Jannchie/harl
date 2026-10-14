//! harl CLI：capture / 查看 / 校验，以及 `serve` 起本地 web。

mod common;
mod config;
mod head;
mod i18n;
mod serve;
mod service;
mod summary;
mod sync;
mod update;

use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use rust_i18n::t;
use walkdir::WalkDir;

use harl_core::normalize;
use harl_core::store::{self, Outcome, Record, Store, Verified};

rust_i18n::i18n!("locales", fallback = "en");

#[derive(Parser)]
// 帮助文字不写在这里：全部来自 locales/cli.toml，由 localize 按当前语言套上（cli_help_localized 测试保证每项都有）
#[command(name = "harl", version)]
struct Cli {
    // 数据目录（默认 $HARL_DATA_DIR 或 ~/.harl）
    #[arg(long, global = true, env = "HARL_DATA_DIR")]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    // 把会话文件快照进 raw 存储
    Capture {
        // 扫描根目录，可重复；默认是各 provider 的标准位置
        #[arg(long = "root")]
        roots: Vec<PathBuf>,
    },
    // 列出每个源文件的最新快照
    Ls,
    // 存储总览
    Status,
    // 验证每个快照能逐字节还原（默认只查最新快照）
    Check {
        // 查 manifest 里的每一条记录，而不只是最新的
        #[arg(long)]
        all: bool,
    },
    // 还原一个快照到 stdout（参数是 tree hash，或源文件路径）
    Cat { what: String },
    // 从 packs 重建 index.db
    Reindex,
    // 把一个会话归一化成 canonical 事件流（NDJSON）。参数匹配源文件路径的任意子串
    Events { needle: String },
    // 只采集不起 UI：监听会话目录、增量 capture，配了远端就定时推上去。装在需要采集的远程机器上
    Agent {
        // 监听并 capture 的根目录，可重复；默认是各 provider 的标准位置
        #[arg(long = "root")]
        roots: Vec<PathBuf>,
    },
    // 和 config.toml 里配置的远端同步一次（推本机的数据，拉别的机器的数据）
    Sync {
        // 只推，不拉
        #[arg(long, conflicts_with = "pull_only")]
        push_only: bool,
        // 只拉，不推
        #[arg(long)]
        pull_only: bool,
    },
    // 检查并安装新版本（GitHub Releases）
    Update {
        // 只检查，不安装
        #[arg(long)]
        check: bool,
    },
    // 开机自启：登录后在后台运行 harl serve（或 agent），挂了自动拉起
    Service {
        #[command(subcommand)]
        action: ServiceCmd,
    },
    // 起本地 HTTP，在浏览器里看会话
    Serve {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 7461)]
        port: u16,
        // 启动后打开浏览器
        #[arg(long)]
        open: bool,
        // 不在后台 capture（默认启动时先 capture 一次，之后监听会话目录，有变化就增量 capture）
        #[arg(long)]
        no_watch: bool,
        // 监听并 capture 的根目录，可重复；默认是各 provider 的标准位置
        #[arg(long = "root")]
        roots: Vec<PathBuf>,
    },
}

#[derive(Subcommand)]
enum ServiceCmd {
    // 安装并立即启动
    Install {
        // 后台跑 harl agent（只采集、不起 UI），而不是 harl serve
        #[arg(long)]
        agent: bool,
        // serve 的端口
        #[arg(long, conflicts_with = "agent")]
        port: Option<u16>,
    },
    // 停止并移除开机自启
    Uninstall,
    Start,
    Stop,
    // 是否已安装、是否在运行、日志在哪
    Status,
}

/// 按当前语言套上 clap 的帮助文字（key 见 locales/cli.toml）：子命令说明是 `<路径>.about`，参数是 `<路径>.<参数 id>`，
/// 全局参数是 `cli.<参数 id>`，clap 自带的 help / version 参数和 help 子命令用 `cli.builtin.*`。
/// 在 build 之后做，这时自带参数已经加上、全局参数也已传到各子命令
fn localize(cmd: clap::Command, path: &str) -> clap::Command {
    let help_cmd = cmd.get_name() == "help" && path != "cli";
    let about = if help_cmd { t!("cli.builtin.help_cmd") } else { t!(format!("{path}.about")) };
    let usage = cmd.get_styles().get_usage();
    let template = format!(
        "{{before-help}}{{about-with-newline}}
{}{}{} {{usage}}

{{all-args}}{{after-help}}",
        usage.render(),
        t!("cli.heading.usage"),
        usage.render_reset()
    );
    cmd.about(about.into_owned())
        .help_template(template)
        .subcommand_help_heading(t!("cli.heading.commands").into_owned())
        .mut_args(|a| {
            let key = match a.get_id().as_str() {
                "help" => "cli.builtin.help".to_string(),
                "version" => "cli.builtin.version".to_string(),
                _ if help_cmd => "cli.builtin.help_cmd_arg".to_string(),
                id if a.is_global_set() => format!("cli.{id}"),
                id => format!("{path}.{id}"),
            };
            let heading = if a.is_positional() { t!("cli.heading.arguments") } else { t!("cli.heading.options") };
            a.help(t!(&key).into_owned()).help_heading(heading.into_owned())
        })
        .mut_subcommands(|s| {
            // help 子命令底下是一份镜像的子命令树（`harl help service install`），用真实命令的 key
            let base = if help_cmd { path.strip_suffix(".help").unwrap_or(path) } else { path };
            let sub_path = format!("{base}.{}", s.get_name());
            localize(s, &sub_path)
        })
}

/// 已经按当前语言换好帮助文字的 clap 命令
fn command() -> clap::Command {
    let mut cmd = Cli::command();
    cmd.build();
    localize(cmd, "cli")
}

/// 非致命的问题统一这样打到 stderr：前缀固定英文，正文按当前语言
fn warn(msg: impl std::fmt::Display) {
    eprintln!("warning: {msg}");
}

fn home() -> Result<PathBuf> {
    dirs::home_dir().context(t!("main.no_home"))
}

/// 扫到的一个会话文件：source key 和目录遍历时顺手拿到的 size/mtime（Windows 上不用再 stat 一次）
struct Found {
    source: String,
    size: u64,
    mtime: DateTime<Utc>,
}

/// 递归找出所有会话文件（.jsonl 和 .jsonl.zstd，包含子 agent 目录），按 source 去重：同一个文件不管从哪个 root、用什么写法找到都只算一个
fn discover(roots: &[PathBuf]) -> Vec<Found> {
    let mut files: Vec<Found> = roots
        .iter()
        .flat_map(|root| WalkDir::new(root).into_iter().filter_map(Result::ok))
        .filter(|e| {
            e.file_type().is_file() && e.file_name().to_str().is_some_and(store::is_session_file)
        })
        .filter_map(|e| {
            let source = store::source_key(e.path());
            match e.metadata() {
                Ok(m) => {
                    let mtime = m
                        .modified()
                        .map(DateTime::<Utc>::from)
                        .unwrap_or_else(|_| Utc::now());
                    Some(Found {
                        source,
                        size: m.len(),
                        mtime,
                    })
                }
                Err(err) => {
                    warn(t!("main.skip", source = source, error = err));
                    None
                }
            }
        })
        .collect();
    files.sort_by(|a, b| a.source.cmp(&b.source));
    files.dedup_by(|a, b| a.source == b.source);
    files
}

/// 一次 capture 的结果
struct Captured {
    discovered: usize,
    new: usize,
    changed: usize,
    unchanged: usize,
    unknown: usize,
    failed: usize,
    done: store::Committed,
}

impl Captured {
    /// 这次有没有存进新快照
    fn wrote(&self) -> bool {
        self.new + self.changed > 0
    }
}

impl std::fmt::Display for Captured {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "discovered {}  new {}  changed {}  unchanged {}  unknown {}  failed {}  |  {} new chunks, {} raw → pack {}",
            self.discovered,
            self.new,
            self.changed,
            self.unchanged,
            self.unknown,
            self.failed,
            self.done.new_chunks,
            human_bytes(self.done.stored_bytes),
            self.done
                .pack
                .as_ref()
                .map(|(name, bytes)| format!(
                    "{name} ({} of {} logical)",
                    human_bytes(*bytes),
                    human_bytes(self.done.logical)
                ))
                .unwrap_or_else(|| "-".into())
        )
    }
}

/// roots 为空时用各 provider 的标准位置
fn capture_roots(roots: &[PathBuf]) -> Result<Vec<PathBuf>> {
    Ok(if roots.is_empty() {
        normalize::default_roots(&home()?)
    } else {
        roots.to_vec()
    })
}

/// 把 discover 出来的文件存进 store。目录遍历放在外面做，`serve` 定时 capture 时不用拿着 Store 的锁扫盘
fn capture(store: &mut Store, files: &[Found]) -> Result<Captured> {
    let (mut new, mut changed, mut unchanged, mut unknown, mut failed) =
        (0usize, 0usize, 0usize, 0usize, 0usize);
    let mut batch = store.batch()?;

    for f in files {
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
        let bytes = match store::read_source(&f.source) {
            Ok(b) => b,
            Err(e) => {
                warn(t!("main.skip", source = f.source, error = e));
                failed += 1;
                continue;
            }
        };
        match batch.put_snapshot(provider.name, &f.source, f.mtime, f.size, &bytes)? {
            Outcome::New => new += 1,
            Outcome::Changed => changed += 1,
            Outcome::Unchanged => unchanged += 1,
        }
    }

    let done = batch.commit()?;
    Ok(Captured {
        discovered: files.len(),
        new,
        changed,
        unchanged,
        unknown,
        failed,
        done,
    })
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
    let machines = store.machines()?;
    let host = machines
        .get(store.machine())
        .map(|m| m.hostname.as_str())
        .unwrap_or("?");
    println!("machine    {}  ({host})", store.machine());
    if machines.len() > 1 {
        println!(
            "machines   {}",
            machines
                .values()
                .map(|m| m.hostname.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    println!("sources    {}   ({n_records} records)", latest.len());
    println!("logical    {}   (latest snapshots)", human_bytes(logical));
    println!(
        "packs      {}   {}   {} chunks",
        s.packs,
        human_bytes(s.pack_bytes),
        s.chunks
    );
    println!("index.db   {}", human_bytes(s.index_bytes));
    if logical > 0 {
        println!(
            "ratio      {:.1}%  (packs / logical)",
            s.pack_bytes as f64 * 100.0 / logical as f64
        );
    }
    Ok(())
}

fn check(store: &Store, all: bool) -> Result<()> {
    let records: Vec<Record> = if all {
        store.records()?
    } else {
        store.latest()?.into_values().collect()
    };
    let mut reader = store.reader();
    let (mut ok, mut bad) = (0usize, 0usize);
    for rec in &records {
        match reader.verify(rec) {
            Ok(Verified::Ok) => ok += 1,
            Ok(Verified::Mismatch { got_len }) => {
                bad += 1;
                eprintln!(
                    "MISMATCH {}  got {got_len} bytes, want {}",
                    rec.source, rec.size
                );
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
    let hits: Vec<&Record> = latest
        .values()
        .filter(|r| r.source.contains(needle))
        .collect();
    match hits.as_slice() {
        [one] => Ok((*one).clone()),
        [] => bail!(t!("main.no_match", needle = needle : {:?})),
        many => bail!(
            "{}\n{}",
            t!(
                "main.many_matches",
                needle = needle : {:?},
                n = many.len()
            ),
            many.iter()
                .map(|r| format!("  {}", r.source))
                .collect::<Vec<_>>()
                .join("\n")
        ),
    }
}

fn cat(store: &Store, what: &str) -> Result<()> {
    // 64 位 hex 当 tree hash；tree 的编码由记录版本决定，所以也得先找到引用它的记录（哪台机器的都行）
    let is_hash = what.len() == 64 && what.bytes().all(|b| b.is_ascii_hexdigit());
    let rec = if is_hash {
        store
            .record_by_tree(what)?
            .with_context(|| t!("main.no_tree_record", tree = what))?
    } else {
        find_one(store, what)?
    };
    let bytes = store.reader().reconstruct(&rec)?;
    std::io::stdout().lock().write_all(&bytes)?;
    Ok(())
}

fn events(store: &Store, needle: &str) -> Result<()> {
    let rec = find_one(store, needle)?;
    let provider = normalize::by_name(&rec.provider)
        .with_context(|| t!("main.no_normalizer", provider = rec.provider))?;
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
    if i == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

fn main() -> Result<()> {
    i18n::init();
    let cli = Cli::from_arg_matches(&command().get_matches()).unwrap_or_else(|e| e.exit());
    update::cleanup();
    if let Cmd::Update { check } = &cli.cmd {
        return update::cli(*check);
    }
    let data_dir = match &cli.data_dir {
        Some(d) => d.clone(),
        None => home()?.join(".harl"),
    };
    if let Cmd::Service { action } = &cli.cmd {
        return match action {
            ServiceCmd::Install { agent, port } => {
                let mut args = vec![if *agent { "agent" } else { "serve" }.to_string()];
                if let Some(p) = port {
                    args.extend(["--port".into(), p.to_string()]);
                }
                // 显式指定的数据目录（参数或环境变量）要带给后台进程
                if let Some(d) = &cli.data_dir {
                    let d = std::path::absolute(d)?;
                    args.extend(["--data-dir".into(), d.to_string_lossy().into_owned()]);
                }
                service::install(&service::Spec { args, data_dir })
            }
            ServiceCmd::Uninstall => service::uninstall(&data_dir),
            ServiceCmd::Start => service::start(&data_dir),
            ServiceCmd::Stop => service::stop(&data_dir),
            ServiceCmd::Status => service::status(&data_dir),
        };
    }
    let config = config::load(&data_dir)?;
    let mut store = Store::open(data_dir.clone())?;
    match &cli.cmd {
        Cmd::Capture { roots } => {
            let roots = capture_roots(roots)?;
            for r in roots.iter().filter(|r| !Path::new(r).is_dir()) {
                warn(t!("main.root_missing", path = r.display()));
            }
            println!("{}", capture(&mut store, &discover(&roots))?);
            Ok(())
        }
        Cmd::Ls => ls(&store),
        Cmd::Status => status(&store),
        Cmd::Check { all } => check(&store, *all),
        Cmd::Cat { what } => cat(&store, what),
        Cmd::Events { needle } => events(&store, needle),
        Cmd::Serve {
            host,
            port,
            open,
            no_watch,
            roots,
        } => {
            let roots = if *no_watch {
                None
            } else {
                Some(capture_roots(roots)?)
            };
            let remote = config.remote.as_ref().map(sync::Remote::new).transpose()?;
            let http = Some(serve::Http {
                host: host.clone(),
                port: *port,
                open: *open,
            });
            serve::run(
                store,
                serve::Options {
                    http,
                    watch_roots: roots,
                    remote,
                    update: config.update.clone(),
                },
            )
        }
        Cmd::Agent { roots } => {
            let remote = config.remote.as_ref().map(sync::Remote::new).transpose()?;
            if remote.is_none() {
                eprintln!(
                    "note: {}",
                    t!("main.no_remote", path = config::path(&data_dir).display())
                );
            }
            println!("harl agent  data {}", data_dir.display());
            serve::run(
                store,
                serve::Options {
                    http: None,
                    watch_roots: Some(capture_roots(roots)?),
                    remote,
                    update: config.update.clone(),
                },
            )
        }
        Cmd::Sync {
            push_only,
            pull_only,
        } => {
            let remote = sync::require(config.remote.as_ref(), &config::path(&data_dir))?;
            let store = std::sync::Mutex::new(store);
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            let synced = rt.block_on(async {
                let pushed = if *pull_only {
                    Default::default()
                } else {
                    remote.push(&store).await?
                };
                let pulled = if *push_only {
                    None
                } else {
                    Some(remote.pull(&store).await?)
                };
                anyhow::Ok(sync::Synced { pushed, pulled })
            })?;
            println!("{synced}");
            Ok(())
        }
        Cmd::Service { .. } | Cmd::Update { .. } => unreachable!("handled above"),
        Cmd::Reindex => {
            let (packs, chunks) = store.reindex()?;
            println!("reindexed {packs} packs, {chunks} chunks");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 拿不到译文时 t! 原样返回 key，所以帮助里不该出现 `cli.` 开头的文字
    fn assert_localized(cmd: &clap::Command, locale: &str) {
        let about = cmd.get_about().map(|s| s.to_string()).unwrap_or_default();
        assert!(
            !about.is_empty() && !about.contains("cli."),
            "{locale}: {} 缺 about：{about:?}",
            cmd.get_name()
        );
        for a in cmd.get_arguments() {
            let help = a.get_help().map(|s| s.to_string()).unwrap_or_default();
            assert!(
                !help.is_empty() && !help.contains("cli."),
                "{locale}: {} 的 {} 缺 help：{help:?}",
                cmd.get_name(),
                a.get_id()
            );
        }
        for s in cmd.get_subcommands() {
            assert_localized(s, locale);
        }
    }

    #[test]
    fn cli_help_localized() {
        for locale in i18n::LOCALES {
            rust_i18n::set_locale(locale);
            assert_localized(&command(), locale);
        }
    }
}
