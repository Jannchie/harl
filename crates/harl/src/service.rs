//! `harl service`：登录后自动在后台跑 `harl serve`（或 `harl agent`），挂了自动拉起。都是用户级的，不需要管理员。
//!
//! - Windows：HKCU 的 Run 注册表项启动 `harlw.exe <参数>`；harlw 无窗口地拉起 harl 并看着它
//! - macOS：~/Library/LaunchAgents 下的 launchd agent（KeepAlive）
//! - Linux：systemd 用户服务（Restart=always）
//!
//! 三种守护都给 harl 设 HARL_SUPERVISED，更新完 harl 只需退出，由守护者用新二进制重启。

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::common;

/// 交给守护者的 harl 参数，比如 `["serve"]`、`["agent", "--data-dir", "D:/harl"]`
pub struct Spec {
    pub args: Vec<String>,
    pub data_dir: PathBuf,
}

/// 命令行里一个参数的引号（Windows 注册表 / systemd 都按空格分参数）
fn quote(s: &str) -> String {
    if s.is_empty() || s.contains([' ', '\t', '"']) { format!("\"{}\"", s.replace('"', "\\\"")) } else { s.to_string() }
}

fn run(cmd: &mut Command) -> Result<String> {
    let out = cmd.output().with_context(|| format!("运行 {cmd:?}"))?;
    if !out.status.success() {
        bail!("{cmd:?} 失败: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn install(spec: &Spec) -> Result<()> {
    imp::install(spec)?;
    println!("已安装：登录后自动运行 harl {}", spec.args.join(" "));
    println!("日志：{}", log_hint(&spec.data_dir));
    Ok(())
}

pub fn uninstall(data_dir: &Path) -> Result<()> {
    imp::uninstall(data_dir)?;
    println!("已卸载开机自启（数据目录没动）");
    Ok(())
}

pub fn start(data_dir: &Path) -> Result<()> {
    imp::start(data_dir)
}

pub fn stop(data_dir: &Path) -> Result<()> {
    imp::stop(data_dir)
}

pub fn status(data_dir: &Path) -> Result<()> {
    match imp::installed(data_dir)? {
        Some(cmd) => println!("已安装：{cmd}"),
        None => println!("未安装（harl service install 安装）"),
    }
    println!("运行中：{}", if imp::running(data_dir)? { "是" } else { "否" });
    println!("日志：{}", log_hint(data_dir));
    Ok(())
}

pub fn is_running(data_dir: &Path) -> bool {
    imp::running(data_dir).unwrap_or(false)
}

fn log_hint(data_dir: &Path) -> String {
    if cfg!(target_os = "linux") {
        "journalctl --user -u harl -f".into()
    } else {
        common::log_path(data_dir).display().to_string()
    }
}

#[cfg(windows)]
mod imp {
    use std::fs::OpenOptions;
    use std::os::windows::process::CommandExt;

    use super::*;

    const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE: &str = "harl";

    pub fn install(spec: &Spec) -> Result<()> {
        let harlw = common::sibling("harlw")?;
        if !harlw.is_file() {
            bail!("找不到 {}（它和 harl.exe 一起发布）", harlw.display());
        }
        let line = std::iter::once(quote(&harlw.to_string_lossy())).chain(spec.args.iter().map(|a| quote(a))).collect::<Vec<_>>().join(" ");
        run(Command::new("reg").args(["add", RUN_KEY, "/v", VALUE, "/t", "REG_SZ", "/d", &line, "/f"]))?;
        // 换了参数要重启才生效
        stop(&spec.data_dir)?;
        spawn(&harlw, &spec.args)
    }

    pub fn uninstall(data_dir: &Path) -> Result<()> {
        stop(data_dir)?;
        if installed(data_dir)?.is_some() {
            run(Command::new("reg").args(["delete", RUN_KEY, "/v", VALUE, "/f"]))?;
        }
        Ok(())
    }

    /// 注册表里记的那条命令
    pub fn installed(_: &Path) -> Result<Option<String>> {
        let Ok(out) = run(Command::new("reg").args(["query", RUN_KEY, "/v", VALUE])) else { return Ok(None) };
        Ok(out.lines().find_map(|l| l.split_once("REG_SZ").map(|(_, v)| v.trim().to_string())))
    }

    pub fn start(data_dir: &Path) -> Result<()> {
        let Some(line) = installed(data_dir)? else { bail!("还没安装，先 harl service install") };
        if running(data_dir)? {
            println!("已经在运行");
            return Ok(());
        }
        // 直接把注册表里那条命令跑一遍
        Command::new("cmd").raw_arg(format!("/c start \"\" {line}")).creation_flags(0x0800_0000).spawn()?;
        println!("已启动");
        Ok(())
    }

    pub fn stop(data_dir: &Path) -> Result<()> {
        if !running(data_dir)? {
            return Ok(());
        }
        let pid = std::fs::read_to_string(common::supervisor_pid_path(data_dir))?;
        // harlw 连同它拉起的 harl 一起结束
        run(Command::new("taskkill").args(["/T", "/F", "/PID", pid.trim()]))?;
        for _ in 0..50 {
            if !running(data_dir)? {
                println!("已停止");
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        bail!("harlw 没有退出")
    }

    /// harlw 运行期间一直锁着 harlw.lock，拿不到锁就是在跑
    pub fn running(data_dir: &Path) -> Result<bool> {
        let Ok(f) = OpenOptions::new().write(true).open(common::supervisor_lock_path(data_dir)) else { return Ok(false) };
        Ok(f.try_lock().is_err())
    }

    fn spawn(harlw: &Path, args: &[String]) -> Result<()> {
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        Command::new(harlw).args(args).creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP).spawn()?;
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use crate::common::SUPERVISED_ENV;

    const LABEL: &str = "com.jannchie.harl";

    fn plist() -> Result<PathBuf> {
        Ok(dirs::home_dir().context("找不到 home")?.join("Library/LaunchAgents").join(format!("{LABEL}.plist")))
    }

    fn domain() -> Result<String> {
        Ok(format!("gui/{}", run(Command::new("id").arg("-u"))?.trim()))
    }

    fn xml(s: &str) -> String {
        s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
    }

    pub fn install(spec: &Spec) -> Result<()> {
        let exe = std::env::current_exe()?;
        let log = common::log_path(&spec.data_dir);
        std::fs::create_dir_all(log.parent().unwrap())?;
        let args: String = std::iter::once(exe.to_string_lossy().into_owned())
            .chain(spec.args.iter().cloned())
            .map(|a| format!("    <string>{}</string>\n", xml(&a)))
            .collect();
        let body = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>
{args}  </array>
  <key>EnvironmentVariables</key><dict><key>{SUPERVISED_ENV}</key><string>1</string></dict>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ThrottleInterval</key><integer>5</integer>
  <key>StandardOutPath</key><string>{log}</string>
  <key>StandardErrorPath</key><string>{log}</string>
</dict>
</plist>
"#,
            log = xml(&log.to_string_lossy())
        );
        let path = plist()?;
        std::fs::create_dir_all(path.parent().unwrap())?;
        let _ = stop(&spec.data_dir);
        std::fs::write(&path, body)?;
        run(Command::new("launchctl").args(["bootstrap", &domain()?, &path.to_string_lossy()]))?;
        Ok(())
    }

    pub fn uninstall(data_dir: &Path) -> Result<()> {
        let _ = stop(data_dir);
        let path = plist()?;
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    pub fn installed(_: &Path) -> Result<Option<String>> {
        let path = plist()?;
        Ok(path.exists().then(|| path.display().to_string()))
    }

    pub fn start(_: &Path) -> Result<()> {
        run(Command::new("launchctl").args(["bootstrap", &domain()?, &plist()?.to_string_lossy()]))?;
        Ok(())
    }

    pub fn stop(_: &Path) -> Result<()> {
        run(Command::new("launchctl").args(["bootout", &format!("{}/{LABEL}", domain()?)]))?;
        Ok(())
    }

    pub fn running(_: &Path) -> Result<bool> {
        Ok(Command::new("launchctl").args(["print", &format!("{}/{LABEL}", domain()?)]).output()?.status.success())
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod imp {
    use super::*;
    use crate::common::SUPERVISED_ENV;

    fn unit() -> Result<PathBuf> {
        Ok(dirs::config_dir().context("找不到 ~/.config")?.join("systemd/user/harl.service"))
    }

    fn systemctl(args: &[&str]) -> Result<String> {
        run(Command::new("systemctl").arg("--user").args(args))
    }

    pub fn install(spec: &Spec) -> Result<()> {
        let exe = std::env::current_exe()?;
        let line = std::iter::once(quote(&exe.to_string_lossy())).chain(spec.args.iter().map(|a| quote(a))).collect::<Vec<_>>().join(" ");
        let body = format!(
            "[Unit]\nDescription=harl: archive coding-agent sessions\nAfter=network-online.target\n\n\
             [Service]\nExecStart={line}\nEnvironment={SUPERVISED_ENV}=1\nRestart=always\nRestartSec=3\n\n\
             [Install]\nWantedBy=default.target\n"
        );
        let path = unit()?;
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(&path, body)?;
        systemctl(&["daemon-reload"])?;
        systemctl(&["enable", "harl"])?;
        systemctl(&["restart", "harl"])?;
        // 没有图形登录的服务器上，用户服务默认只在登录期间跑
        let user = std::env::var("USER").unwrap_or_default();
        let lingering = Command::new("loginctl")
            .args(["show-user", &user, "--property=Linger"])
            .output()
            .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("yes"));
        if !lingering {
            println!("提示：要在没登录时也运行（服务器上），执行一次 sudo loginctl enable-linger {user}");
        }
        Ok(())
    }

    pub fn uninstall(_: &Path) -> Result<()> {
        let path = unit()?;
        if path.exists() {
            let _ = systemctl(&["disable", "--now", "harl"]);
            std::fs::remove_file(path)?;
            systemctl(&["daemon-reload"])?;
        }
        Ok(())
    }

    pub fn installed(_: &Path) -> Result<Option<String>> {
        let path = unit()?;
        Ok(path.exists().then(|| path.display().to_string()))
    }

    pub fn start(_: &Path) -> Result<()> {
        systemctl(&["start", "harl"]).map(drop)
    }

    pub fn stop(_: &Path) -> Result<()> {
        systemctl(&["stop", "harl"]).map(drop)
    }

    pub fn running(_: &Path) -> Result<bool> {
        Ok(Command::new("systemctl").args(["--user", "is-active", "--quiet", "harl"]).status()?.success())
    }
}
