//! 新版本检测与自更新：查 GitHub Releases 的最新版本，下载本平台的包，校验 sha256，替换 harl / harlw，再重启。
//!
//! 发布包（见 .github/workflows/release.yml）：`harl-<target>.zip`（Windows）/ `harl-<target>.tar.gz`，
//! 里面是 harl 和 harlw；旁边一个 `<包名>.sha256`。
//!
//! 替换：新文件先写成 `<名>.new`；Windows 不能覆盖正在运行的 exe，但能改名，所以旧的先挪成 `<名>.old`（下次启动时删掉），
//! Unix 直接 rename 覆盖。重启：有守护者（HARL_SUPERVISED）就以 EXIT_RESTART 退出交给它；没有就自己拉起新进程再退出。

use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::common::{self, EXIT_RESTART, SUPERVISED_ENV};

pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
const TARGET: &str = env!("HARL_TARGET");
/// 查到结果后这么久内不再查；查失败（离线）的话隔得短一些
const FRESH: Duration = Duration::from_secs(6 * 3600);
const RETRY: Duration = Duration::from_secs(15 * 60);
const BINARIES: [&str; 2] = ["harl", "harlw"];

/// `https://github.com/<owner>/<repo>` → `<owner>/<repo>`
fn repo() -> &'static str {
    env!("CARGO_PKG_REPOSITORY").trim_end_matches('/').trim_start_matches("https://github.com/")
}

fn asset_name() -> String {
    format!("harl-{TARGET}.{}", if cfg!(windows) { "zip" } else { "tar.gz" })
}

#[derive(Deserialize, Clone)]
struct GhRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    body: Option<String>,
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Deserialize, Clone)]
struct GhAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Serialize, Clone)]
pub struct Latest {
    pub version: String,
    pub url: String,
    pub notes: String,
    pub published_at: Option<String>,
}

#[derive(Serialize)]
pub struct VersionInfo {
    pub current: &'static str,
    pub latest: Option<Latest>,
    pub update_available: bool,
    /// 这个进程能不能自己更新；不能时 reason 说明为什么
    pub can_update: bool,
    pub reason: Option<String>,
}

/// `v1.2.3` / `1.2.3-rc.1` → [1, 2, 3]；比较只看数字部分
fn numeric(v: &str) -> Option<Vec<u64>> {
    let core = v.trim_start_matches('v').split(['-', '+']).next()?;
    core.split('.').map(|p| p.parse().ok()).collect()
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    matches!((numeric(candidate), numeric(current)), (Some(a), Some(b)) if a > b)
}

fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(concat!("harl/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(120))
        .build()?)
}

/// 最新的正式版；仓库还没有发布过（404）就是 None
async fn fetch_latest() -> Result<Option<GhRelease>> {
    let url = format!("https://api.github.com/repos/{}/releases/latest", repo());
    let resp = client()?.get(&url).header("accept", "application/vnd.github+json").send().await?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let resp = resp.error_for_status().context("查询 GitHub Releases")?;
    Ok(Some(serde_json::from_slice(&resp.bytes().await?)?))
}

async fn download(url: &str) -> Result<Vec<u8>> {
    let resp = client()?.get(url).send().await?.error_for_status().with_context(|| format!("下载 {url}"))?;
    Ok(resp.bytes().await?.to_vec())
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// 包里的 harl / harlw，按文件名找（不管包里有没有一层目录）
fn extract(archive: &[u8]) -> Result<Vec<(&'static str, Vec<u8>)>> {
    let want = |path: &str| {
        let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
        BINARIES.into_iter().find(|b| file == format!("{b}{}", std::env::consts::EXE_SUFFIX))
    };
    let mut out = Vec::new();
    if cfg!(windows) {
        let mut zip = zip::ZipArchive::new(Cursor::new(archive))?;
        for i in 0..zip.len() {
            let mut f = zip.by_index(i)?;
            if let Some(name) = want(f.name()) {
                let mut buf = Vec::new();
                f.read_to_end(&mut buf)?;
                out.push((name, buf));
            }
        }
    } else {
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
        for entry in tar.entries()? {
            let mut entry = entry?;
            let path = entry.path()?.to_string_lossy().into_owned();
            if let Some(name) = want(&path) {
                let mut buf = Vec::new();
                entry.read_to_end(&mut buf)?;
                out.push((name, buf));
            }
        }
    }
    if !out.iter().any(|(n, _)| *n == "harl") {
        bail!("发布包里没有 harl 可执行文件");
    }
    Ok(out)
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// 新二进制放到 dir 下，替换同名的旧文件
fn replace(dir: &Path, files: &[(&str, Vec<u8>)]) -> Result<()> {
    for (name, bytes) in files {
        let target = dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
        let new = with_suffix(&target, ".new");
        std::fs::write(&new, bytes).with_context(|| format!("写 {}", new.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o755))?;
        }
        if cfg!(windows) && target.exists() {
            let old = with_suffix(&target, ".old");
            let _ = std::fs::remove_file(&old);
            std::fs::rename(&target, &old).with_context(|| format!("挪开 {}", target.display()))?;
        }
        std::fs::rename(&new, &target).with_context(|| format!("换上 {}", target.display()))?;
    }
    Ok(())
}

/// 删掉上次更新留下的 `.old`（Windows 上那时还在运行，删不掉）
pub fn cleanup() {
    let Ok(exe) = std::env::current_exe() else { return };
    let Some(dir) = exe.parent() else { return };
    for b in BINARIES {
        let _ = std::fs::remove_file(dir.join(format!("{b}{}.old", std::env::consts::EXE_SUFFIX)));
    }
}

/// 下载、校验、替换。返回装上的版本
async fn install(rel: &GhRelease, dir: &Path) -> Result<String> {
    let name = asset_name();
    let find = |n: &str| rel.assets.iter().find(|a| a.name == n).map(|a| a.browser_download_url.clone());
    let url = find(&name).with_context(|| format!("{} 没有本平台的包 {name}", rel.tag_name))?;
    let sum_url = find(&format!("{name}.sha256")).with_context(|| format!("{} 没有 {name}.sha256", rel.tag_name))?;
    let archive = download(&url).await?;
    let want = String::from_utf8(download(&sum_url).await?)?;
    let want = want.split_whitespace().next().unwrap_or_default().to_ascii_lowercase();
    let got = sha256_hex(&archive);
    if got != want {
        bail!("{name} 的 sha256 对不上（{got} ≠ {want}），已放弃");
    }
    let files = extract(&archive)?;
    let dir = dir.to_path_buf();
    tokio::task::spawn_blocking(move || replace(&dir, &files)).await??;
    Ok(rel.tag_name.trim_start_matches('v').to_string())
}

/// 用新二进制重启自己。不返回
pub fn restart(exe: &Path) -> ! {
    if std::env::var_os(SUPERVISED_ENV).is_some() {
        std::process::exit(EXIT_RESTART);
    }
    // 新进程会等端口空出来（见 serve 的 bind 重试）
    match std::process::Command::new(exe).args(std::env::args_os().skip(1)).spawn() {
        Ok(_) => std::process::exit(0),
        Err(e) => {
            eprintln!("error: 重启失败，请手动重新运行 harl: {e}");
            std::process::exit(1)
        }
    }
}

/// serve 持有一个：缓存最新版本，防止并发更新
pub struct Updater {
    check: bool,
    /// 启动时的可执行文件路径：更新后 Windows 上 current_exe 可能指向改了名的旧文件
    exe: PathBuf,
    cache: tokio::sync::Mutex<Option<(Instant, Option<GhRelease>)>>,
    busy: AtomicBool,
}

impl Updater {
    pub fn new(check: bool) -> Self {
        Self {
            check,
            exe: std::env::current_exe().unwrap_or_default(),
            cache: Default::default(),
            busy: AtomicBool::new(false),
        }
    }

    pub fn exe(&self) -> &Path {
        &self.exe
    }

    async fn latest(&self, force: bool) -> Option<GhRelease> {
        let mut cache = self.cache.lock().await;
        if let Some((at, rel)) = &*cache {
            let ttl = if rel.is_some() { FRESH } else { RETRY };
            if !force && at.elapsed() < ttl {
                return rel.clone();
            }
        }
        let rel = match fetch_latest().await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("warning: 检查新版本失败: {e:#}");
                None
            }
        };
        *cache = Some((Instant::now(), rel.clone()));
        rel
    }

    fn blocker(&self) -> Option<String> {
        if cfg!(debug_assertions) {
            Some("开发构建不自动更新".into())
        } else if self.exe.parent().is_none_or(|d| std::fs::metadata(d).is_ok_and(|m| m.permissions().readonly())) {
            Some(format!("{} 不可写", self.exe.display()))
        } else {
            None
        }
    }

    pub async fn info(&self, force: bool) -> VersionInfo {
        let rel = if self.check || force { self.latest(force).await } else { None };
        let latest = rel.as_ref().map(|r| Latest {
            version: r.tag_name.trim_start_matches('v').to_string(),
            url: r.html_url.clone(),
            notes: r.body.clone().unwrap_or_default(),
            published_at: r.published_at.clone(),
        });
        let update_available = latest.as_ref().is_some_and(|l| is_newer(&l.version, CURRENT));
        let reason = self.blocker().or_else(|| {
            rel.as_ref()
                .filter(|r| !r.assets.iter().any(|a| a.name == asset_name()))
                .map(|_| format!("这个版本没有 {TARGET} 的包"))
        });
        VersionInfo { current: CURRENT, latest, update_available, can_update: reason.is_none(), reason }
    }

    /// 装最新版，返回装上的版本；调用方随后 `restart`
    pub async fn update(&self) -> Result<String> {
        if let Some(r) = self.blocker() {
            bail!(r);
        }
        if self.busy.swap(true, Ordering::SeqCst) {
            bail!("已经在更新了");
        }
        let res = async {
            let rel = self.latest(true).await.context("没查到可用的发布")?;
            let v = rel.tag_name.trim_start_matches('v');
            if !is_newer(v, CURRENT) {
                bail!("已经是最新版本 {CURRENT}");
            }
            install(&rel, self.exe.parent().context("可执行文件没有所在目录")?).await
        }
        .await;
        if res.is_err() {
            self.busy.store(false, Ordering::SeqCst);
        }
        res
    }
}

/// `harl update`
pub fn cli(check_only: bool) -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let up = Updater::new(true);
    let info = rt.block_on(up.info(true));
    let Some(latest) = &info.latest else {
        println!("当前 {CURRENT}；没查到发布（https://github.com/{}/releases）", repo());
        return Ok(());
    };
    if !info.update_available {
        println!("已经是最新版本 {CURRENT}");
        return Ok(());
    }
    println!("当前 {CURRENT}，最新 {}  {}", latest.version, latest.url);
    if check_only {
        return Ok(());
    }
    let v = rt.block_on(up.update())?;
    println!("已更新到 {v}");
    if common::data_dir(None).is_some_and(|d| crate::service::is_running(&d)) {
        println!("后台服务还在跑旧版本：harl service stop，再 harl service start 换成新版本");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compare() {
        assert!(is_newer("0.2.0", "0.1.9"));
        assert!(is_newer("v1.0.0", "0.9.9"));
        assert!(is_newer("0.10.0", "0.9.0"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0-rc.1", "0.1.0"));
        assert!(!is_newer("garbage", "0.1.0"));
    }

    /// 造一个和 release.yml 打出来一样形状的包：解出 harl / harlw，替换掉目录里的旧文件
    #[test]
    fn extract_and_replace() {
        use std::io::Write;
        let exe = |b: &str| format!("{b}{}", std::env::consts::EXE_SUFFIX);
        let archive = if cfg!(windows) {
            let mut buf = Cursor::new(Vec::new());
            let mut zip = zip::ZipWriter::new(&mut buf);
            for (name, body) in [(exe("harl"), b"new-harl".as_slice()), (exe("harlw"), b"new-harlw"), ("README.md".into(), b"x")] {
                zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
                zip.write_all(body).unwrap();
            }
            zip.finish().unwrap();
            buf.into_inner()
        } else {
            let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
            for (name, body) in [(exe("harl"), b"new-harl".as_slice()), (exe("harlw"), b"new-harlw")] {
                let mut h = tar::Header::new_gnu();
                h.set_size(body.len() as u64);
                h.set_mode(0o755);
                h.set_cksum();
                tar.append_data(&mut h, format!("harl-x/{name}"), body).unwrap();
            }
            tar.into_inner().unwrap().finish().unwrap()
        };
        let files = extract(&archive).unwrap();
        assert_eq!(files.len(), 2);

        let dir = std::env::temp_dir().join(format!("harl-update-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(exe("harl")), b"old").unwrap();
        replace(&dir, &files).unwrap();
        assert_eq!(std::fs::read(dir.join(exe("harl"))).unwrap(), b"new-harl");
        assert_eq!(std::fs::read(dir.join(exe("harlw"))).unwrap(), b"new-harlw");
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn repo_from_manifest() {
        assert_eq!(repo(), "Jannchie/harl");
    }
}
