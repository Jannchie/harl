//! harlw：Windows 上开机自启用的守护。本身是 GUI 子系统，登录时不会弹出黑窗口；
//! 用无窗口方式拉起同目录的 `harl <参数>`，输出写进 `<data_dir>/logs/harl.log`，挂了就重启，
//! harl 以 EXIT_RESTART 退出（更新完）时立即重启。macOS / Linux 用 launchd / systemd，不需要它。
#![cfg_attr(windows, windows_subsystem = "windows")]

#[path = "common.rs"]
mod common;

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use common::{EXIT_RESTART, SUPERVISED_ENV};

/// 日志超过这么大就挪成 harl.log.1
const LOG_LIMIT: u64 = 10 << 20;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let data_dir_arg = args
        .iter()
        .position(|a| a == "--data-dir")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from);
    let Some(data_dir) = common::data_dir(data_dir_arg.as_deref()) else {
        return;
    };
    let _ = fs::create_dir_all(&data_dir);
    // 单实例：运行期间一直锁着；已经有一个在跑就直接退出
    let Ok(lock) = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(common::supervisor_lock_path(&data_dir))
    else {
        return;
    };
    if lock.try_lock().is_err() {
        return;
    }
    let _ = fs::write(
        common::supervisor_pid_path(&data_dir),
        std::process::id().to_string(),
    );

    let Ok(harl) = common::sibling("harl") else {
        return;
    };
    let log = common::log_path(&data_dir);
    let mut backoff = Duration::from_secs(1);
    loop {
        let started = Instant::now();
        let code = run_once(&harl, &args, &log);
        match code {
            Some(0) => break,
            Some(EXIT_RESTART) => {
                note(&log, "harlw: restarting (update)");
                backoff = Duration::from_secs(1);
                continue;
            }
            _ => {}
        }
        // 跑了一阵才挂的，从头退避；一启动就挂（端口被占、配置错）的，逐步拉长到一分钟
        if started.elapsed() > Duration::from_secs(60) {
            backoff = Duration::from_secs(1);
        }
        note(
            &log,
            &format!(
                "harlw: harl exited ({code:?}), restarting in {}s",
                backoff.as_secs()
            ),
        );
        std::thread::sleep(backoff);
        backoff = (backoff * 2).min(Duration::from_secs(60));
    }
}

fn run_once(harl: &Path, args: &[String], log: &Path) -> Option<i32> {
    rotate(log);
    let out = open_log(log)?;
    let err = out.try_clone().ok()?;
    let mut cmd = Command::new(harl);
    cmd.args(args)
        .env(SUPERVISED_ENV, "1")
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    match cmd.spawn() {
        Ok(mut child) => child.wait().ok().and_then(|s| s.code()),
        Err(e) => {
            note(
                log,
                &format!("harlw: failed to start {}: {e}", harl.display()),
            );
            None
        }
    }
}

fn open_log(log: &Path) -> Option<File> {
    fs::create_dir_all(log.parent()?).ok()?;
    OpenOptions::new().create(true).append(true).open(log).ok()
}

fn rotate(log: &Path) {
    if fs::metadata(log).is_ok_and(|m| m.len() > LOG_LIMIT) {
        let _ = fs::rename(log, log.with_extension("log.1"));
    }
}

fn note(log: &Path, msg: &str) {
    if let Some(mut f) = open_log(log) {
        let _ = writeln!(
            f,
            "{} {msg}",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
        );
    }
}
