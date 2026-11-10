//! `harl serve`：本地 HTTP。`/api/*` 是 JSON，其余路径给 web 的构建产物（编译时嵌进二进制），
//! 没有的路径回退到 index.html 让前端路由接管。
//!
//! Store 不是 Sync（rusqlite）。写的一方（capture、同步）共用放在 Mutex 里的那个句柄；
//! HTTP 请求在 blocking 线程上从读句柄池借一个（各自一条 SQLite 连接，WAL 下互不阻塞），页面同时发的几个请求不用排队。
//! 后台线程监听会话目录，有变化就增量 capture：扫盘不拿锁，只有写快照时拿锁（增量时很快）。

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex, mpsc};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use chrono::{DateTime, NaiveDate, Utc};
use notify::{EventKind, RecursiveMode, Watcher};
use rust_embed::RustEmbed;
use rust_i18n::t;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use harl_core::event::Event;
use harl_core::normalize::{self, AgentKind, Run, homes};
use harl_core::store::{self, Record, Store};

use crate::config::UpdateConfig;
use crate::head;
use crate::summary;
use crate::sync::{self, Remote};
use crate::update::{self, Updater, VersionInfo};

#[derive(RustEmbed)]
#[folder = "../../web/dist"]
struct Web;

type Shared = Arc<Mutex<Store>>;

/// 读句柄池里最多留这么多个空闲的；并发多于此时临时开的用完就关
const POOL_IDLE: usize = 8;

/// HTTP 读请求用的 Store 句柄（`Store::reopen`）：借出去用完放回，空了就新开一个
struct Pool {
    /// 只用来 reopen；不和写句柄共用锁，capture 写着的时候也能开新句柄
    seed: Mutex<Store>,
    idle: Mutex<Vec<Store>>,
}

impl Pool {
    fn new(store: &Store) -> Result<Self> {
        Ok(Self { seed: Mutex::new(store.reopen()?), idle: Mutex::new(Vec::new()) })
    }

    fn get(&self) -> Result<Store> {
        if let Some(s) = self.idle.lock().unwrap_or_else(|p| p.into_inner()).pop() {
            return Ok(s);
        }
        self.seed.lock().unwrap_or_else(|p| p.into_inner()).reopen()
    }

    fn put(&self, store: Store) {
        let mut idle = self.idle.lock().unwrap_or_else(|p| p.into_inner());
        if idle.len() < POOL_IDLE {
            idle.push(store);
        }
    }
}

/// events 响应体的缓存上限（所有会话合计）；最久没用的先丢
const EVENTS_CACHE_BYTES: usize = 256 << 20;

/// 序列化好的 events 响应体，按 tree 记：快照不可变，同一个 tree 的事件流不会变
#[derive(Default)]
struct Bodies {
    map: HashMap<String, Bytes>,
    /// 从最久没用到最近用过
    order: VecDeque<String>,
    bytes: usize,
}

impl Bodies {
    fn get(&mut self, id: &str) -> Option<Bytes> {
        let body = self.map.get(id)?.clone();
        if let Some(i) = self.order.iter().position(|k| k == id) {
            let k = self.order.remove(i).unwrap();
            self.order.push_back(k);
        }
        Some(body)
    }

    fn put(&mut self, id: String, body: Bytes) {
        if body.len() > EVENTS_CACHE_BYTES || self.map.contains_key(&id) {
            return;
        }
        self.bytes += body.len();
        self.order.push_back(id.clone());
        self.map.insert(id, body);
        while self.bytes > EVENTS_CACHE_BYTES {
            let Some(old) = self.order.pop_front() else { break };
            if let Some(b) = self.map.remove(&old) {
                self.bytes -= b.len();
            }
        }
    }
}

/// handler 共用的状态
#[derive(Clone)]
struct App {
    /// capture / 同步写入用的句柄；HTTP 只在重启前拿它（等 capture 写完）
    store: Shared,
    pool: Arc<Pool>,
    /// /api/sessions 的响应体，键是 (manifest 版本, packs 行数, pack 总字节)：
    /// 列表只由各源的最新记录和它们的 head 决定，head 只取决于内容（chunk 晚到时 packs 会变）
    sessions: Arc<Mutex<Option<((u64, u64, u64), Bytes)>>>,
    events: Arc<Mutex<Bodies>>,
}

#[derive(Serialize)]
struct Status {
    data_dir: String,
    machine: String,
    /// 存储里所有机器：id → 主机名（raw/machines/）
    machines: BTreeMap<String, String>,
    sources: usize,
    records: usize,
    logical_bytes: u64,
    packs: u64,
    pack_bytes: u64,
    chunks: u64,
    index_bytes: u64,
}

/// 一个源文件的最新快照，id 是 tree hash
#[derive(Serialize)]
struct Session {
    id: String,
    provider: String,
    machine: String,
    source: String,
    session: String,
    parent_session: Option<String>,
    /// 第一条用户提示的第一行，没有就 None
    title: Option<String>,
    /// 会话的工作目录（`/` 分隔），UI 按它分组；开头几行里没写就 None
    cwd: Option<String>,
    /// 作为 agent 的类别：`main`、`subagent`、`fork`、`workflow`，或 provider 自己的类别（Codex 的 `review`）
    kind: AgentKind,
    /// agent 的短名字（Codex 的 agent 路径 / 昵称、workflow 脚本给的 label）；不知道就 None
    label: Option<String>,
    /// 同一次编排（一次 workflow 运行）的分组键，只在同一个根会话下面有意义；拿不到就 None
    run: Option<Run>,
    /// 这次编排的由来：workflow 转述的用户请求的第一行
    run_title: Option<String>,
    size: u64,
    lines: u32,
    mtime: DateTime<Utc>,
    captured_at: DateTime<Utc>,
}

/// anyhow 错误 → 500 + 文字；找不到 → 404
struct ApiError(StatusCode, String);

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        Self(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, self.1).into_response()
    }
}

/// 在 blocking 线程上借一个读句柄干活
async fn with_store<T: Send + 'static>(
    app: &App,
    f: impl FnOnce(&Store) -> Result<T, ApiError> + Send + 'static,
) -> Result<T, ApiError> {
    let pool = app.pool.clone();
    tokio::task::spawn_blocking(move || {
        let store = pool.get()?;
        let out = f(&store);
        pool.put(store);
        out
    })
    .await
    .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
}

async fn status(State(app): State<App>) -> Result<Json<Status>, ApiError> {
    let s = with_store(&app, |store| {
        let m = store.manifest()?;
        let stats = store.stats()?;
        Ok(Status {
            data_dir: store.root().display().to_string(),
            machine: store.machine().to_string(),
            machines: store.machines()?.into_values().map(|m| (m.id, m.hostname)).collect(),
            sources: m.latest.len(),
            records: m.records,
            logical_bytes: m.latest.values().map(|r| r.size).sum(),
            packs: stats.packs,
            pack_bytes: stats.pack_bytes,
            chunks: stats.chunks,
            index_bytes: stats.index_bytes,
        })
    })
    .await?;
    Ok(Json(s))
}

#[derive(Deserialize)]
struct SummaryQuery {
    /// 客户端时区偏移，分钟，东为正（JS 的 -getTimezoneOffset()）
    #[serde(default)]
    tz: i32,
    /// 本地日期 YYYY-MM-DD，两端都含；不给就不限
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
    /// RFC 3339 时刻，滚动窗口的起点（含）
    since: Option<DateTime<Utc>>,
    /// 逗号分隔，多选；不给就不限
    provider: Option<String>,
    model: Option<String>,
    /// 换行分隔的项目键（路径可能含逗号，见 summary::project_key）；空串表示"没有 cwd 的会话"
    project: Option<String>,
}

fn csv(v: Option<String>) -> BTreeSet<String> {
    v.iter().flat_map(|s| s.split(',')).map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect()
}

/// 项目是路径，可能含逗号，所以前端用 `\n` 分隔；空项保留（表示没有 cwd）
fn projects(v: Option<String>) -> BTreeSet<String> {
    v.iter().flat_map(|s| s.split('\n')).map(String::from).collect()
}

async fn summary(State(app): State<App>, Query(q): Query<SummaryQuery>) -> Result<Response, ApiError> {
    let range = summary::Range { from: q.from, to: q.to, since: q.since };
    let filter = summary::Filter { providers: csv(q.provider), models: csv(q.model), projects: projects(q.project) };
    let s = with_store(&app, move |store| Ok(summary::build(store, q.tz, range, filter)?)).await?;
    Ok(Json(s).into_response())
}

async fn sessions(State(app): State<App>) -> Result<Response, ApiError> {
    let cache = app.sessions.clone();
    let body = with_store(&app, move |store| {
        let m = store.manifest()?;
        let (packs, pack_bytes) = store.pack_totals()?;
        let key = (m.generation, packs, pack_bytes);
        // 拿着锁建：同时来的几个请求只建一次
        let mut cached = cache.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((k, body)) = cached.as_ref()
            && *k == key
        {
            return Ok(body.clone());
        }
        let body = Bytes::from(serde_json::to_vec(&session_list(store, &m.latest)).map_err(anyhow::Error::from)?);
        *cached = Some((key, body.clone()));
        Ok(body)
    })
    .await?;
    Ok(([(header::CONTENT_TYPE, "application/json")], body).into_response())
}

/// 每个源文件的最新快照，最近 capture 的在前
fn session_list(store: &Store, latest: &store::Latest) -> Vec<Session> {
    let mut reader = store.reader();
    let live = normalize::live(latest.values().cloned());
    head::retain(&live.iter().map(|r| r.tree.as_str()).collect::<HashSet<_>>());
    let (recs, mut rows): (Vec<Record>, Vec<head::ListRow>) = live
        .into_iter()
        .map(|r| {
            // 标题拿不到（chunk 缺失等）不影响列表
            let row = match normalize::by_name(&r.provider) {
                Some(p) => head::row_for(store, &mut reader, p, &r),
                None => head::ListRow { session: normalize::file_stem(&r.source).to_string(), head: Default::default(), dir: None },
            };
            (r, row)
        })
        .unzip();
    head::fill_agents(&mut rows);
    // journal 是 workflow 的运行日志，不是对话：只用来给同一 run 目录的 agent 补 label，不进列表
    let mut out: Vec<Session> = recs
        .into_iter()
        .zip(rows)
        .filter(|(_, row)| row.head.agent.kind != AgentKind::Journal)
        .map(|(r, head::ListRow { session, head: h, .. })| Session {
            id: r.tree,
            provider: r.provider,
            machine: r.machine,
            source: r.source,
            session,
            parent_session: h.parent,
            title: h.title,
            cwd: h.cwd,
            kind: h.agent.kind,
            label: h.agent.label,
            run: h.agent.run,
            run_title: h.agent.run_title,
            size: r.size,
            lines: r.lines,
            mtime: r.mtime,
            captured_at: r.captured_at,
        })
        .collect();
    out.sort_by_key(|s| std::cmp::Reverse(s.captured_at));
    out
}

fn find(store: &Store, id: &str) -> Result<Record, ApiError> {
    store
        .record_by_tree(id)
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, t!("serve.no_snapshot", tree = id).into_owned()))
}

/// 这个构建的标识，拼进 events 的 ETag：同一个 tree 换了版本（normalizer 改了）出来的事件可能不同。
/// 开发时版本号不变，所以再带上可执行文件的修改时间
static BUILD: LazyLock<String> = LazyLock::new(|| {
    let stamp = std::env::current_exe()
        .and_then(std::fs::metadata)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    format!("{}-{stamp:x}", env!("CARGO_PKG_VERSION"))
});

/// 快照按内容寻址、不可变，响应可以按 ETag 缓存。不用 immutable：升级后同一个 tree 的 events 可能变，
/// 浏览器每次带 If-None-Match 来问一下（本地一个来回），没变就 304
const REVALIDATE: &str = "private, no-cache";

/// If-None-Match 里有没有这个 ETag（可以是逗号分隔的多个，或者 `*`）
fn etag_matches(headers: &HeaderMap, etag: &str) -> bool {
    headers
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(str::trim)
        .any(|t| t == "*" || t.strip_prefix("W/").unwrap_or(t) == etag)
}

/// 带上缓存头；客户端已经有这一份就回 304
fn cacheable(headers: &HeaderMap, etag: &str, content_type: &'static str, body: impl FnOnce() -> Bytes) -> Response {
    let cache = [(header::ETAG, etag.to_string()), (header::CACHE_CONTROL, REVALIDATE.to_string())];
    if etag_matches(headers, etag) {
        return (StatusCode::NOT_MODIFIED, cache).into_response();
    }
    (cache, [(header::CONTENT_TYPE, content_type)], body()).into_response()
}

#[derive(Deserialize)]
struct EventsQuery {
    /// `slim=1`：大块的 base64（图片、文件）换成占位，见 [`elide`]
    slim: Option<String>,
}

impl EventsQuery {
    fn slim(&self) -> bool {
        self.slim.as_deref().is_some_and(|v| v == "1" || v == "true")
    }
}

async fn events(
    State(app): State<App>,
    Path(id): Path<String>,
    Query(q): Query<EventsQuery>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let slim = q.slim();
    // 两种形状各自一个 ETag、各自一份缓存
    let variant = if slim { "-slim" } else { "" };
    let etag = format!("\"{id}-{}{variant}\"", *BUILD);
    // 记录只追加不删除，客户端拿到过这个 ETag 就说明这个快照在，不用再查
    if etag_matches(&headers, &etag) {
        return Ok(cacheable(&headers, &etag, "application/json", Bytes::new));
    }
    let cache_key = format!("{id}{variant}");
    let hit = app.events.lock().unwrap_or_else(|p| p.into_inner()).get(&cache_key);
    let body = match hit {
        Some(b) => b,
        None => {
            let body = Bytes::from(with_store(&app, move |store| events_json(store, &id, slim)).await?);
            app.events.lock().unwrap_or_else(|p| p.into_inner()).put(cache_key, body.clone());
            body
        }
    };
    Ok(cacheable(&headers, &etag, "application/json", || body))
}

/// 一个快照的 canonical 事件，序列化成 JSON 数组；slim 时大块 base64 换成占位
fn events_json(store: &Store, id: &str, slim: bool) -> Result<Vec<u8>, ApiError> {
    // 直接序列化成 JSON 数组的字节，省得先攒一个 Vec<Event> 再序列化一遍
    let rec = find(store, id)?;
    let provider = normalize::by_name(&rec.provider)
        .with_context(|| t!("main.no_normalizer", provider = rec.provider))
        .map_err(ApiError::from)?;
    let lines = store.reader().lines(&rec).map_err(ApiError::from)?;
    let mut out = vec![b'['];
    let mut first = true;
    // 占位的序号在同一个 seq（源文件的一行，可能出好几个事件）里从 0 数
    let mut ord = (u32::MAX, 0u32);
    normalize::normalize(provider, &rec.source, &lines, |mut ev| {
        if !first {
            out.push(b',');
        }
        first = false;
        if slim {
            if ord.0 != ev.seq {
                ord = (ev.seq, 0);
            }
            let seq = ev.seq;
            visit_blobs(&mut ev, &mut |s, at| {
                let len = s.len() - at;
                s.truncate(at);
                s.push_str(&elided_marker(seq, ord.1, len));
                ord.1 += 1;
            });
        }
        serde_json::to_writer(&mut out, &ev)?;
        Ok(())
    })
    .map_err(ApiError::from)?;
    out.push(b']');
    Ok(out)
}

/// 短于这个长度的 base64 不省略：省不了多少，还要多一次请求
const ELIDE_MIN: usize = 1024;

/// 省略掉的 base64 留下的占位：`harl:elided:<seq>:<序号>:<原长度>`。
/// 前端按它去 `/api/sessions/{id}/blobs/{seq}/{序号}` 取原文；token 估算不读 base64（图片按尺寸算），
/// 所以 slim 和完整版折出来的上下文数字一样
fn elided_marker(seq: u32, ord: u32, len: usize) -> String {
    format!("harl:elided:{seq}:{ord}:{len}")
}

/// 字符串里能省略的 base64 从哪开始。`data:<mime>;base64,` 的 URL 在哪都算（省略逗号后面的部分）；
/// 整串 base64 只在 slot（图片、文件的载荷位置，见 [`visit_blobs`]）上才算：工具输出里恰好是一大段 base64 的文本
/// 要按字符数估 token、数行数，不能动。不能省略就 None
fn blob_start(s: &str, slot: bool) -> Option<usize> {
    if s.len() < ELIDE_MIN {
        return None;
    }
    let at = if s.starts_with("data:") {
        let head = &s[..s.len().min(128)];
        head.find(";base64,")? + ";base64,".len()
    } else if slot {
        0
    } else {
        return None;
    };
    let body = &s.as_bytes()[at..];
    let base64 = |b: &u8| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'=' | b'\n' | b'\r');
    (body.len() >= ELIDE_MIN && body.iter().all(base64)).then_some(at)
}

/// 对象里哪个键放的是二进制载荷：
/// - Anthropic 的图片 / 文档块 `{type: "base64", media_type, data}`、MCP 的 `{type: "image", data, mimeType}`；
/// - Claude Code 读图片时 toolUseResult 的 `file.base64`；
/// - Codex 生图结果的 `result`（`image_generation_call`，或带 revisedPrompt 的 item）
fn is_slot(o: &serde_json::Map<String, Value>, key: &str) -> bool {
    let ty = o.get("type").and_then(Value::as_str);
    match key {
        "base64" => true,
        "data" => matches!(ty, Some("base64" | "image")) || o.contains_key("media_type") || o.contains_key("mimeType"),
        "result" => ty == Some("image_generation_call") || o.contains_key("revisedPrompt") || o.contains_key("revised_prompt"),
        _ => false,
    }
}

/// 按固定顺序（ext、tool.input、tool.output，对象按键序）访问事件里能省略的 base64 串。
/// slim 序列化和取原文共用它，序号才对得上
fn visit_blobs(ev: &mut Event, f: &mut impl FnMut(&mut String, usize)) {
    fn walk(v: &mut Value, slot: bool, f: &mut impl FnMut(&mut String, usize)) {
        match v {
            Value::String(s) => {
                if let Some(at) = blob_start(s, slot) {
                    f(s, at);
                }
            }
            Value::Array(a) => a.iter_mut().for_each(|x| walk(x, false, f)),
            Value::Object(o) => {
                let slots: Vec<bool> = o.keys().map(|k| is_slot(o, k)).collect();
                for ((_, x), slot) in o.iter_mut().zip(slots) {
                    walk(x, slot, f);
                }
            }
            _ => {}
        }
    }
    ev.ext.values_mut().for_each(|x| walk(x, false, f));
    if let Some(tool) = &mut ev.tool {
        for v in [&mut tool.input, &mut tool.output].into_iter().flatten() {
            walk(v, false, f);
        }
    }
}

/// slim 省略掉的一块 base64 原文（不含 `data:…;base64,` 前缀）
async fn blob(
    State(app): State<App>,
    Path((id, seq, ord)): Path<(String, u32, u32)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let etag = format!("\"{id}-{}-{seq}-{ord}\"", *BUILD);
    if etag_matches(&headers, &etag) {
        return Ok(cacheable(&headers, &etag, "text/plain; charset=utf-8", Bytes::new));
    }
    let body = with_store(&app, move |store| blob_of(store, &id, seq, ord)).await?;
    Ok(cacheable(&headers, &etag, "text/plain; charset=utf-8", || Bytes::from(body)))
}

fn blob_of(store: &Store, id: &str, seq: u32, ord: u32) -> Result<String, ApiError> {
    let rec = find(store, id)?;
    let provider = normalize::by_name(&rec.provider)
        .with_context(|| t!("main.no_normalizer", provider = rec.provider))
        .map_err(ApiError::from)?;
    let lines = store.reader().lines(&rec).map_err(ApiError::from)?;
    let mut n = 0u32;
    let mut found = None;
    // normalizer 有状态，要从头跑；过了这一行就不用再往下
    let res = normalize::normalize(provider, &rec.source, &lines, |mut ev| {
        if ev.seq > seq {
            anyhow::bail!(PastSeq);
        }
        if ev.seq == seq && found.is_none() {
            visit_blobs(&mut ev, &mut |s, at| {
                if n == ord {
                    found = Some(s.split_off(at));
                }
                n += 1;
            });
        }
        Ok(())
    });
    if let Err(e) = res
        && !e.is::<PastSeq>()
    {
        return Err(e.into());
    }
    found.ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("no blob {seq}:{ord} in {id}")))
}

/// 取原文时提前停下 normalize 用的哨兵
#[derive(Debug)]
struct PastSeq;

impl std::fmt::Display for PastSeq {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("past seq")
    }
}

impl std::error::Error for PastSeq {}

async fn raw(State(app): State<App>, Path(id): Path<String>, headers: HeaderMap) -> Result<Response, ApiError> {
    // 原文逐字节由 tree 决定，和版本无关
    let etag = format!("\"{id}\"");
    if etag_matches(&headers, &etag) {
        return Ok(cacheable(&headers, &etag, "text/plain; charset=utf-8", Bytes::new));
    }
    let bytes = with_store(&app, move |store| {
        let rec = find(store, &id)?;
        Ok(store.reader().reconstruct(&rec)?)
    })
    .await?;
    Ok(cacheable(&headers, &etag, "text/plain; charset=utf-8", || Bytes::from(bytes)))
}

/// 嵌入的静态文件；没有就回 index.html（前端路由）
async fn web(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let (path, file) = match Web::get(path) {
        Some(f) if !path.is_empty() => (path, f),
        _ => match Web::get("index.html") {
            Some(f) => ("index.html", f),
            None => {
                return (StatusCode::NOT_FOUND, t!("serve.no_web_build")).into_response();
            }
        },
    };
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    ([(header::CONTENT_TYPE, mime.as_ref())], file.data).into_response()
}

#[derive(Deserialize)]
struct VersionQuery {
    /// 忽略缓存，马上查一次
    #[serde(default)]
    force: bool,
}

async fn version(Extension(up): Extension<Arc<Updater>>, Query(q): Query<VersionQuery>) -> Json<VersionInfo> {
    Json(up.info(q.force).await)
}

/// 只接受本页面发起的请求：Origin 必须和 Host 一致。
/// 更新会替换本机的可执行文件，不能让随便哪个网页对 127.0.0.1 发个 POST 就触发
fn same_origin(headers: &HeaderMap) -> Result<(), ApiError> {
    let get = |h| headers.get(h).and_then(|v| v.to_str().ok());
    let forbid = || ApiError(StatusCode::FORBIDDEN, t!("serve.same_origin_only").into_owned());
    let (Some(origin), Some(host)) = (get(header::ORIGIN), get(header::HOST)) else {
        return Err(forbid());
    };
    let origin_host = origin.split_once("://").map(|(_, rest)| rest).unwrap_or_default();
    if origin_host != host {
        return Err(forbid());
    }
    Ok(())
}

#[derive(Serialize)]
struct Updated {
    version: String,
}

/// 下载并换上新版本，回应之后重启自己；页面轮询 /api/version 等新版本起来。
/// 安装和重启在单独的任务里跑：页面中途关掉（axum 丢掉这个 handler 的 future）也会装完、重启
async fn do_update(
    State(app): State<App>,
    Extension(up): Extension<Arc<Updater>>,
    headers: HeaderMap,
) -> Result<Json<Updated>, ApiError> {
    same_origin(&headers)?;
    let task = tokio::spawn(async move {
        let version = up.update(true).await?;
        println!("update  {}", t!("serve.updated_restarting", version = version));
        tokio::spawn(async move {
            // 让响应先发出去
            tokio::time::sleep(Duration::from_millis(500)).await;
            restart_quiet(&app.store, up.exe())
        });
        anyhow::Ok(version)
    });
    let version = task
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(|e| ApiError(StatusCode::CONFLICT, format!("{e:#}")))?;
    Ok(Json(Updated { version }))
}

/// 换上新版本后重启：拿着存储锁退出，保证不会在 capture 写到一半时退出。手动和自动更新都走这里
fn restart_quiet(state: &Shared, exe: &std::path::Path) -> ! {
    let _quiet = state.lock().unwrap_or_else(|p| p.into_inner());
    update::restart(exe)
}

/// 自动更新：等到装上新版本就重启
async fn auto_update(up: Arc<Updater>, state: Shared) {
    let version = update::auto(&up).await;
    println!("update  {}", t!("update.auto_installed", version = version));
    restart_quiet(&state, up.exe())
}

fn router(app: App, updater: Arc<Updater>) -> Router {
    Router::new()
        .route("/api/version", get(version))
        .route("/api/update", post(do_update))
        .route("/api/status", get(status))
        .route("/api/summary", get(summary))
        .route("/api/sessions", get(sessions))
        .route("/api/sessions/{id}/events", get(events))
        .route("/api/sessions/{id}/blobs/{seq}/{ord}", get(blob))
        .route("/api/sessions/{id}/raw", get(raw))
        .fallback(web)
        .layer(Extension(updater))
        .with_state(app)
}

/// 变化后安静这么久才 capture：会话文件是一行行追加的，一轮对话会连续写很多次
const QUIET: Duration = Duration::from_secs(2);
/// 一直在写（长任务）时最多攒这么久也 capture 一次
const MAX_DELAY: Duration = Duration::from_secs(15);
/// 收不到变化通知的根目录（WSL 的 9P 共享、WSL 里挂的 Windows 盘）隔这么久轮询一次
const POLL: Duration = Duration::from_secs(30);

fn capture_now(state: &Shared, roots: &[PathBuf], after: &AfterCapture) {
    let files = crate::discover(roots);
    let res = {
        let mut store = state.lock().unwrap_or_else(|p| p.into_inner());
        crate::capture(&mut store, &files)
    };
    match res {
        Ok(c) if c.wrote() => {
            println!("capture  {c}");
            if let Some(kick) = &after.kick {
                let _ = kick.send(());
            }
            if let Some(pool) = &after.warm {
                warm(pool, c.done.records);
            }
        }
        Ok(_) => {}
        Err(e) => crate::warn(t!("serve.capture_failed", error = e : {:#})),
    }
}

/// 全部最新快照；启动时预热用
fn live_records(pool: &Pool) -> Vec<Record> {
    match pool.get().and_then(|store| {
        let m = store.manifest();
        pool.put(store);
        m
    }) {
        Ok(m) => normalize::live(m.latest.values().cloned()),
        Err(e) => {
            crate::warn(t!("serve.warm_failed", error = e : {:#}));
            Vec::new()
        }
    }
}

/// 把这些快照的 stats 和 head 预先算好进缓存（index.db 和进程内都有），页面打开时就不用等。
/// 用读句柄，不挡 capture 也不挡 API 请求；已缓存的只是一次查表。没缓存的并行算（`summary::prime_all`）。
/// 预热完、到时间了顺手清掉 index.db 里用不上的派生缓存（`summary::gc_if_due`：启动那一轮之后一次，之后每几个小时一次）
fn warm(pool: &Pool, recs: Vec<Record>) {
    let store = match pool.get() {
        Ok(s) => s,
        Err(e) => {
            crate::warn(t!("serve.warm_failed", error = e : {:#}));
            return;
        }
    };
    match summary::prime_all(&store, &recs) {
        Ok(failed) => {
            for (source, e) in failed {
                crate::warn(t!("serve.stats_failed", source = source, error = e : {:#}));
            }
        }
        Err(e) => crate::warn(t!("serve.warm_failed", error = e : {:#})),
    }
    match summary::gc_if_due(&store, &recs) {
        Ok(Some(gc)) if gc.deleted + gc.legacy > 0 => {
            println!("gc  {}", t!("serve.gc_done", removed = gc.deleted + gc.legacy, kept = gc.kept))
        }
        Ok(_) => {}
        Err(e) => crate::warn(t!("serve.gc_failed", error = e : {:#})),
    }
    pool.put(store);
}

/// 后台线程：启动时 capture 一次，然后监听 roots，会话文件有变化就（防抖后）增量 capture
/// 会话文件有变化就往 tx 发一个信号
fn on_change(tx: mpsc::Sender<()>) -> impl notify::EventHandler {
    move |res: notify::Result<notify::Event>| {
        let Ok(ev) = res else { return };
        if matches!(ev.kind, EventKind::Access(_)) {
            return;
        }
        let relevant = ev
            .paths
            .iter()
            .any(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(store::is_session_file));
        if relevant {
            let _ = tx.send(());
        }
    }
}

/// capture 写了新快照之后做什么
struct AfterCapture {
    /// 叫醒同步线程，尽快推上去
    kick: Option<mpsc::Sender<()>>,
    /// 预先算好统计和会话头（有 UI 时才值得），用这个池里的读句柄
    warm: Option<Arc<Pool>>,
}

/// 监听器建不起来时先照常 capture 一次，再返回错误：agent 据此非零退出交给守护者退避重启，serve 只警告
fn watch(state: Shared, roots: Vec<PathBuf>, after: AfterCapture) -> Result<()> {
    let (tx, rx) = mpsc::channel();
    // 跨 WSL / Windows 的根收不到系统通知，改用轮询；两种监听发到同一个 channel
    let native = notify::recommended_watcher(on_change(tx.clone()));
    let poller = notify::PollWatcher::new(on_change(tx), notify::Config::default().with_poll_interval(POLL));
    let mut watchers = match (native, poller) {
        (Ok(n), Ok(p)) => Ok((n, p)),
        (Err(e), _) | (_, Err(e)) => Err(e),
    };
    if let Ok((native, poller)) = &mut watchers {
        for r in roots.iter().filter(|r| r.is_dir()) {
            let w: &mut dyn Watcher = if homes::is_foreign(r) { poller } else { native };
            if let Err(e) = w.watch(r, RecursiveMode::Recursive) {
                crate::warn(t!("serve.watch_failed", path = r.display(), error = e));
            }
        }
    }

    capture_now(&state, &roots, &after);
    if let Some(pool) = &after.warm {
        warm(pool, live_records(pool));
    }
    // 监听器得活到下面的循环结束：丢掉它们 channel 就断了
    let _watchers = watchers.map_err(|e| anyhow::anyhow!("{}", t!("serve.watcher_failed", error = e)))?;
    // 阻塞等第一个变化，然后等到安静 QUIET 或者累计 MAX_DELAY 再 capture
    while rx.recv().is_ok() {
        let start = Instant::now();
        while start.elapsed() < MAX_DELAY && rx.recv_timeout(QUIET).is_ok() {}
        capture_now(&state, &roots, &after);
    }
    Ok(())
}

pub struct Http {
    pub host: String,
    pub port: u16,
    pub open: bool,
}

pub struct Options {
    /// None：不起 HTTP，只采集（`harl agent`）
    pub http: Option<Http>,
    /// Some：后台监听这些目录并增量 capture
    pub watch_roots: Option<Vec<PathBuf>>,
    /// Some：后台同步到远端
    pub remote: Option<Remote>,
    /// 新版本检查（UI 上提示）和自动更新
    pub update: UpdateConfig,
}

pub fn run(store: Store, opts: Options) -> Result<()> {
    let state: Shared = Arc::new(Mutex::new(store));
    let kick = opts.remote.map(|remote| {
        let (tx, rx) = mpsc::channel();
        let state = state.clone();
        std::thread::spawn(move || sync::run_loop(remote, state, rx));
        tx
    });
    // 有 UI 才要读句柄池
    let pool = match &opts.http {
        Some(_) => Some(Arc::new(Pool::new(&state.lock().unwrap_or_else(|p| p.into_inner()))?)),
        None => None,
    };
    let after = AfterCapture { kick, warm: pool.clone() };
    let updater = Arc::new(Updater::new(opts.update.check));
    let auto = opts.update.auto && updater.can_self_update();
    let (Some(Http { host, port, open }), Some(pool)) = (opts.http, pool) else {
        // 只采集时没有 tokio runtime，自动更新单独起一个线程
        if auto {
            let (up, state) = (updater, state.clone());
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread().enable_all().build();
                match rt {
                    Ok(rt) => rt.block_on(auto_update(up, state)),
                    Err(e) => crate::warn(t!("update.auto_failed", error = e)),
                }
            });
        }
        // 只采集：在当前线程监听；没有要监听的就只剩同步线程在跑
        // 监听不了就带着错误退出（非零），守护者会退避后重启；退出码 0 会让 harlw 当成正常停止
        return match opts.watch_roots {
            Some(roots) => watch(state, roots, after),
            None => loop {
                std::thread::park();
            },
        };
    };
    if let Some(roots) = opts.watch_roots {
        let state = state.clone();
        // 有 UI 时监听不了不退出：页面还能用，启动时已经 capture 过一次
        std::thread::spawn(move || {
            if let Err(e) = watch(state, roots, after) {
                crate::warn(format!("{e:#}"));
            }
        });
    }
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let addr: SocketAddr = format!("{host}:{port}").parse().context(t!("serve.bad_addr"))?;
        let listener = bind(addr).await?;
        let url = format!("http://{addr}");
        println!("harl serve  {url}");
        // 自更新后自己拉起的新进程带着原来的 --open，不要每次更新都再开一个标签页
        if open && std::env::var_os(crate::common::RESTARTED_ENV).is_none() {
            let _ = ::open::that(&url);
        }
        if auto {
            tokio::spawn(auto_update(updater.clone(), state.clone()));
        }
        let app = App { store: state, pool, sessions: Arc::default(), events: Arc::default() };
        axum::serve(listener, router(app, updater)).await?;
        Ok(())
    })
}

/// 更新后自己拉起的新进程会比旧进程先跑到这里：端口被占时等一会儿
async fn bind(addr: SocketAddr) -> Result<tokio::net::TcpListener> {
    let mut tries = 0;
    loop {
        match tokio::net::TcpListener::bind(addr).await {
            Ok(l) => return Ok(l),
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse && tries < 40 => {
                tries += 1;
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            Err(e) => return Err(e).with_context(|| t!("serve.listen", addr = addr)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn etag_list_weak_and_star() {
        let mut h = HeaderMap::new();
        assert!(!etag_matches(&h, "\"a\""));
        h.insert(header::IF_NONE_MATCH, "\"b\", W/\"a\"".parse().unwrap());
        assert!(etag_matches(&h, "\"a\""));
        assert!(!etag_matches(&h, "\"c\""));
        h.insert(header::IF_NONE_MATCH, "*".parse().unwrap());
        assert!(etag_matches(&h, "\"c\""));
    }

    #[test]
    fn blob_start_only_long_base64() {
        let b64 = "QUJD".repeat(300);
        assert_eq!(blob_start(&b64, true), Some(0));
        assert_eq!(blob_start(&format!("data:image/png;base64,{b64}"), false), Some(22));
        // 太短、不是 base64、data URL 但不是 base64、不在载荷位置上的都不动
        assert_eq!(blob_start("QUJD", true), None);
        assert_eq!(blob_start(&format!("{b64} "), true), None);
        assert_eq!(blob_start(&format!("data:text/plain,{b64}"), true), None);
        assert_eq!(blob_start(&b64, false), None);
    }

    #[test]
    fn slim_markers_number_blobs_in_visit_order() {
        let b64 = "QUJD".repeat(300);
        let mut ev = Event { seq: 7, ..Default::default() };
        ev.tool = Some(harl_core::event::Tool {
            output: Some(serde_json::json!([
                { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": b64.clone() } },
                // 工具输出的正文恰好是 base64：要估 token，不省略
                { "type": "text", "text": b64.clone() }
            ])),
            ..Default::default()
        });
        ev.ext.insert("a".into(), serde_json::json!({ "file": { "base64": format!("data:image/png;base64,{b64}") } }));
        ev.ext.insert("b".into(), "short".into());
        ev.ext.insert("c".into(), serde_json::json!({ "data": b64.clone() }));
        let mut seen = Vec::new();
        visit_blobs(&mut ev, &mut |s, at| {
            seen.push(at);
            let len = s.len() - at;
            s.truncate(at);
            s.push_str(&elided_marker(7, seen.len() as u32 - 1, len));
        });
        assert_eq!(seen, [22, 0]);
        assert_eq!(ev.ext["a"]["file"]["base64"], "data:image/png;base64,harl:elided:7:0:1200");
        let output = ev.tool.as_ref().and_then(|t| t.output.as_ref()).unwrap();
        assert_eq!(output[0]["source"]["data"], "harl:elided:7:1:1200");
        assert_eq!(output[1]["text"], b64.as_str());
        assert_eq!(ev.ext["b"], "short");
        assert_eq!(ev.ext["c"]["data"], b64.as_str());
    }

    #[test]
    fn bodies_evict_least_recent_by_bytes() {
        let mut b = Bodies::default();
        let mb = |n: usize| Bytes::from(vec![0u8; n << 20]);
        b.put("a".into(), mb(100));
        b.put("b".into(), mb(100));
        // 用过 a，挤掉的是 b
        assert!(b.get("a").is_some());
        b.put("c".into(), mb(100));
        assert!(b.get("b").is_none());
        assert!(b.get("a").is_some() && b.get("c").is_some());
        assert_eq!(b.bytes, 200 << 20);
        // 超过上限的不缓存
        b.put("huge".into(), mb(300));
        assert!(b.get("huge").is_none());
        assert_eq!(b.bytes, 200 << 20);
    }
}
