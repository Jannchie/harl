//! `harl serve`：本地 HTTP。`/api/*` 是 JSON，其余路径给 web 的构建产物（编译时嵌进二进制），
//! 没有的路径回退到 index.html 让前端路由接管。
//!
//! Store 不是 Sync（rusqlite），所以放 Mutex 里、每个请求在 blocking 线程上拿锁干活。本地单人用，够了。

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, NaiveDate, Utc};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};

use harl_core::event::Kind;
use harl_core::normalize::{self, Provider};
use harl_core::store::{self, Reader, Record, Store};

use crate::summary;

#[derive(RustEmbed)]
#[folder = "../../web/dist"]
struct Web;

type Shared = Arc<Mutex<Store>>;

#[derive(Serialize)]
struct Status {
    data_dir: String,
    machine: String,
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

/// 开头 TITLE_SCAN 行里第一个 turn_started 的第一行文字，截到 TITLE_LEN 字符
const TITLE_SCAN: usize = 40;
const TITLE_LEN: usize = 120;

fn title(reader: &mut Reader<'_>, provider: &Provider, rec: &Record) -> Result<Option<String>> {
    let lines = reader.lines_prefix(rec, TITLE_SCAN)?;
    let mut found = None;
    normalize::normalize(provider, &rec.source, &lines, |ev| {
        if found.is_none() && ev.kind == Kind::TurnStarted {
            found = ev.text.as_deref().and_then(|t| t.lines().find(|l| !l.trim().is_empty())).map(|l| {
                let l = l.trim();
                match l.char_indices().nth(TITLE_LEN) {
                    Some((i, _)) => format!("{}…", &l[..i]),
                    None => l.to_string(),
                }
            });
        }
        Ok(())
    })?;
    Ok(found)
}

#[derive(Deserialize)]
struct SummaryQuery {
    /// 客户端时区偏移，分钟，东为正（JS 的 -getTimezoneOffset()）
    #[serde(default)]
    tz: i32,
    /// 本地日期 YYYY-MM-DD，两端都含；不给就不限
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
}

async fn summary(State(state): State<Shared>, Query(q): Query<SummaryQuery>) -> Result<Response, ApiError> {
    let range = summary::Range { from: q.from, to: q.to };
    let s = with_store(state, move |store| summary::build(store, q.tz, range)).await?;
    Ok(Json(s).into_response())
}

async fn sessions(State(state): State<Shared>) -> Result<Json<Vec<Session>>, ApiError> {
    let list = with_store(state, |store| {
        let mut reader = store.reader();
        let mut out: Vec<Session> = store
            .latest()?
            .into_values()
            .map(|r| {
                let provider = normalize::by_name(&r.provider);
                let (session, parent_session) = match provider {
                    Some(p) => normalize::identity(p, &r.source),
                    None => (normalize::file_stem(&r.source).to_string(), None),
                };
                // 标题拿不到（chunk 缺失等）不影响列表
                let title = provider.and_then(|p| title(&mut reader, p, &r).ok().flatten());
                Session {
                    id: r.tree,
                    provider: r.provider,
                    machine: r.machine,
                    source: r.source,
                    session,
                    parent_session,
                    title,
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

pub fn router(store: Store) -> Router {
    Router::new()
        .route("/api/status", get(status))
        .route("/api/summary", get(summary))
        .route("/api/sessions", get(sessions))
        .route("/api/sessions/{id}/events", get(events))
        .route("/api/sessions/{id}/raw", get(raw))
        .fallback(web)
        .with_state(Arc::new(Mutex::new(store)))
}

pub fn run(store: Store, host: &str, port: u16, open: bool) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let addr: SocketAddr = format!("{host}:{port}").parse().context("监听地址不合法")?;
        let listener = tokio::net::TcpListener::bind(addr).await.with_context(|| format!("监听 {addr}"))?;
        let url = format!("http://{addr}");
        println!("harl serve  {url}");
        if open {
            let _ = ::open::that(&url);
        }
        axum::serve(listener, router(store)).await?;
        Ok(())
    })
}
