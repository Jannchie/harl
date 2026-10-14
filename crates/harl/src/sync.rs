//! 远程同步：把 `raw/` 镜像到一个对象存储（S3 / R2 / MinIO …）或一个目录（NAS、网盘），不需要服务器。全程增量。
//!
//! ```text
//! <prefix>/packs/<上传毫秒>-<hash>.pack        不可变、内容命名。键名带上传时间，拉的时候只列最近的
//! <prefix>/manifest/<machine>/seg/<偏移>.jsonl  这台机器 manifest 的一段字节：[偏移, 偏移+大小)。每次推只传新增的部分
//! <prefix>/manifest/<machine>/base.jsonl        段攒到 SEG_LIMIT 个时，把整份写成 base、删掉旧段，段数有上限
//! <prefix>/machines/<machine>.json
//! ```
//!
//! manifest 只追加，所以远端的段和 base 都是本地文件某个区间的原样拷贝；拉的时候从本地已有的长度接着取字节，
//! 拼出来和对方的文件逐字节一致，重叠、中途断掉都无所谓。
//!
//! 顺序保证读到的记录引用的 chunk 都在：推的时候先 pack 后 manifest；拉的时候先取 manifest 的新字节，再拉 pack，最后才落盘。
//! 本地 capture 会和拉下来的 pack 去重，新记录可能指向别的机器的 pack——那个 pack 本来就在远端，所以照样成立。
//!
//! 列 pack 的开销：进程里第一次推 / 拉时完整列一次（之后记在内存里），平时只按上传时间增量列最近的；
//! 机器之间时钟可能有偏差，所以增量列时往回多看 PACK_SKEW，并且每 FULL_LIST_EVERY 完整列一次兜底。
//! 拉到的新记录如果有 tree 不在本地，说明有 pack 漏了，马上完整列一次。

use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result, bail};
use futures::TryStreamExt;
use object_store::buffered::BufWriter;
use object_store::local::LocalFileSystem;
use object_store::path::Path as ObjPath;
use object_store::{ObjectMeta, ObjectStore, ObjectStoreExt, PutPayload};
use tokio::io::AsyncWriteExt;

use harl_core::store::{self, Store};
use rust_i18n::t;

use crate::config::RemoteConfig;

/// 本机 manifest 在远端的段数到这么多就合并成 base
const SEG_LIMIT: usize = if cfg!(test) { 4 } else { 64 };
/// 隔这么久完整列一次 pack，兜住时钟偏差之类漏掉的
const FULL_LIST_EVERY: Duration = Duration::from_secs(6 * 3600);
/// 增量列 pack 时往回多看这么久（毫秒）
const PACK_SKEW_MS: u64 = 3600 * 1000;

pub struct Remote {
    os: std::sync::Arc<dyn ObjectStore>,
    prefix: ObjPath,
    pub url: String,
    pub pull: bool,
    pub interval: Duration,
    /// 进程内记住的远端状态，省掉每轮的完整列表；进程重启后第一轮重新列
    state: tokio::sync::Mutex<State>,
}

#[derive(Default)]
struct State {
    /// 远端已有的 pack 名；None = 这个进程还没列过
    remote_packs: Option<HashSet<String>>,
    /// 见过的 pack 键里最大的上传时间（毫秒），增量列从这里往回 PACK_SKEW_MS 开始
    pack_mark_ms: u64,
    full_listed_at: Option<Instant>,
    /// 本机 manifest 在远端的 (长度, 段数)
    own: Option<(u64, usize)>,
    /// 上次推上去的本机描述 (长度, 修改时间)
    machine_pushed: Option<(u64, SystemTime)>,
    /// 这个进程拉过机器描述没有
    machines_pulled: bool,
}

/// `file://…`、或者干脆是个路径（`D:/nas/harl`、`/mnt/nas/harl`）：本地目录
fn local_dir(url: &str) -> Option<PathBuf> {
    if let Some(rest) = url.strip_prefix("file://") {
        // file:///C:/x → C:/x；file:///home/x → /home/x
        let rest = rest.strip_prefix('/').filter(|r| r.as_bytes().get(1) == Some(&b':')).unwrap_or(rest);
        return Some(PathBuf::from(rest));
    }
    (!url.contains("://")).then(|| PathBuf::from(url))
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// `<13 位毫秒>-<hash>.pack` → (毫秒, pack 名)
fn parse_pack_key(file: &str) -> Option<(u64, &str)> {
    let (ms, name) = file.split_once('-')?;
    Some((ms.parse().ok()?, name)).filter(|(_, n)| store::is_pack_name(n))
}

/// 一台机器的 manifest 在远端的样子
#[derive(Default)]
struct RemoteManifest {
    base: Option<ObjectMeta>,
    /// 偏移 → 段
    segs: BTreeMap<u64, ObjectMeta>,
}

impl RemoteManifest {
    fn len(&self) -> u64 {
        let base = self.base.as_ref().map_or(0, |b| b.size);
        self.segs.iter().map(|(off, m)| off + m.size).max().unwrap_or(0).max(base)
    }
}

/// 按 url 连上一个对象存储：`s3://…` 等交给 object_store（options 里没写的 aws_* 从环境变量补），
/// 本地路径 / `file://` 是一个目录。返回 (存储, 前缀)
fn connect(url: &str, options: &BTreeMap<String, String>) -> Result<(std::sync::Arc<dyn ObjectStore>, ObjPath)> {
    if let Some(dir) = local_dir(url) {
        std::fs::create_dir_all(&dir).with_context(|| t!("sync.create_dir", path = dir.display()))?;
        return Ok((std::sync::Arc::new(LocalFileSystem::new_with_prefix(&dir)?), ObjPath::default()));
    }
    let parsed = url::Url::parse(url).with_context(|| t!("sync.bad_url", url = url))?;
    let mut opts = options.clone();
    // 没写进配置的 aws_* 从环境变量补，密钥可以不落盘
    for (k, v) in std::env::vars() {
        let k = k.to_ascii_lowercase();
        if k.starts_with("aws_") {
            opts.entry(k).or_insert(v);
        }
    }
    let (os, prefix) =
        object_store::parse_url_opts(&parsed, opts).with_context(|| t!("sync.connect", url = url))?;
    Ok((os.into(), prefix))
}

impl Remote {
    pub fn new(cfg: &RemoteConfig) -> Result<Self> {
        let (os, prefix) = connect(&cfg.url, &cfg.options)?;
        Ok(Self {
            os,
            prefix,
            url: cfg.url.clone(),
            pull: cfg.pull,
            interval: Duration::from_secs(cfg.interval.max(5)),
            state: Default::default(),
        })
    }

    fn path(&self, parts: &[&str]) -> ObjPath {
        parts.iter().fold(self.prefix.clone(), |p, part| p.join(*part))
    }

    async fn list(&self, prefix: &ObjPath) -> Result<Vec<ObjectMeta>> {
        self.os
            .list(Some(prefix))
            .try_collect()
            .await
            .with_context(|| t!("sync.list_remote", prefix = prefix))
    }

    /// 本地文件流式传上去；大文件自动分段上传
    async fn upload_file(&self, src: &Path, key: ObjPath) -> Result<u64> {
        let mut f =
            tokio::fs::File::open(src).await.with_context(|| t!("sync.open", path = src.display()))?;
        let mut w = BufWriter::new(self.os.clone(), key);
        let n = tokio::io::copy(&mut f, &mut w).await?;
        w.shutdown().await?;
        Ok(n)
    }

    /// 下载到临时文件再改名，半个文件不会出现在 dst
    async fn download_file(&self, key: &ObjPath, dst: &Path) -> Result<u64> {
        let tmp = dst.with_file_name(format!(".pull-{}.tmp", dst.file_name().unwrap_or_default().to_string_lossy()));
        let mut out = tokio::fs::File::create(&tmp).await?;
        let mut stream = self.os.get(key).await?.into_stream();
        let mut n = 0;
        while let Some(b) = stream.try_next().await? {
            out.write_all(&b).await?;
            n += b.len() as u64;
        }
        out.sync_all().await?;
        drop(out);
        tokio::fs::rename(&tmp, dst).await?;
        Ok(n)
    }

    // ---- packs ----

    /// 列远端的 pack：full 时全部，否则只列上传时间在 mark − PACK_SKEW 之后的。顺手更新 state
    async fn list_packs(&self, st: &mut State, full: bool) -> Result<Vec<(String, ObjPath)>> {
        let dir = self.path(&["packs"]);
        let metas: Vec<ObjectMeta> = if full || st.remote_packs.is_none() {
            st.full_listed_at = Some(Instant::now());
            st.remote_packs = Some(HashSet::new());
            self.list(&dir).await?
        } else {
            let from = st.pack_mark_ms.saturating_sub(PACK_SKEW_MS);
            let offset = dir.clone().join(format!("{from:013}"));
            self.os
                .list_with_offset(Some(&dir), &offset)
                .try_collect()
                .await
                .context(t!("sync.list_packs"))?
        };
        let known = st.remote_packs.get_or_insert_default();
        let mut out = Vec::new();
        for m in metas {
            let Some((ms, name)) = m.location.filename().and_then(parse_pack_key) else {
                continue;
            };
            st.pack_mark_ms = st.pack_mark_ms.max(ms);
            known.insert(name.to_string());
            out.push((name.to_string(), m.location.clone()));
        }
        Ok(out)
    }

    /// 远端有、本地没有的 pack 拉下来登记进索引，返回 (个数, 字节)
    async fn fetch_packs(&self, store: &Mutex<Store>, listed: Vec<(String, ObjPath)>) -> Result<(usize, u64)> {
        let (local, packs_dir) = with(store, |s| Ok((s.pack_names()?, s.packs_dir())))?;
        let local: HashSet<String> = local.into_iter().collect();
        let (mut n, mut bytes) = (0, 0);
        for (name, key) in listed {
            if local.contains(&name) {
                continue;
            }
            bytes += self.download_file(&key, &packs_dir.join(&name)).await?;
            with(store, |s| s.add_pack(&name))?;
            n += 1;
        }
        Ok((n, bytes))
    }

    // ---- manifest ----

    async fn remote_manifest(&self, machine: &str) -> Result<RemoteManifest> {
        let mut out = RemoteManifest::default();
        for m in self.list(&self.path(&["manifest", machine])).await? {
            let parts: Vec<_> = m.location.parts().collect();
            let n = parts.len();
            match (n >= 2).then(|| (parts[n - 2].as_ref(), parts[n - 1].as_ref())) {
                Some((_, "base.jsonl")) => out.base = Some(m),
                Some(("seg", file)) => {
                    if let Some(off) = file.strip_suffix(".jsonl").and_then(|o| o.parse().ok()) {
                        out.segs.insert(off, m);
                    }
                }
                _ => {}
            }
        }
        Ok(out)
    }

    /// 从 from 开始把远端的字节接起来：先找覆盖这个位置的段，没有就从 base 取。遇到空洞就停
    async fn read_manifest_from(&self, rm: &RemoteManifest, from: u64) -> Result<Vec<u8>> {
        let (end, mut pos, mut out) = (rm.len(), from, Vec::new());
        while pos < end {
            let seg = rm.segs.range(..=pos).next_back().filter(|(off, m)| *off + m.size > pos);
            let (obj, start, stop) = match (seg, &rm.base) {
                (Some((off, m)), _) => (m, pos - off, m.size),
                (None, Some(b)) if b.size > pos => (b, pos, b.size),
                _ => {
                    crate::warn(t!("sync.manifest_gap", pos = pos));
                    break;
                }
            };
            out.extend_from_slice(&self.os.get_range(&obj.location, start..stop).await?);
            pos += stop - start;
        }
        Ok(out)
    }

    // ---- 推 / 拉 ----

    /// 本机的 pack、描述、manifest 新增的部分推上去
    pub async fn push(&self, store: &Mutex<Store>) -> Result<Pushed> {
        let mut st = self.state.lock().await;
        let (me, packs, packs_dir, manifest_path, machine_path) = with(store, |s| {
            Ok((s.machine().to_string(), s.pack_names()?, s.packs_dir(), s.manifest_path(), s.machine_path()))
        })?;
        let mut out = Pushed::default();

        if st.remote_packs.is_none() {
            self.list_packs(&mut st, true).await?;
        }
        for name in packs {
            if st.remote_packs.as_ref().is_some_and(|k| k.contains(&name)) {
                continue;
            }
            let ms = now_ms();
            out.bytes += self.upload_file(&packs_dir.join(&name), self.path(&["packs", &format!("{ms:013}-{name}")])).await?;
            out.packs += 1;
            st.pack_mark_ms = st.pack_mark_ms.max(ms);
            st.remote_packs.get_or_insert_default().insert(name);
        }

        if let Ok(meta) = std::fs::metadata(&machine_path) {
            let sig = (meta.len(), meta.modified()?);
            if st.machine_pushed != Some(sig) {
                let key = self.path(&["machines", &format!("{me}.json")]);
                self.os.put(&key, PutPayload::from(std::fs::read(&machine_path)?)).await?;
                st.machine_pushed = Some(sig);
            }
        }

        // manifest 最后传；只传到最后一个换行，正在追加的半行留到下次
        let mut bytes = match std::fs::read(&manifest_path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(e.into()),
        };
        bytes.truncate(bytes.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1));
        let (mut len, mut segs) = match st.own {
            Some(o) => o,
            None => {
                let rm = self.remote_manifest(&me).await?;
                (rm.len(), rm.segs.len())
            }
        };
        let local = bytes.len() as u64;
        if local < len {
            bail!(t!("sync.remote_longer", len = len, local = local));
        }
        if local > len {
            let key = self.path(&["manifest", &me, "seg", &format!("{len:020}.jsonl")]);
            self.os.put(&key, PutPayload::from(bytes[len as usize..].to_vec())).await?;
            out.manifest_bytes = local - len;
            (len, segs) = (local, segs + 1);
            if segs >= SEG_LIMIT {
                segs = self.compact(&me, bytes).await?;
            }
        }
        st.own = Some((len, segs));
        Ok(out)
    }

    /// 整份写成 base，再删掉被它覆盖的段；返回剩下的段数。删不掉（没有删除权限）也不影响正确性，只是段数不降
    async fn compact(&self, me: &str, bytes: Vec<u8>) -> Result<usize> {
        let len = bytes.len() as u64;
        self.os.put(&self.path(&["manifest", me, "base.jsonl"]), PutPayload::from(bytes)).await?;
        let rm = self.remote_manifest(me).await?;
        let mut left = 0;
        for (off, m) in &rm.segs {
            if off + m.size > len {
                left += 1;
            } else if let Err(e) = self.os.delete(&m.location).await {
                crate::warn(t!("sync.delete_segment_failed", error = e));
                left += 1;
            }
        }
        Ok(left)
    }

    /// 别的机器 manifest 的新字节、它们引用的 pack、机器描述拉下来
    pub async fn pull(&self, store: &Mutex<Store>) -> Result<Pulled> {
        let mut st = self.state.lock().await;
        let me = with(store, |s| Ok(s.machine().to_string()))?;
        let mut out = Pulled::default();

        let listing = self.os.list_with_delimiter(Some(&self.path(&["manifest"]))).await.context(t!("sync.list_manifests"))?;
        let mut fresh = Vec::new();
        for dir in listing.common_prefixes {
            let Some(id) = dir.filename().map(str::to_string) else { continue };
            if id == me || !store::is_machine_id(&id) {
                continue;
            }
            let at = with(store, |s| Ok(s.manifest_len(&id)))?;
            // 一台机器出错（比如正好在合并、段被删了）不耽误别的，下一轮再来
            let got = async {
                let rm = self.remote_manifest(&id).await?;
                if rm.len() <= at { Ok(Vec::new()) } else { self.read_manifest_from(&rm, at).await }
            }
            .await;
            match got {
                Ok(bytes) if !bytes.is_empty() => fresh.push((id, at, bytes)),
                Ok(_) => {}
                Err(e) => crate::warn(t!("sync.pull_manifest_failed", id = id, error = e : {:#})),
            }
        }

        let full_due = st.full_listed_at.is_none_or(|t| t.elapsed() > FULL_LIST_EVERY);
        if !fresh.is_empty() || full_due {
            let listed = self.list_packs(&mut st, full_due).await?;
            let (n, b) = self.fetch_packs(store, listed).await?;
            (out.packs, out.bytes) = (n, b);
            // 新记录引用的 tree 有不在本地的：增量列表漏了 pack（时钟偏差），完整列一次
            let missing = |s: &mut Store| {
                Ok(fresh.iter().flat_map(|(_, _, b)| Store::parse_records(b)).any(|r| !s.has_tree(&r)))
            };
            if !full_due && with(store, missing)? {
                let listed = self.list_packs(&mut st, true).await?;
                let (n, b) = self.fetch_packs(store, listed).await?;
                (out.packs, out.bytes) = (out.packs + n, out.bytes + b);
            }
        }

        for (id, at, bytes) in &fresh {
            with(store, |s| s.append_manifest(id, *at, bytes))?;
            out.manifest_bytes += bytes.len() as u64;
        }
        out.manifests = fresh.len();

        if !fresh.is_empty() || !st.machines_pulled {
            self.pull_machines(store, &me).await?;
            st.machines_pulled = true;
        }
        Ok(out)
    }

    async fn pull_machines(&self, store: &Mutex<Store>, me: &str) -> Result<()> {
        for m in self.list(&self.path(&["machines"])).await? {
            let Some(name) = m.location.filename() else {
                continue;
            };
            let Some(id) = name.strip_suffix(".json") else {
                continue;
            };
            if id == me || !store::is_machine_id(id) {
                continue;
            }
            let path = with(store, |s| Ok(s.root().join("raw/machines").join(name)))?;
            if std::fs::metadata(&path).ok().map(|l| l.len()) != Some(m.size) {
                let bytes = self.os.get(&m.location).await?.bytes().await?;
                if let Err(e) = with(store, |s| s.install_machine(id, &bytes)) {
                    crate::warn(t!("sync.skip_machine", name = name, error = e : {:#}));
                }
            }
        }
        Ok(())
    }

    /// 推，然后按配置拉
    pub async fn sync(&self, store: &Mutex<Store>, pull: bool) -> Result<Synced> {
        let pushed = self.push(store).await?;
        let pulled = if pull { Some(self.pull(store).await?) } else { None };
        Ok(Synced { pushed, pulled })
    }
}

/// 拿着 Store 的锁做一件短事；不要跨 await 持锁
fn with<T>(store: &Mutex<Store>, f: impl FnOnce(&mut Store) -> Result<T>) -> Result<T> {
    let mut s = store.lock().unwrap_or_else(|p| p.into_inner());
    f(&mut s)
}

#[derive(Default)]
pub struct Pushed {
    pub packs: usize,
    pub bytes: u64,
    pub manifest_bytes: u64,
}

#[derive(Default)]
pub struct Pulled {
    pub packs: usize,
    pub bytes: u64,
    pub manifests: usize,
    pub manifest_bytes: u64,
}

pub struct Synced {
    pub pushed: Pushed,
    pub pulled: Option<Pulled>,
}

impl Synced {
    pub fn is_noop(&self) -> bool {
        self.pushed.packs == 0
            && self.pushed.manifest_bytes == 0
            && self.pulled.as_ref().is_none_or(|p| p.packs == 0 && p.manifests == 0)
    }
}

impl fmt::Display for Synced {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let p = &self.pushed;
        let hb = crate::human_bytes;
        write!(f, "push  {} packs ({})  manifest +{}", p.packs, hb(p.bytes), hb(p.manifest_bytes))?;
        if let Some(p) = &self.pulled {
            write!(f, "  |  pull  {} packs ({})  {} manifests (+{})", p.packs, hb(p.bytes), p.manifests, hb(p.manifest_bytes))?;
        }
        Ok(())
    }
}

/// 后台同步线程：先同步一次，之后每 interval 一次，或者被 kick（刚 capture 了新快照）提前唤醒
pub fn run_loop(remote: Remote, store: std::sync::Arc<Mutex<Store>>, kick: mpsc::Receiver<()>) {
    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => return crate::warn(t!("sync.thread_failed", error = e)),
    };
    let secs = remote.interval.as_secs();
    let mode = if remote.pull { t!("sync.loop_push_pull", secs = secs) } else { t!("sync.loop_push_only", secs = secs) };
    println!("sync  {}  {mode}", remote.url);
    loop {
        match rt.block_on(remote.sync(&store, remote.pull)) {
            Ok(s) if !s.is_noop() => println!("sync  {s}"),
            Ok(_) => {}
            Err(e) => crate::warn(t!("sync.failed", error = e : {:#})),
        }
        match kick.recv_timeout(remote.interval) {
            // 一次 capture 可能连着几次；攒一下再推
            Ok(()) => {
                std::thread::sleep(Duration::from_secs(2));
                while kick.try_recv().is_ok() {}
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => std::thread::sleep(remote.interval),
        }
    }
}

/// `harl sync` 的入口：没配远端就给个能抄的例子
pub fn require(remote: Option<&RemoteConfig>, config_path: &Path) -> Result<Remote> {
    let Some(cfg) = remote else { bail!("{}", t!("sync.no_remote", path = config_path.display())) };
    Remote::new(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tmp(PathBuf);
    impl Tmp {
        fn new(name: &str) -> Self {
            let p = std::env::temp_dir().join(format!("harl-sync-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn remote(dir: &Path, pull: bool) -> Remote {
        let cfg = RemoteConfig { url: dir.to_string_lossy().into_owned(), pull, interval: 60, options: Default::default() };
        Remote::new(&cfg).unwrap()
    }

    /// a 一轮轮长、一轮轮推（中间段数超限会合并成 base），b 隔几轮拉一次：b 手里 a 的 manifest 始终和 a 的逐字节一致，能还原；
    /// a 换个进程（新的 Remote）再推不会重传任何东西
    #[test]
    fn incremental_roundtrip_with_compaction() {
        let (ta, tb, tr) = (Tmp::new("a"), Tmp::new("b"), Tmp::new("remote"));
        let sa = Mutex::new(Store::open(ta.0.clone()).unwrap());
        let sb = Mutex::new(Store::open(tb.0.clone()).unwrap());
        let (ra, rb) = (remote(&tr.0, false), remote(&tr.0, true));
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let src = "/h/.claude/projects/p/x.jsonl";
        let mut content = Vec::new();
        let a_id = sa.lock().unwrap().machine().to_string();

        for i in 0..11 {
            content.extend_from_slice(format!("{{\"i\":{i}}}
").as_bytes());
            {
                let mut s = sa.lock().unwrap();
                let mut batch = s.batch().unwrap();
                batch.put_snapshot("claude-code", src, chrono::Utc::now(), content.len() as u64, &content).unwrap();
                batch.commit().unwrap();
            }
            let pushed = rt.block_on(ra.push(&sa)).unwrap();
            assert_eq!(pushed.packs, 1, "round {i}");
            assert!(pushed.manifest_bytes > 0);
            if i % 3 == 2 {
                let pulled = rt.block_on(rb.pull(&sb)).unwrap();
                assert_eq!(pulled.manifests, 1);
                let want = std::fs::read(sa.lock().unwrap().manifest_path()).unwrap();
                assert_eq!(std::fs::read(sb.lock().unwrap().manifest_path_of(&a_id)).unwrap(), want, "round {i}");
            }
        }
        // 合并过：段数有上限，base 在
        let segs = std::fs::read_dir(tr.0.join("manifest").join(&a_id).join("seg")).unwrap().count();
        assert!(segs < SEG_LIMIT, "{segs} segs");
        assert!(tr.0.join("manifest").join(&a_id).join("base.jsonl").exists());

        let pulled = rt.block_on(rb.pull(&sb)).unwrap();
        assert_eq!(pulled.manifests, 1);
        let b = sb.lock().unwrap();
        let latest = b.latest().unwrap();
        let rec = &latest[&(a_id.clone(), src.to_string())];
        assert_eq!(b.reader().reconstruct(rec).unwrap(), content);
        drop(b);

        // 什么都没变：再拉一次是空的
        let again = rt.block_on(rb.pull(&sb)).unwrap();
        assert_eq!((again.manifests, again.packs), (0, 0));
        // 新进程：完整列一次后发现都在，不重传
        let fresh = rt.block_on(remote(&tr.0, false).push(&sa)).unwrap();
        assert_eq!((fresh.packs, fresh.manifest_bytes), (0, 0));
    }

    #[test]
    fn pack_keys() {
        assert_eq!(parse_pack_key("0001759380000000-00ff.pack"), Some((1759380000000, "00ff.pack")));
        assert_eq!(parse_pack_key("00ff.pack"), None);
        assert_eq!(parse_pack_key("1-../x.pack"), None);
        // 补零到 13 位后字典序 = 时间序
        assert!(format!("{:013}", 999_999_999_999u64) < format!("{:013}", 1_000_000_000_000u64));
    }
}
