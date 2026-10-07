//! `harl serve`：本地 HTTP。`/api/*` 是 JSON，其余路径给 web 的构建产物（编译时嵌进二进制），
//! 没有的路径回退到 index.html 让前端路由接管。
//!
//! Store 不是 Sync（rusqlite），所以放 Mutex 里、每个请求在 blocking 线程上拿锁干活。本地单人用，够了。
//! 后台线程监听会话目录，有变化就增量 capture：扫盘不拿锁，只有写快照时拿锁（增量时很快）。

use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, NaiveDate, Utc};
use notify::{EventKind, RecursiveMode, Watcher};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};

use harl_core::normalize::{self, homes};
use harl_core::store::{self, Record, Store};

use crate::head::head_for;
use crate::summary;

#[derive(RustEmbed)]
#[folder = "../../web/dist"]
struct Web;

type Shared = Arc<Mutex<Store>>;

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

/// 在 blocking 线程上拿着 Store 干活
async fn with_store<T: Send + 'static>(
    state: Shared,
    f: impl FnOnce(&Store) -> Result<T> + Send + 'static,
) -> Result<T, ApiError> {
    let out = tokio::task::spawn_blocking(move || {
        let store = state.lock().unwrap_or_else(|p| p.into_inner());
        f(&store)
    })
    .await
    .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(out?)
}

async fn status(State(state): State<Shared>) -> Result<Json<Status>, ApiError> {
    let s = with_store(state, |store| {
        let records = store.records()?;
        let n_records = records.len();
        let latest = store::latest_of(records);
        let stats = store.stats()?;
        Ok(Status {
            data_dir: store.root().display().to_string(),
            machine: store.machine().to_string(),
            machines: store.machines()?.into_values().map(|m| (m.id, m.hostname)).collect(),
            sources: latest.len(),
            records: n_records,
            logical_bytes: latest.values().map(|r| r.size).sum(),
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

async fn summary(State(state): State<Shared>, Query(q): Query<SummaryQuery>) -> Result<Response, ApiError> {
    let range = summary::Range { from: q.from, to: q.to, since: q.since };
    let filter = summary::Filter { providers: csv(q.provider), models: csv(q.model), projects: projects(q.project) };
    let s = with_store(state, move |store| summary::build(store, q.tz, range, filter)).await?;
    Ok(Json(s).into_response())
}

async fn sessions(State(state): State<Shared>) -> Result<Json<Vec<Session>>, ApiError> {
    let list = with_store(state, |store| {
        let mut reader = store.reader();
        let mut out: Vec<Session> = normalize::live(store.latest()?.into_values())
            .into_iter()
            .map(|r| {
                let provider = normalize::by_name(&r.provider);
                let session = match provider {
                    Some(p) => normalize::identity(p, &r.source).0,
                    None => normalize::file_stem(&r.source).to_string(),
                };
                // 标题拿不到（chunk 缺失等）不影响列表
                let h = provider.and_then(|p| head_for(store, &mut reader, p, &r)).unwrap_or_default();
                Session {
                    id: r.tree,
                    provider: r.provider,
                    machine: r.machine,
                    source: r.source,
                    session,
                    parent_session: h.parent,
                    title: h.title,
                    cwd: h.cwd,
                    size: r.size,
                    lines: r.lines,
                    mtime: r.mtime,
                    captured_at: r.captured_at,
                }
            })
            .collect();
        out.sort_by_key(|s| std::cmp::Reverse(s.captured_at));
        Ok(out)
    })
    .await?;
    Ok(Json(list))
}

fn find(store: &Store, id: &str) -> Result<Record, ApiError> {
    store
        .record_by_tree(id)
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("没有 tree 为 {id} 的快照")))
}

async fn events(State(state): State<Shared>, Path(id): Path<String>) -> Result<Response, ApiError> {
    // 直接序列化成 JSON 数组的字节，省得先攒一个 Vec<Event> 再序列化一遍
    let body = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, ApiError> {
        let store = state.lock().unwrap_or_else(|p| p.into_inner());
        let rec = find(&store, &id)?;
        let provider = normalize::by_name(&rec.provider)
            .with_context(|| format!("没有 {} 的 normalizer", rec.provider))
            .map_err(ApiError::from)?;
        let lines = store.reader().lines(&rec).map_err(ApiError::from)?;
        let mut out = vec![b'['];
        let mut first = true;
        normalize::normalize(provider, &rec.source, &lines, |ev| {
            if !first {
                out.push(b',');
            }
            first = false;
            serde_json::to_writer(&mut out, &ev)?;
            Ok(())
        })
        .map_err(ApiError::from)?;
        out.push(b']');
        Ok(out)
    })
    .await
    .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))??;
    Ok(([(header::CONTENT_TYPE, "application/json")], Bytes::from(body)).into_response())
}

async fn raw(State(state): State<Shared>, Path(id): Path<String>) -> Result<Response, ApiError> {
    let bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, ApiError> {
        let store = state.lock().unwrap_or_else(|p| p.into_inner());
        let rec = find(&store, &id)?;
        store.reader().reconstruct(&rec).map_err(ApiError::from)
    })
    .await
    .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))??;
    Ok(([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], Bytes::from(bytes)).into_response())
}

/// 嵌入的静态文件；没有就回 index.html（前端路由）
async fn web(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let (path, file) = match Web::get(path) {
        Some(f) if !path.is_empty() => (path, f),
        _ => match Web::get("index.html") {
            Some(f) => ("index.html", f),
            None => {
                return (StatusCode::NOT_FOUND, "web 没有构建产物：先在 web/ 里 pnpm build，再重新编译 harl")
                    .into_response();
            }
        },
    };
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    ([(header::CONTENT_TYPE, mime.as_ref())], file.data).into_response()
}

pub fn router(state: Shared) -> Router {
    Router::new()
        .route("/api/status", get(status))
        .route("/api/summary", get(summary))
        .route("/api/sessions", get(sessions))
        .route("/api/sessions/{id}/events", get(events))
        .route("/api/sessions/{id}/raw", get(raw))
        .fallback(web)
        .with_state(state)
}

/// 变化后安静这么久才 capture：会话文件是一行行追加的，一轮对话会连续写很多次
const QUIET: Duration = Duration::from_secs(2);
/// 一直在写（长任务）时最多攒这么久也 capture 一次
const MAX_DELAY: Duration = Duration::from_secs(15);
/// 收不到变化通知的根目录（WSL 的 9P 共享、WSL 里挂的 Windows 盘）隔这么久轮询一次
const POLL: Duration = Duration::from_secs(30);

fn capture_now(state: &Shared, roots: &[PathBuf]) {
    let files = crate::discover(roots);
    let res = {
        let mut store = state.lock().unwrap_or_else(|p| p.into_inner());
        crate::capture(&mut store, &files)
    };
    match res {
        Ok(c) if c.wrote() => {
            println!("capture  {c}");
            warm(state, c.done.records);
        }
        Ok(_) => {}
        Err(e) => eprintln!("warning: capture 失败: {e:#}"),
    }
}

/// 全部最新快照；启动时预热用
fn live_records(state: &Shared) -> Vec<Record> {
    let store = state.lock().unwrap_or_else(|p| p.into_inner());
    match store.latest() {
        Ok(l) => normalize::live(l.into_values()),
        Err(e) => {
            eprintln!("warning: 预热缓存失败: {e:#}");
            Vec::new()
        }
    }
}

/// 把这些快照的 stats 和 head 预先算好进缓存，页面打开时就不用等。
/// 每个会话单独拿锁，中间让 API 请求插队；已缓存的只是一次查表
fn warm(state: &Shared, recs: Vec<Record>) {
    for rec in recs {
        let Some(provider) = normalize::by_name(&rec.provider) else { continue };
        let store = state.lock().unwrap_or_else(|p| p.into_inner());
        if let Err(e) = summary::stats_for(&store, &rec) {
            eprintln!("warning: 统计 {} 失败: {e:#}", rec.source);
        }
        head_for(&store, &mut store.reader(), provider, &rec);
    }
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

fn watch(state: Shared, roots: Vec<PathBuf>) {
    let (tx, rx) = mpsc::channel();
    // 跨 WSL / Windows 的根收不到系统通知，改用轮询；两种监听发到同一个 channel
    let native = notify::recommended_watcher(on_change(tx.clone()));
    let poller = notify::PollWatcher::new(on_change(tx), notify::Config::default().with_poll_interval(POLL));
    let (mut native, mut poller) = match (native, poller) {
        (Ok(n), Ok(p)) => (n, p),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("warning: 无法创建文件监听，不会自动 capture: {e}");
            return;
        }
    };
    for r in roots.iter().filter(|r| r.is_dir()) {
        let w: &mut dyn Watcher = if homes::is_foreign(r) { &mut poller } else { &mut native };
        if let Err(e) = w.watch(r, RecursiveMode::Recursive) {
            eprintln!("warning: 监听 {} 失败: {e}", r.display());
        }
    }

    capture_now(&state, &roots);
    warm(&state, live_records(&state));
    // 阻塞等第一个变化，然后等到安静 QUIET 或者累计 MAX_DELAY 再 capture
    while rx.recv().is_ok() {
        let start = Instant::now();
        while start.elapsed() < MAX_DELAY && rx.recv_timeout(QUIET).is_ok() {}
        capture_now(&state, &roots);
    }
}

/// `watch_roots` 为 Some 时起后台 capture 线程
pub fn run(store: Store, host: &str, port: u16, open: bool, watch_roots: Option<Vec<PathBuf>>) -> Result<()> {
    let state: Shared = Arc::new(Mutex::new(store));
    if let Some(roots) = watch_roots {
        let state = state.clone();
        std::thread::spawn(move || watch(state, roots));
    }
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let addr: SocketAddr = format!("{host}:{port}").parse().context("监听地址不合法")?;
        let listener = tokio::net::TcpListener::bind(addr).await.with_context(|| format!("监听 {addr}"))?;
        let url = format!("http://{addr}");
        println!("harl serve  {url}");
        if open {
            let _ = ::open::that(&url);
        }
        axum::serve(listener, router(state)).await?;
        Ok(())
    })
}
