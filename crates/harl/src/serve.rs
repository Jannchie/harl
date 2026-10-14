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
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use chrono::{DateTime, NaiveDate, Utc};
use notify::{EventKind, RecursiveMode, Watcher};
use rust_embed::RustEmbed;
use rust_i18n::t;
use serde::{Deserialize, Serialize};

use harl_core::normalize::{self, homes};
use harl_core::store::{self, Record, Store};

use crate::config::UpdateConfig;
use crate::head::head_for;
use crate::summary;
use crate::sync::{self, Remote};
use crate::update::{self, Updater, VersionInfo};

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
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, t!("serve.no_snapshot", tree = id).into_owned()))
}

async fn events(State(state): State<Shared>, Path(id): Path<String>) -> Result<Response, ApiError> {
    // 直接序列化成 JSON 数组的字节，省得先攒一个 Vec<Event> 再序列化一遍
    let body = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, ApiError> {
        let store = state.lock().unwrap_or_else(|p| p.into_inner());
        let rec = find(&store, &id)?;
        let provider = normalize::by_name(&rec.provider)
            .with_context(|| t!("main.no_normalizer", provider = rec.provider))
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

/// 下载并换上新版本，回应之后重启自己；页面轮询 /api/version 等新版本起来
async fn do_update(
    State(state): State<Shared>,
    Extension(up): Extension<Arc<Updater>>,
    headers: HeaderMap,
) -> Result<Json<Updated>, ApiError> {
    same_origin(&headers)?;
    let version = up.update(true).await.map_err(|e| ApiError(StatusCode::CONFLICT, format!("{e:#}")))?;
    println!("update  {}", t!("serve.updated_restarting", version = version));
    tokio::spawn(async move {
        // 让响应先发出去
        tokio::time::sleep(Duration::from_millis(500)).await;
        restart_quiet(&state, up.exe())
    });
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

pub fn router(state: Shared, updater: Arc<Updater>) -> Router {
    Router::new()
        .route("/api/version", get(version))
        .route("/api/update", post(do_update))
        .route("/api/status", get(status))
        .route("/api/summary", get(summary))
        .route("/api/sessions", get(sessions))
        .route("/api/sessions/{id}/events", get(events))
        .route("/api/sessions/{id}/raw", get(raw))
        .fallback(web)
        .layer(Extension(updater))
        .with_state(state)
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
            if after.warm {
                warm(state, c.done.records);
            }
        }
        Ok(_) => {}
        Err(e) => crate::warn(t!("serve.capture_failed", error = e : {:#})),
    }
}

/// 全部最新快照；启动时预热用
fn live_records(state: &Shared) -> Vec<Record> {
    let store = state.lock().unwrap_or_else(|p| p.into_inner());
    match store.latest() {
        Ok(l) => normalize::live(l.into_values()),
        Err(e) => {
            crate::warn(t!("serve.warm_failed", error = e : {:#}));
            Vec::new()
        }
    }
}

/// 把这些快照的 stats 和 head 预先算好进缓存，页面打开时就不用等。
/// 每个会话单独拿锁，中间让 API 请求插队；已缓存的只是一次查表
fn warm(state: &Shared, recs: Vec<Record>) {
    for rec in recs {
        let Some(provider) = normalize::by_name(&rec.provider) else {
            continue;
        };
        let store = state.lock().unwrap_or_else(|p| p.into_inner());
        if let Err(e) = summary::stats_for(&store, &rec) {
            crate::warn(t!("serve.stats_failed", source = rec.source, error = e : {:#}));
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

/// capture 写了新快照之后做什么
struct AfterCapture {
    /// 叫醒同步线程，尽快推上去
    kick: Option<mpsc::Sender<()>>,
    /// 预先算好统计和会话头（有 UI 时才值得）
    warm: bool,
}

fn watch(state: Shared, roots: Vec<PathBuf>, after: AfterCapture) {
    let (tx, rx) = mpsc::channel();
    // 跨 WSL / Windows 的根收不到系统通知，改用轮询；两种监听发到同一个 channel
    let native = notify::recommended_watcher(on_change(tx.clone()));
    let poller = notify::PollWatcher::new(on_change(tx), notify::Config::default().with_poll_interval(POLL));
    let (mut native, mut poller) = match (native, poller) {
        (Ok(n), Ok(p)) => (n, p),
        (Err(e), _) | (_, Err(e)) => {
            crate::warn(t!("serve.watcher_failed", error = e));
            return;
        }
    };
    for r in roots.iter().filter(|r| r.is_dir()) {
        let w: &mut dyn Watcher = if homes::is_foreign(r) { &mut poller } else { &mut native };
        if let Err(e) = w.watch(r, RecursiveMode::Recursive) {
            crate::warn(t!("serve.watch_failed", path = r.display(), error = e));
        }
    }

    capture_now(&state, &roots, &after);
    if after.warm {
        warm(&state, live_records(&state));
    }
    // 阻塞等第一个变化，然后等到安静 QUIET 或者累计 MAX_DELAY 再 capture
    while rx.recv().is_ok() {
        let start = Instant::now();
        while start.elapsed() < MAX_DELAY && rx.recv_timeout(QUIET).is_ok() {}
        capture_now(&state, &roots, &after);
    }
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
    let after = AfterCapture { kick, warm: opts.http.is_some() };
    let updater = Arc::new(Updater::new(opts.update.check));
    let auto = opts.update.auto && updater.can_self_update();
    let Some(Http { host, port, open }) = opts.http else {
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
        match opts.watch_roots {
            Some(roots) => watch(state, roots, after),
            None => loop {
                std::thread::park();
            },
        }
        return Ok(());
    };
    if let Some(roots) = opts.watch_roots {
        let state = state.clone();
        std::thread::spawn(move || watch(state, roots, after));
    }
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let addr: SocketAddr = format!("{host}:{port}").parse().context(t!("serve.bad_addr"))?;
        let listener = bind(addr).await?;
        let url = format!("http://{addr}");
        println!("harl serve  {url}");
        if open {
            let _ = ::open::that(&url);
        }
        if auto {
            tokio::spawn(auto_update(updater.clone(), state.clone()));
        }
        axum::serve(listener, router(state, updater)).await?;
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
