//! `/api/summary`：把所有最新快照的会话统计聚合成仪表盘要的形状。
//! 每个快照的统计按 tree 缓存在 index.db 里，第一次算完以后只有新快照要算。
//!
//! 时间范围过滤在 turn 这一级做：所有聚合（totals、by_model、by_tool、按天、按会话）都只从落在
//! `[from, to]` 里的 turn 推导，这样各个数字彼此一致。不属于任何 turn 的零星事件不计。
//!
//! provider、项目过滤按会话；模型过滤也在 turn 这一级：只留用过所选模型的 turn，其中 tokens / 步数 / 模型时间
//! 只算所选模型的部分，工具和墙钟时间拆不到模型上，按整个 turn 算。
//! 上下文组成按所选模型的 prompt 占比等比取一部分；skill、程序和工具一样按整个 turn 算。

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::{Context, Result};
use chrono::{DateTime, Datelike, FixedOffset, NaiveDate, TimeZone, Timelike, Utc};
use rust_i18n::t;
use serde::Serialize;

use harl_core::composition::Mix;
use harl_core::event::ToolCategory;
use harl_core::normalize;
use harl_core::resume::{self, Checkpoint};
use harl_core::stats::{self, ByTag, ModelAgg, ReviewAgg, SessionStats, Signals, Tokens, ToolAgg, TurnStat, UseTable};
use harl_core::store::{Reader, Record, Store};

use crate::head;

#[derive(Serialize, Default)]
pub struct Totals {
    pub sessions: u32,
    pub turns: u32,
    pub steps: u32,
    pub tool_calls: u32,
    pub tool_failures: u32,
    pub tokens: Tokens,
    pub model_ms: u64,
    pub tool_ms: u64,
    pub wall_ms: u64,
    pub first_ts: Option<DateTime<Utc>>,
    pub last_ts: Option<DateTime<Utc>>,
    /// auto 模式分类器的审查次数、拦截、没给结论，以及拦截原因
    pub review: ReviewAgg,
    /// 插话、打断、上下文压缩的次数
    pub signals: Signals,
    /// turn 边界上 hook 的执行时间（Claude Code 的 stop hook）
    pub hook_ms: u64,
}

#[derive(Serialize, Default)]
pub struct ProviderAgg {
    pub sessions: u32,
    pub turns: u32,
    pub tokens: Tokens,
    pub model_ms: u64,
    pub tool_ms: u64,
    pub wall_ms: u64,
    pub by_model: BTreeMap<String, ModelAgg>,
}

/// 一天（按客户端时区）
#[derive(Serialize, Default)]
pub struct Day {
    pub date: String,
    pub turns: u32,
    pub steps: u32,
    pub tool_calls: u32,
    pub tokens: Tokens,
    pub model_ms: u64,
    pub tool_ms: u64,
    pub wall_ms: u64,
    pub by_model: BTreeMap<String, ModelAgg>,
    /// provider → 模型 → 聚合：web 端按天定价时 provider 的费用也从这里累加，和总费用同一个口径
    pub by_provider: BTreeMap<String, BTreeMap<String, ModelAgg>>,
    /// 推理强度 → 步数：看强度的占比怎么随时间变
    pub by_effort: BTreeMap<String, u32>,
    /// 工具名 → 调用次数
    pub tools: BTreeMap<String, u32>,
    /// skill → 加载次数
    pub skills: BTreeMap<String, u32>,
    /// shell 程序 → 用到它的调用次数
    pub programs: BTreeMap<String, u32>,
    /// prompt 按上下文组成拆开（见 stats::TurnStat::composition）
    pub composition: Mix,
}

/// 一个会话落在范围里的 turn 之和。web 端拿它做会话排行，按 cwd 归成项目
#[derive(Serialize, Default)]
pub struct SessionRow {
    /// 快照 tree hash，会话页的路由参数
    pub id: String,
    pub provider: String,
    pub title: Option<String>,
    /// 工作目录（`/` 分隔），即项目；拿不到就 None
    pub cwd: Option<String>,
    pub parent_session: Option<String>,
    pub turns: u32,
    pub steps: u32,
    pub tool_calls: u32,
    pub tool_failures: u32,
    pub tokens: Tokens,
    pub model_ms: u64,
    pub tool_ms: u64,
    pub wall_ms: u64,
    pub first_ts: Option<DateTime<Utc>>,
    pub last_ts: Option<DateTime<Utc>>,
    /// web 端按模型定价
    pub by_model: BTreeMap<String, ModelAgg>,
    /// 单步最大的 prompt（新输入 + 缓存读写），即上下文最满的时候
    pub peak_ctx: u64,
    pub compactions: u32,
    /// 最后一条人打的消息（单行、最多 200 字符）；整个会话的，不受范围影响
    pub last_user: Option<String>,
    /// 会话末尾的上下文 tokens（最后一次请求的实际 prompt + 之后新增的估算）；不受范围影响
    pub ctx: u64,
    /// 会话末尾上下文的组成，合计是 ctx
    pub ctx_parts: Mix,
    /// ctx 全是估算（没有一步报过 prompt）
    pub ctx_estimated: bool,
    /// 范围内各步 prompt 按上下文组成拆开之和
    pub composition: Mix,
    /// 范围内：工具名 → 调用次数、skill → 加载次数、shell 程序 → 调用次数（点排行项过滤会话用）
    pub tools: BTreeMap<String, u32>,
    pub skills: BTreeMap<String, u32>,
    pub programs: BTreeMap<String, u32>,
}

/// 一个 (星期, 小时) 格子；只输出有 turn 的格子
#[derive(Serialize, Default)]
pub struct HourCell {
    /// 0 = 周一
    pub weekday: u8,
    pub hour: u8,
    pub turns: u32,
    /// turn 的模型 / 工具 / 墙钟时间整块记在 turn 开始的那个小时
    pub model_ms: u64,
    pub tool_ms: u64,
    pub wall_ms: u64,
    pub by_model: BTreeMap<String, Tokens>,
}

/// turn 墙钟时间分布的分桶上界（不含）：10 s、30 s、1、2、5、10、30、60 min；最后一桶没有上界
const TURN_EDGES_MS: [u64; 8] = [10_000, 30_000, 60_000, 120_000, 300_000, 600_000, 1_800_000, 3_600_000];
/// 最慢 turn 列表的长度
const SLOWEST: usize = 10;
/// 并行取 / 算会话统计的线程数上限
const PARSE_THREADS: usize = 8;

/// 范围内 turn 的墙钟时间分布
#[derive(Serialize, Default)]
pub struct TurnDurations {
    pub edges_ms: Vec<u64>,
    /// 每桶的 turn 数，比 edges_ms 多一桶
    pub counts: Vec<u32>,
    pub p50_ms: u64,
    pub p90_ms: u64,
    pub max_ms: u64,
    /// 最慢的几个 turn，从慢到快
    pub slowest: Vec<SlowTurn>,
}

#[derive(Serialize, Clone)]
pub struct SlowTurn {
    /// 会话的快照 tree hash（会话页路由参数）
    pub session_id: String,
    pub provider: String,
    pub title: Option<String>,
    /// turn id，会话页的锚点是 `turn-t:<id>`
    pub turn: String,
    pub ts: DateTime<Utc>,
    pub wall_ms: u64,
    /// 模型时间 + 工具时间：墙钟里真正在干活的部分，剩下的是等人、闲置
    pub active_ms: u64,
    pub steps: u32,
    pub tool_calls: u32,
    /// web 端按模型定价
    pub by_model: BTreeMap<String, ModelAgg>,
}

impl TurnDurations {
    fn build(mut ms: Vec<u64>, slowest: Vec<SlowTurn>) -> Self {
        ms.sort_unstable();
        let mut counts = vec![0; TURN_EDGES_MS.len() + 1];
        for &v in &ms {
            counts[TURN_EDGES_MS.partition_point(|&e| e <= v)] += 1;
        }
        // 最近秩：第 ⌈p·n⌉ 个
        let pct = |p: f64| ms.get(((p * ms.len() as f64).ceil() as usize).max(1) - 1).copied().unwrap_or(0);
        Self {
            edges_ms: TURN_EDGES_MS.to_vec(),
            counts,
            p50_ms: pct(0.5),
            p90_ms: pct(0.9),
            max_ms: ms.last().copied().unwrap_or(0),
            slowest,
        }
    }
}

/// 请求的时间范围：本地日期，两端都含；since 是滚动窗口（"最近 24 小时"）的精确起点，和 from 同时给时取更晚的
#[derive(Serialize, Clone, Copy, Default)]
pub struct Range {
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub since: Option<DateTime<Utc>>,
}

/// 按 provider / 模型 / 项目过滤；空集合表示不限
#[derive(Serialize, Clone, Default)]
pub struct Filter {
    pub providers: BTreeSet<String>,
    pub models: BTreeSet<String>,
    /// 项目键（见 project_key）；拿不到 cwd 的会话记作空串
    pub projects: BTreeSet<String>,
}

/// 能选的 provider、模型和项目：全部数据里出现过的，不受过滤和时间范围影响
#[derive(Serialize, Default)]
pub struct Options {
    pub providers: BTreeSet<String>,
    pub models: BTreeSet<String>,
    pub projects: BTreeSet<String>,
}

#[derive(Serialize)]
pub struct Summary {
    /// 分桶用的时区偏移（分钟，东为正）
    pub tz_offset_min: i32,
    pub range: Range,
    pub filter: Filter,
    pub options: Options,
    /// 全部数据的时间跨度（不受 range 影响），给范围选择器定边界
    pub available: (Option<DateTime<Utc>>, Option<DateTime<Utc>>),
    pub totals: Totals,
    pub by_provider: BTreeMap<String, ProviderAgg>,
    pub by_model: BTreeMap<String, ModelAgg>,
    /// 工具名 → 统计（每项带 category）
    pub by_tool: BTreeMap<String, ToolAgg>,
    /// 跨 harness 的工具类别：shell / read / edit / search / web / agent / user / mcp / meta / other
    pub by_category: BTreeMap<ToolCategory, ToolAgg>,
    /// 推理强度 → 模型 → 聚合；只含带强度的步（Claude Code、Codex）
    pub by_effort: ByTag,
    /// skill → 模型 → 聚合；只含归属了 skill 的步（Claude Code）
    pub by_skill: ByTag,
    /// skill 加载：次数、最后一次、会话数、路径与来源
    pub skills: UseTable,
    /// shell 命令用到的程序：次数、失败、最后一次、会话数
    pub programs: UseTable,
    /// 范围内全部 prompt 按上下文组成拆开（billed 输入的组成）
    pub composition: Mix,
    pub daily: Vec<Day>,
    /// 本地时区的 星期 × 小时 分桶，稀疏
    pub hourly: Vec<HourCell>,
    /// 范围内有 turn 的会话，按最后活动时间倒序
    pub sessions: Vec<SessionRow>,
    pub turn_durations: TurnDurations,
}

/// 进程内留着最近改过的几个源文件读完时的状态（见 harl_core::resume），按 (machine, source) 记。
/// 活跃会话每次 capture 换一个 tree，新快照只是追加时从这里接着算，不用把几十 MB 的 JSONL 从头归一化。
/// 每个检查点带着一份会话统计的中间状态，只留 mtime 最新的 CHECKPOINTS 个：正在写的会话总是最新的那几个
const CHECKPOINTS: usize = 32;
type Checkpoints = HashMap<(String, String), (DateTime<Utc>, Checkpoint)>;
static RESUME: Mutex<Option<Checkpoints>> = Mutex::new(None);

fn resume_memo() -> MutexGuard<'static, Option<Checkpoints>> {
    RESUME.lock().unwrap_or_else(|p| p.into_inner())
}

/// 取或算一个快照的统计
pub fn stats_for(store: &Store, rec: &Record) -> Result<SessionStats> {
    if let Some(s) = store.cached::<SessionStats>("stats", stats::STATS_VERSION, &rec.tree)? {
        return Ok(s);
    }
    let provider =
        normalize::by_name(&rec.provider).with_context(|| t!("main.no_normalizer", provider = rec.provider))?;
    let key = (rec.machine.clone(), rec.source.clone());
    // 拿出来算，算完放回去；两个线程同时算同一个源文件时，后到的那个从头算，结果一样
    let prev = resume_memo().as_mut().and_then(|m| m.remove(&key)).map(|(_, cp)| cp);
    let (s, cp) = resume::stats(&mut store.reader(), provider, rec, prev)?;
    if let Some(cp) = cp {
        let mut memo = resume_memo();
        let m = memo.get_or_insert_default();
        // 别的线程可能已经放进了更新的
        if m.get(&key).is_none_or(|(t, _)| *t <= rec.mtime) {
            m.insert(key, (rec.mtime, cp));
        }
        while m.len() > CHECKPOINTS {
            let oldest = m.iter().min_by_key(|(_, (t, _))| *t).map(|(k, _)| k.clone()).unwrap();
            m.remove(&oldest);
        }
    }
    store.put_cached("stats", stats::STATS_VERSION, &rec.tree, &s)?;
    Ok(s)
}

/// 进程内的 (stats, head)，按 (tree, source) 记：tree 不可变，head 里有一部分从路径推出来，所以带上 source。
/// index.db 里的缓存是 JSON，几千个会话每次请求都反序列化一遍要一秒多；这里留着解析好的，每次 build 后只留还活着的快照
type Parsed = (Arc<SessionStats>, Arc<head::Head>);
static PARSED: Mutex<Option<HashMap<(String, String), Parsed>>> = Mutex::new(None);

fn parse(store: &Store, reader: &mut Reader<'_>, rec: &Record) -> Result<Parsed> {
    let s = stats_for(store, rec)?;
    let head = normalize::by_name(&rec.provider).and_then(|p| head::head_for(store, reader, p, rec)).unwrap_or_default();
    Ok((Arc::new(s), Arc::new(head)))
}

/// 后台预热：算好（或从 index.db 读出）这些快照的 stats 和 head，并放进进程内缓存，第一次打开仪表盘也不用等。
/// 并行（`parse_many`）；已经在进程内缓存里的、没有 normalizer 的跳过。一个快照失败不影响别的，失败的连同错误返回
pub fn prime_all(store: &Store, recs: &[Record]) -> Result<Vec<(String, anyhow::Error)>> {
    let todo: Vec<&Record> = {
        let memo = PARSED.lock().unwrap_or_else(|p| p.into_inner());
        let mut seen = std::collections::HashSet::new();
        recs.iter()
            .filter(|r| normalize::by_name(&r.provider).is_some())
            .filter(|r| memo.as_ref().is_none_or(|m| !m.contains_key(&(r.tree.clone(), r.source.clone()))))
            .filter(|r| seen.insert((r.tree.as_str(), r.source.as_str())))
            .collect()
    };
    let done = parse_many(store, todo, false)?;
    let mut failed = Vec::new();
    let mut memo = PARSED.lock().unwrap_or_else(|p| p.into_inner());
    let m = memo.get_or_insert_default();
    for (r, v) in done {
        match v {
            Ok(v) => {
                m.insert((r.tree.clone(), r.source.clone()), v);
            }
            Err(e) => failed.push((r.source.clone(), e)),
        }
    }
    Ok(failed)
}

/// 并行取或算这些快照的 (stats, head)，第一个出错就整体失败
fn parse_all(store: &Store, todo: Vec<&Record>) -> Result<HashMap<(String, String), Parsed>> {
    parse_many(store, todo, true)?
        .into_iter()
        .map(|(r, v)| Ok(((r.tree.clone(), r.source.clone()), v?)))
        .collect()
}

/// 并行取或算这些快照的 (stats, head)，每个快照各自的结果。冷启动时几千个会话每个都要查一次 index.db、反序列化一份 JSON，
/// 新快照要读 pack、归一化，单线程要好几秒。每个线程一条自己的 SQLite 连接（`reopen`）；大的先算，免得最后剩一个大文件拖着。
/// `stop_on_error` 时第一个出错后其余线程不再领新活，结果里只有已经做完的
fn parse_many<'r>(store: &Store, mut todo: Vec<&'r Record>, stop_on_error: bool) -> Result<Vec<(&'r Record, Result<Parsed>)>> {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(PARSE_THREADS).min(todo.len());
    if threads <= 1 {
        let mut reader = store.reader();
        let mut out = Vec::with_capacity(todo.len());
        for r in todo {
            let v = parse(store, &mut reader, r);
            let stop = stop_on_error && v.is_err();
            out.push((r, v));
            if stop {
                break;
            }
        }
        return Ok(out);
    }
    todo.sort_by_key(|r| std::cmp::Reverse(r.size));
    let next = AtomicUsize::new(0);
    let failed = AtomicBool::new(false);
    // Store 不是 Sync（SQLite 连接），每个线程拿走自己的句柄
    let stores = (0..threads).map(|_| store.reopen()).collect::<Result<Vec<_>>>()?;
    let (next, failed, todo) = (&next, &failed, &todo);
    let results: Vec<Vec<_>> = std::thread::scope(|scope| {
        let workers: Vec<_> = stores
            .into_iter()
            .map(|store| {
                scope.spawn(move || {
                    let mut reader = store.reader();
                    let mut out = Vec::new();
                    while !failed.load(Ordering::Relaxed) {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(&r) = todo.get(i) else { break };
                        let v = parse(&store, &mut reader, r);
                        if stop_on_error && v.is_err() {
                            failed.store(true, Ordering::Relaxed);
                        }
                        out.push((r, v));
                    }
                    out
                })
            })
            .collect();
        workers.into_iter().map(|w| w.join().unwrap_or_else(|p| std::panic::resume_unwind(p))).collect()
    });
    Ok(results.into_iter().flatten().collect())
}

/// index.db 里的派生缓存：kind 和本版本的口径版本。`Store::gc_derived` 只留这些版本
pub const DERIVED: [(&str, u32); 2] = [("stats", stats::STATS_VERSION), ("head", head::HEAD_VERSION)];

/// serve 里两次派生缓存 GC 之间至少隔这么久
const GC_EVERY: std::time::Duration = std::time::Duration::from_secs(6 * 3600);
/// 上一次 GC 的时间；None 是这个进程还没做过
static LAST_GC: Mutex<Option<std::time::Instant>> = Mutex::new(None);

/// serve 预热完顺手清过期的派生缓存（`Store::gc_derived`），到时间了才做，返回 None 表示这次没做。
/// 第一次要等启动时那一轮预热做完——全部最新快照都在 `recs` 里或已经在进程内缓存里——免得 GC 抢在预热前头拖慢首屏；
/// 之后每 GC_EVERY 一次，清掉期间被新快照取代的那些
pub fn gc_if_due(store: &Store, recs: &[Record]) -> Result<Option<harl_core::store::DerivedGc>> {
    let mut last = LAST_GC.lock().unwrap_or_else(|p| p.into_inner());
    match *last {
        Some(t) if t.elapsed() < GC_EVERY => return Ok(None),
        Some(_) => {}
        None => {
            let live = normalize::live(store.manifest()?.latest.values().cloned());
            let memo = PARSED.lock().unwrap_or_else(|p| p.into_inner());
            let warmed: std::collections::HashSet<(&str, &str)> = recs.iter().map(|r| (r.tree.as_str(), r.source.as_str())).collect();
            let done = live.iter().all(|r| {
                normalize::by_name(&r.provider).is_none()
                    || warmed.contains(&(r.tree.as_str(), r.source.as_str()))
                    || memo.as_ref().is_some_and(|m| m.contains_key(&(r.tree.clone(), r.source.clone())))
            });
            if !done {
                return Ok(None);
            }
        }
    }
    *last = Some(std::time::Instant::now());
    drop(last);
    Ok(Some(store.gc_derived(&DERIVED)?))
}

pub fn build(store: &Store, tz_offset_min: i32, range: Range, filter: Filter) -> Result<Summary> {
    let tz = FixedOffset::east_opt(tz_offset_min * 60).unwrap_or(FixedOffset::east_opt(0).unwrap());
    // 本地日期 → UTC 边界；to 是含的那天，所以取次日零点
    let bound = |d: NaiveDate| tz.from_local_datetime(&d.and_hms_opt(0, 0, 0).unwrap()).single().map(|t| t.to_utc());
    let from_utc = range.from.and_then(bound).max(range.since);
    let to_utc = range.to.and_then(|d| bound(d.succ_opt().unwrap_or(d)));
    let in_range = |ts: DateTime<Utc>| from_utc.is_none_or(|f| ts >= f) && to_utc.is_none_or(|t| ts < t);

    let mut totals = Totals::default();
    let mut available = (None, None);
    let mut by_provider: BTreeMap<String, ProviderAgg> = BTreeMap::new();
    let mut by_model: BTreeMap<String, ModelAgg> = BTreeMap::new();
    let mut by_tool: BTreeMap<String, ToolAgg> = BTreeMap::new();
    let mut by_category: BTreeMap<ToolCategory, ToolAgg> = BTreeMap::new();
    let mut by_effort = ByTag::new();
    let mut by_skill = ByTag::new();
    let mut skills = UseTable::new();
    let mut programs = UseTable::new();
    let mut composition = Mix::default();
    // 本地日期 → Day；NaiveDate 的顺序和 YYYY-MM-DD 字符串一致，输出时才格式化
    let mut days: BTreeMap<NaiveDate, Day> = BTreeMap::new();
    let mut hourly: BTreeMap<(u8, u8), HourCell> = BTreeMap::new();
    let mut options = Options::default();
    let mut sessions: Vec<SessionRow> = Vec::new();
    let mut turn_ms: Vec<u64> = Vec::new();
    // 保持从慢到快，最多 SLOWEST 个
    let mut slowest: Vec<SlowTurn> = Vec::new();
    let mut memo = PARSED.lock().unwrap_or_else(|p| p.into_inner());
    let mut prev = memo.take().unwrap_or_default();
    let recs = normalize::live(store.latest()?.into_values());
    let todo: Vec<&Record> = recs.iter().filter(|r| !prev.contains_key(&(r.tree.clone(), r.source.clone()))).collect();
    prev.extend(parse_all(store, todo)?);
    let mut parsed = HashMap::with_capacity(recs.len());

    for rec in recs {
        let key = (rec.tree.clone(), rec.source.clone());
        // 两台机器上同一路径、同一内容的快照 key 相同，第二个从 parsed 里拿
        let (s, head) = match prev.remove(&key).or_else(|| parsed.get(&key).cloned()) {
            Some(v) => v,
            None => parse(store, &mut store.reader(), &rec)?,
        };
        parsed.insert(key, (s.clone(), head.clone()));
        let project = head.cwd.clone().unwrap_or_default();
        options.projects.insert(project.clone());
        available.0 = min_opt(available.0, s.first_ts);
        available.1 = max_opt(available.1, s.last_ts);
        options.providers.insert(rec.provider.clone());
        options.models.extend(s.by_model.keys().cloned());
        if !filter.providers.is_empty() && !filter.providers.contains(&rec.provider) {
            continue;
        }
        if !filter.projects.is_empty() && !filter.projects.contains(&project_key(&project)) {
            continue;
        }

        let mut row = SessionRow {
            id: rec.tree.clone(),
            provider: rec.provider.clone(),
            title: head.title.clone(),
            cwd: head.cwd.clone(),
            parent_session: head.parent.clone(),
            last_user: s.last_user.clone(),
            ctx: s.ctx.tokens,
            ctx_parts: s.ctx.parts,
            ctx_estimated: s.ctx.estimated,
            ..Default::default()
        };
        // skill 的来源要用会话的 cwd 判断，先在会话内攒着
        let mut row_skills = UseTable::new();
        let provider = rec.provider.clone();
        let p = by_provider.entry(rec.provider).or_default();

        for t in s.turn_stats.iter().filter(|t| t.ts.is_some_and(in_range)) {
            let Some(t) = filter.pick(t) else { continue };
            let t = &*t;
            let ts = t.ts.unwrap();
            turn_ms.push(t.wall_ms);
            if slowest.len() < SLOWEST || slowest.last().is_some_and(|s| t.wall_ms > s.wall_ms) {
                let at = slowest.partition_point(|s| s.wall_ms >= t.wall_ms);
                slowest.insert(
                    at,
                    SlowTurn {
                        session_id: rec.tree.clone(),
                        provider: provider.clone(),
                        title: row.title.clone(),
                        turn: t.id.clone(),
                        ts,
                        wall_ms: t.wall_ms,
                        active_ms: t.model_ms + t.tool_ms,
                        steps: t.steps,
                        tool_calls: t.tool_calls,
                        by_model: t.by_model.clone(),
                    },
                );
                slowest.truncate(SLOWEST);
            }
            row.turns += 1;
            row.steps += t.steps;
            row.tool_calls += t.tool_calls;
            row.tokens.add(&t.tokens);
            row.model_ms += t.model_ms;
            row.tool_ms += t.tool_ms;
            row.wall_ms += t.wall_ms;
            row.first_ts = min_opt(row.first_ts, Some(ts));
            row.last_ts = max_opt(row.last_ts, Some(ts));
            row.tool_failures += t.tool_failures;
            row.peak_ctx = row.peak_ctx.max(t.peak_ctx);
            row.compactions += t.signals.compactions;
            totals.review.add(&t.review);
            totals.signals.add(&t.signals);
            totals.hook_ms += t.hook_ms;

            let local = ts.with_timezone(&tz);
            let d = days.entry(local.date_naive()).or_default();
            d.turns += 1;
            d.steps += t.steps;
            d.tool_calls += t.tool_calls;
            d.tokens.add(&t.tokens);
            d.model_ms += t.model_ms;
            d.tool_ms += t.tool_ms;
            d.wall_ms += t.wall_ms;
            let (wd, h) = (local.weekday().num_days_from_monday() as u8, local.hour() as u8);
            let cell = hourly.entry((wd, h)).or_insert_with(|| HourCell { weekday: wd, hour: h, ..Default::default() });
            cell.turns += 1;
            cell.model_ms += t.model_ms;
            cell.tool_ms += t.tool_ms;
            cell.wall_ms += t.wall_ms;

            for (m, a) in &t.by_model {
                slot(&mut row.by_model, m).add(a);
                slot(&mut by_model, m).add(a);
                slot(&mut p.by_model, m).add(a);
                slot(&mut d.by_model, m).add(a);
                slot(slot(&mut d.by_provider, &provider), m).add(a);
                slot(&mut cell.by_model, m).add(&a.tokens);
            }
            for (name, a) in &t.by_tool {
                slot(&mut by_tool, name).add(a);
                by_category.entry(a.category).or_default().add(a);
                *slot(&mut row.tools, name) += a.calls;
                *slot(&mut d.tools, name) += a.calls;
            }
            stats::merge_uses(&mut row_skills, &t.skills);
            stats::merge_uses(&mut programs, &t.programs);
            for (name, u) in &t.skills {
                *slot(&mut row.skills, name) += u.n;
                *slot(&mut d.skills, name) += u.n;
            }
            for (name, u) in &t.programs {
                *slot(&mut row.programs, name) += u.n;
                *slot(&mut d.programs, name) += u.n;
            }
            row.composition.add(&t.composition);
            d.composition.add(&t.composition);
            composition.add(&t.composition);
            stats::merge_tags(&mut by_effort, &t.by_effort);
            for (e, models) in &t.by_effort {
                *slot(&mut d.by_effort, e) += models.values().map(|a| a.steps).sum::<u32>();
            }
            stats::merge_tags(&mut by_skill, &t.by_skill);
        }
        if row.turns == 0 {
            continue;
        }
        // 会话数：这个会话在范围内用过的每一项各记一次
        for name in row.tools.keys() {
            by_tool.entry(name.clone()).or_default().sessions += 1;
        }
        for name in row.programs.keys() {
            programs.entry(name.clone()).or_default().sessions += 1;
        }
        for (name, mut u) in row_skills {
            u.sessions = 1;
            u.source = u.path.as_deref().map(|p| skill_source(p, row.cwd.as_deref()).to_string());
            skills.entry(name).or_default().add(&u);
        }

        totals.sessions += 1;
        totals.turns += row.turns;
        totals.steps += row.steps;
        totals.tool_calls += row.tool_calls;
        totals.tool_failures += row.tool_failures;
        totals.tokens.add(&row.tokens);
        totals.model_ms += row.model_ms;
        totals.tool_ms += row.tool_ms;
        totals.wall_ms += row.wall_ms;
        totals.first_ts = min_opt(totals.first_ts, row.first_ts);
        totals.last_ts = max_opt(totals.last_ts, row.last_ts);

        p.sessions += 1;
        p.turns += row.turns;
        p.tokens.add(&row.tokens);
        p.model_ms += row.model_ms;
        p.tool_ms += row.tool_ms;
        p.wall_ms += row.wall_ms;
        sessions.push(row);
    }
    *memo = Some(parsed);
    drop(memo);
    sessions.sort_by_key(|s| std::cmp::Reverse(s.last_ts));
    by_provider.retain(|_, p| p.sessions > 0);

    let daily = days
        .into_iter()
        .map(|(date, mut d)| {
            d.date = date.format("%Y-%m-%d").to_string();
            d
        })
        .collect();
    Ok(Summary {
        tz_offset_min,
        range,
        filter,
        options,
        available,
        totals,
        by_provider,
        by_model,
        by_tool,
        by_category,
        by_effort,
        by_skill,
        skills,
        programs,
        composition,
        daily,
        hourly: hourly.into_values().collect(),
        sessions,
        turn_durations: TurnDurations::build(turn_ms, slowest),
    })
}

impl Filter {
    /// 按模型过滤一个 turn：没选模型原样返回；没用到所选模型的 turn 丢掉；
    /// 否则只留所选模型的 by_model，并据此重算 steps / tokens / model_ms
    fn pick<'a>(&self, t: &'a TurnStat) -> Option<Cow<'a, TurnStat>> {
        if self.models.is_empty() {
            return Some(Cow::Borrowed(t));
        }
        let by_model: BTreeMap<String, ModelAgg> =
            t.by_model.iter().filter(|(m, _)| self.models.contains(*m)).map(|(m, a)| (m.clone(), a.clone())).collect();
        if by_model.is_empty() {
            return None;
        }
        let mut sum = ModelAgg::default();
        for a in by_model.values() {
            sum.add(a);
        }
        let prompt = |t: &Tokens| t.input + t.cache_read + t.cache_write;
        let composition = if prompt(&t.tokens) == 0 { Mix::default() } else { t.composition.scaled_to(prompt(&sum.tokens)) };
        Some(Cow::Owned(TurnStat {
            steps: sum.steps,
            tokens: sum.tokens,
            model_ms: sum.model_ms,
            by_model,
            by_tool: t.by_tool.clone(),
            by_effort: self.pick_tags(&t.by_effort),
            by_skill: self.pick_tags(&t.by_skill),
            id: t.id.clone(),
            review: t.review.clone(),
            skills: t.skills.clone(),
            programs: t.programs.clone(),
            composition,
            ..*t
        }))
    }

    /// 标签下只留所选模型，留空的标签去掉
    fn pick_tags(&self, tags: &ByTag) -> ByTag {
        tags.iter()
            .map(|(tag, models)| {
                let kept = models.iter().filter(|(m, _)| self.models.contains(*m)).map(|(m, a)| (m.clone(), a.clone()));
                (tag.clone(), kept.collect::<BTreeMap<_, _>>())
            })
            .filter(|(_, models)| !models.is_empty())
            .collect()
    }
}

/// 项目键：和 web 侧栏的分组一致（lib/project.ts）——小写、`/` 分隔、去掉结尾斜杠，
/// 所以 Windows 上大小写不同的同一个目录算一个项目
fn project_key(cwd: &str) -> String {
    normalize::slashed(cwd).trim_end_matches('/').to_lowercase()
}

/// skill 从哪来：内置（`bundled:` 前缀）、插件目录、会话 cwd 下的项目 skill，其余算用户自己的
fn skill_source(path: &str, cwd: Option<&str>) -> &'static str {
    let p = project_key(path);
    if p.starts_with("bundled:") {
        return "bundled";
    }
    if p.contains("/plugins/") {
        return "plugin";
    }
    match cwd.map(project_key).filter(|c| !c.is_empty()) {
        Some(c) if p.strip_prefix(c.as_str()).is_some_and(|r| r.starts_with('/')) => "project",
        _ => "user",
    }
}

/// `m[k]`，没有就插入默认值。已有的键不用为 entry 再分配一个 String：聚合循环每个 turn 要查几十次
fn slot<'a, V: Default>(m: &'a mut BTreeMap<String, V>, k: &str) -> &'a mut V {
    if !m.contains_key(k) {
        m.insert(k.to_string(), V::default());
    }
    m.get_mut(k).unwrap()
}

fn min_opt(a: Option<DateTime<Utc>>, b: Option<DateTime<Utc>>) -> Option<DateTime<Utc>> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

fn max_opt(a: Option<DateTime<Utc>>, b: Option<DateTime<Utc>>) -> Option<DateTime<Utc>> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agg(steps: u32, input: u64, ms: u64) -> ModelAgg {
        ModelAgg { steps, tokens: Tokens { input, ..Default::default() }, model_ms: ms }
    }

    fn turn() -> TurnStat {
        TurnStat {
            ts: Some(Utc::now()),
            steps: 3,
            tool_calls: 2,
            tokens: Tokens { input: 30, ..Default::default() },
            model_ms: 300,
            wall_ms: 1000,
            by_model: BTreeMap::from([("a".into(), agg(2, 20, 200)), ("b".into(), agg(1, 10, 100))]),
            ..Default::default()
        }
    }

    fn only(models: &[&str]) -> Filter {
        Filter { models: models.iter().map(|m| m.to_string()).collect(), ..Default::default() }
    }

    // 模型过滤：只留所选模型的份额，工具和墙钟按整个 turn；没用到所选模型的 turn 丢掉
    #[test]
    fn model_filter_splits_turn() {
        let t = turn();
        let picked = only(&["b"]).pick(&t).expect("用到了 b");
        assert_eq!((picked.steps, picked.tokens.input, picked.model_ms), (1, 10, 100));
        assert_eq!((picked.tool_calls, picked.wall_ms), (2, 1000));
        assert_eq!(picked.by_model.keys().collect::<Vec<_>>(), ["b"]);
        assert!(only(&["c"]).pick(&t).is_none());
        // 推理强度 / skill 也只留所选模型，没剩下模型的标签去掉
        let t = TurnStat {
            by_effort: BTreeMap::from([
                ("high".into(), BTreeMap::from([("a".into(), agg(1, 1, 1)), ("b".into(), agg(1, 2, 2))])),
                ("max".into(), BTreeMap::from([("a".into(), agg(1, 3, 3))])),
            ]),
            ..turn()
        };
        let picked = only(&["b"]).pick(&t).unwrap();
        assert_eq!(picked.by_effort.keys().collect::<Vec<_>>(), ["high"]);
        assert_eq!(picked.by_effort["high"].keys().collect::<Vec<_>>(), ["b"]);
        assert!(matches!(Filter::default().pick(&t), Some(Cow::Borrowed(_))));
    }

    /// 两个 Claude Code 会话：a 在 9-19 用 git + pdf skill，b 在 9-20 用 git + npm
    fn two_sessions() -> (std::path::PathBuf, Store) {
        let dir = std::env::temp_dir().join(format!("harl-summary-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut store = Store::open(dir.clone()).unwrap();
        let session = |day: &str, cwd: &str, cmd: &str, extra: &str| {
            let cwd = cwd.replace('\\', "\\\\");
            [
                format!(r#"{{"type":"user","uuid":"u1","promptId":"p1","cwd":"{cwd}","timestamp":"2026-09-{day}T00:00:00Z","message":{{"role":"user","content":"go"}}}}"#),
                format!(r#"{{"type":"assistant","requestId":"r1","timestamp":"2026-09-{day}T00:00:01Z","message":{{"role":"assistant","model":"m","usage":{{"input_tokens":100,"output_tokens":1}},"content":[{{"type":"tool_use","id":"t1","name":"Bash","input":{{"command":"{cmd}"}}}}]}}}}"#),
                format!(r#"{{"type":"user","promptId":"p1","timestamp":"2026-09-{day}T00:00:02Z","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"t1","content":"ok"}}]}}}}"#),
                extra.replace("DAY", day),
            ]
            .join("\n")
                + "\n"
        };
        let skill = [
            r#"{"type":"assistant","requestId":"r2","timestamp":"2026-09-DAYT00:00:03Z","message":{"role":"assistant","model":"m","usage":{"input_tokens":200,"output_tokens":1},"content":[{"type":"tool_use","id":"t2","name":"Skill","input":{"skill":"pdf"}}]}}"#,
            r#"{"type":"user","promptId":"p1","timestamp":"2026-09-DAYT00:00:04Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t2","content":"Launching skill: pdf"}]}}"#,
            r#"{"type":"user","isMeta":true,"promptId":"p1","timestamp":"2026-09-DAYT00:00:04Z","message":{"role":"user","content":[{"type":"text","text":"Base directory for this skill: E:\\p\\a\\.claude\\skills\\pdf\n\nbody"}]}}"#,
        ]
        .join("\n");
        let a = session("19", r"E:\p\a", "git status", &skill);
        let b = session("20", "/srv/b", "cd /srv/b && git pull && npm test", "");
        let now = Utc::now();
        let mut batch = store.batch().unwrap();
        for (src, bytes) in [("/h/.claude/projects/a/s1.jsonl", &a), ("/h/.claude/projects/b/s2.jsonl", &b)] {
            batch.put_snapshot("claude-code", src, now, bytes.len() as u64, bytes.as_bytes()).unwrap();
        }
        batch.commit().unwrap();
        (dir, store)
    }

    // 范围聚合：工具 / skill / 程序带会话数和最后一次，组成合计等于 prompt 之和；范围和项目过滤照样生效
    #[test]
    fn range_tables_and_composition() {
        let (dir, store) = two_sessions();
        let s = build(&store, 0, Range::default(), Filter::default()).unwrap();
        assert_eq!((s.programs["git"].n, s.programs["git"].sessions), (2, 2));
        assert_eq!((s.programs["npm"].n, s.programs["npm"].sessions), (1, 1));
        assert!(!s.programs.contains_key("cd"));
        assert_eq!(s.by_tool["Bash"].sessions, 2);
        assert_eq!(s.by_tool["Bash"].last_ts.unwrap().to_rfc3339(), "2026-09-20T00:00:02+00:00");
        let pdf = &s.skills["pdf"];
        assert_eq!((pdf.n, pdf.sessions, pdf.source.as_deref()), (1, 1, Some("project")));
        assert_eq!(pdf.path.as_deref(), Some(r"E:\p\a\.claude\skills\pdf"));
        // a：100 + 200，b：100
        assert_eq!(s.composition.sum(), 400);
        let day: BTreeMap<_, _> = s.daily.iter().map(|d| (d.date.as_str(), d)).collect();
        assert_eq!(day["2026-09-19"].skills["pdf"], 1);
        assert_eq!(day["2026-09-20"].programs["npm"], 1);
        assert_eq!(day["2026-09-19"].composition.sum(), 300);
        let row_b = s.sessions.iter().find(|r| r.cwd.as_deref() == Some("/srv/b")).unwrap();
        assert_eq!(row_b.programs.keys().collect::<Vec<_>>(), ["git", "npm"]);
        assert_eq!(row_b.tools["Bash"], 1);
        assert_eq!(row_b.last_user.as_deref(), Some("go"));
        assert_eq!(row_b.ctx_parts.sum(), row_b.ctx);

        // 只看 9-19：npm 不在了
        let d = |s: &str| Some(s.parse::<NaiveDate>().unwrap());
        let s = build(&store, 0, Range { from: d("2026-09-19"), to: d("2026-09-19"), since: None }, Filter::default()).unwrap();
        assert!(!s.programs.contains_key("npm"));
        assert_eq!(s.programs["git"].sessions, 1);
        assert_eq!(s.composition.sum(), 300);
        // 只看项目 b：没有 skill
        let f = Filter { projects: BTreeSet::from(["/srv/b".to_string()]), ..Default::default() };
        let s = build(&store, 0, Range::default(), f).unwrap();
        assert!(s.skills.is_empty());
        assert_eq!(s.by_tool["Bash"].sessions, 1);
        drop(store);
        let _ = std::fs::remove_dir_all(dir);
    }

    // 模型过滤：上下文组成按所选模型的 prompt 占比取，skill / 程序按整个 turn
    #[test]
    fn model_filter_scales_composition() {
        let t = TurnStat {
            composition: Mix { system: 15, user: 15, ..Default::default() },
            programs: UseTable::from([("git".into(), stats::UseAgg { n: 2, ..Default::default() })]),
            ..turn()
        };
        let picked = only(&["b"]).pick(&t).unwrap();
        // b 的 prompt 10 / 全部 30
        assert_eq!(picked.composition.sum(), 10);
        assert_eq!((picked.composition.system, picked.composition.user), (5, 5));
        assert_eq!(picked.programs["git"].n, 2);
    }

    // 会话追加后再取统计：从上一次的检查点接着算，结果和从头算一样
    #[test]
    fn appended_snapshot_resumes() {
        let (dir, mut store) = two_sessions();
        let src = "/h/.claude/projects/a/s1.jsonl";
        let rec_of = |store: &Store| store.latest().unwrap().into_values().find(|r| r.source == src).unwrap();
        let first = rec_of(&store);
        stats_for(&store, &first).unwrap();
        let mut bytes = store.reader().reconstruct(&first).unwrap();
        bytes.extend_from_slice(
            br#"{"type":"user","uuid":"u9","promptId":"p9","timestamp":"2026-09-21T00:00:00Z","message":{"role":"user","content":"more"}}
{"type":"assistant","requestId":"r9","timestamp":"2026-09-21T00:00:05Z","message":{"role":"assistant","model":"m","usage":{"input_tokens":7,"output_tokens":2},"content":[{"type":"text","text":"ok"}]}}
"#,
        );
        let mut batch = store.batch().unwrap();
        batch.put_snapshot("claude-code", src, Utc::now(), bytes.len() as u64, &bytes).unwrap();
        batch.commit().unwrap();
        let second = rec_of(&store);
        assert_ne!(first.tree, second.tree);
        let got = stats_for(&store, &second).unwrap();
        let p = normalize::by_name("claude-code").unwrap();
        let (want, _) = resume::stats(&mut store.reader(), p, &second, None).unwrap();
        assert_eq!(serde_json::to_string(&got).unwrap(), serde_json::to_string(&want).unwrap());
        assert_eq!(got.turns, 2);
        let key = (second.machine.clone(), second.source.clone());
        assert_eq!(resume_memo().as_ref().unwrap()[&key].1.lines(), second.lines as usize);
        drop(store);
        let _ = std::fs::remove_dir_all(dir);
    }

    // prime_all 并行算好放进进程内缓存；gc_if_due 预热完第一次就做，之后按时间节流；GC 只留当前版本 × 最新快照的行。
    // （「没预热完不做」不在这里测：别的测试的 two_sessions 内容相同，会并发往同一个进程内缓存里放同样的键）
    #[test]
    fn prime_all_then_gc() {
        let (dir, store) = two_sessions();
        let recs = normalize::live(store.latest().unwrap().into_values());
        store.put_cached("stats", stats::STATS_VERSION - 1, &recs[0].tree, &"old").unwrap();
        store.put_cached("head", head::HEAD_VERSION, "0000", &"gone").unwrap();
        assert!(prime_all(&store, &recs).unwrap().is_empty());
        for r in &recs {
            assert!(PARSED.lock().unwrap().as_ref().unwrap().contains_key(&(r.tree.clone(), r.source.clone())));
        }
        // 进程内缓存可能已经有别的测试放进去的同样的键，prime_all 就不写 index.db 了；这里保证有一行当前版本的
        stats_for(&store, &recs[0]).unwrap();
        let gc = gc_if_due(&store, &recs).unwrap().expect("预热完第一次就做");
        assert_eq!(gc.deleted, 2);
        assert!(gc.kept >= 1);
        assert_eq!(store.cached::<String>("stats", stats::STATS_VERSION - 1, &recs[0].tree).unwrap(), None);
        assert!(store.cached::<SessionStats>("stats", stats::STATS_VERSION, &recs[0].tree).unwrap().is_some());
        assert!(gc_if_due(&store, &recs).unwrap().is_none(), "之后按时间节流");
        drop(store);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn skill_source_from_path() {
        assert_eq!(skill_source("bundled:simplify", Some("E:/x")), "bundled");
        assert_eq!(skill_source(r"C:\Users\me\.claude\plugins\cache\p\skills\tdd", None), "plugin");
        assert_eq!(skill_source(r"E:\Code\Harl\.claude\skills\x", Some("e:/code/harl")), "project");
        assert_eq!(skill_source("E:/code/harl2/.claude/skills/x", Some("E:/code/harl")), "user");
        assert_eq!(skill_source("/home/me/.codex/skills/a/SKILL.md", Some("/srv/p")), "user");
    }

    // 项目键和 web 侧栏同一口径：大小写、分隔符、结尾斜杠不同的同一目录是一个项目
    #[test]
    fn project_key_normalizes() {
        assert_eq!(project_key(r"E:\code\Harl\"), "e:/code/harl");
        assert_eq!(project_key("e:/code/harl"), "e:/code/harl");
        assert_eq!(project_key(""), "");
    }

    // 分桶按上界（不含），百分位取最近秩
    #[test]
    fn turn_durations_buckets_and_percentiles() {
        let d = TurnDurations::build(vec![5_000, 10_000, 45_000, 3_600_000, 7_200_000], Vec::new());
        assert_eq!(d.counts, [1, 1, 1, 0, 0, 0, 0, 0, 2]);
        assert_eq!((d.p50_ms, d.p90_ms, d.max_ms), (45_000, 7_200_000, 7_200_000));
        let empty = TurnDurations::build(Vec::new(), Vec::new());
        assert_eq!((empty.p50_ms, empty.max_ms), (0, 0));
    }
}
