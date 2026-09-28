//! 同一台机器上可能有几个 home：Windows 上跑着的 WSL 发行版各有自己的 `/home/*`，
//! WSL 里又能看到 Windows 的 `C:\Users\*`。agent 装在哪边，会话就写在哪边的 home 下。

use std::fs;
use std::path::{Path, PathBuf};

/// 本机 home 加上跨 WSL / Windows 能看到的其他 home；不保证都存在
pub fn all(home: &Path) -> Vec<PathBuf> {
    let mut out = vec![home.to_path_buf()];
    if cfg!(windows) {
        out.extend(wsl_homes());
    } else if in_wsl() {
        out.extend(windows_homes());
    }
    out
}

/// 是不是另一边系统的路径：Windows 上的 `\\wsl.localhost\…`、WSL 里的 `/mnt/<盘符>/…`。
/// 这两种走 9P / drvfs，文件系统通知不可靠
pub fn is_foreign(p: &Path) -> bool {
    let s = p.to_string_lossy();
    s.starts_with(r"\\") || s.starts_with("/mnt/")
}

/// 正在运行的发行版里的 `/root` 和 `/home/*`。停着的不碰：访问 `\\wsl.localhost\<发行版>` 会把它启动起来
fn wsl_homes() -> Vec<PathBuf> {
    running_distros()
        .into_iter()
        // Docker Desktop 自己的发行版，没人在里面跑 agent
        .filter(|d| !d.starts_with("docker-desktop"))
        .flat_map(|d| {
            let root = PathBuf::from(format!(r"\\wsl.localhost\{d}"));
            let mut homes = vec![root.join("root")];
            homes.extend(subdirs(&root.join("home")));
            homes
        })
        .collect()
}

/// `wsl.exe -l --running -q`；没装 WSL 或调用失败就当没有
fn running_distros() -> Vec<String> {
    let Ok(out) = std::process::Command::new("wsl.exe")
        .args(["-l", "--running", "-q"])
        // 默认输出 UTF-16LE；这个变量让它改用 UTF-8（老版本不认，下面兜底）
        .env("WSL_UTF8", "1")
        .output()
    else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    let text = if out.stdout.len() >= 2 && out.stdout[1] == 0 {
        let units: Vec<u16> = out.stdout.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    text.lines().map(|l| l.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}').to_string()).filter(|l| !l.is_empty()).collect()
}

fn in_wsl() -> bool {
    std::env::var_os("WSL_DISTRO_NAME").is_some() || Path::new("/proc/sys/fs/binfmt_misc/WSLInterop").exists()
}

/// `/mnt/<盘符>/Users/*`
fn windows_homes() -> Vec<PathBuf> {
    let Ok(mnt) = fs::read_dir("/mnt") else { return Vec::new() };
    mnt.filter_map(Result::ok)
        .filter(|e| e.file_name().to_str().is_some_and(|n| n.len() == 1 && n.chars().all(|c| c.is_ascii_alphabetic())))
        .flat_map(|e| subdirs(&e.path().join("Users")))
        .collect()
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = fs::read_dir(dir) else { return Vec::new() };
    rd.filter_map(Result::ok).filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.path()).collect()
}
