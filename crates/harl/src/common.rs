//! `harl` 和 `harlw`（Windows 上的无窗口守护）共用的约定。两个二进制各自 `mod` 进这个文件。
#![allow(dead_code)]

use std::path::{Path, PathBuf};

/// harl 以这个退出码退出 = "换好新版本了，请马上把我重新拉起来"
pub const EXIT_RESTART: i32 = 75;

/// 守护者（harlw / launchd / systemd）启动 harl 时设上；有它 harl 更新完只管退出，由守护者重启
pub const SUPERVISED_ENV: &str = "HARL_SUPERVISED";

/// `--data-dir` > `$HARL_DATA_DIR` > `~/.harl`
pub fn data_dir(arg: Option<&Path>) -> Option<PathBuf> {
    arg.map(Path::to_path_buf)
        .or_else(|| std::env::var_os("HARL_DATA_DIR").filter(|v| !v.is_empty()).map(PathBuf::from))
        .or_else(|| dirs::home_dir().map(|h| h.join(".harl")))
}

pub fn log_path(data_dir: &Path) -> PathBuf {
    data_dir.join("logs").join("harl.log")
}

/// harlw 运行期间一直锁着它（单实例；别人拿不到锁 = harlw 在跑）
pub fn supervisor_lock_path(data_dir: &Path) -> PathBuf {
    data_dir.join("harlw.lock")
}

/// harlw 的 pid，`harl service stop` 用
pub fn supervisor_pid_path(data_dir: &Path) -> PathBuf {
    data_dir.join("harlw.pid")
}

/// 同目录下的另一个二进制（harl ↔ harlw）
pub fn sibling(name: &str) -> std::io::Result<PathBuf> {
    Ok(std::env::current_exe()?.with_file_name(format!("{name}{}", std::env::consts::EXE_SUFFIX)))
}
