//! `<data_dir>/config.toml`：远程同步等需要持久下来的设置。文件不存在就全用默认值。
//!
//! ```toml
//! [remote]
//! url = "s3://my-bucket/harl"          # 或本地 / 网盘目录：url = "D:/nas/harl"
//! pull = true                          # 把别的机器的数据拉下来；只采集的机器设 false
//! interval = 60                        # 秒；有新快照会提前推
//!
//! [remote.options]                     # 原样交给 object_store，见其 AmazonS3ConfigKey
//! aws_endpoint = "https://<account>.r2.cloudflarestorage.com"
//! aws_region = "auto"
//! aws_access_key_id = "…"
//! aws_secret_access_key = "…"
//! ```
//!
//! options 里没写的 `aws_*` 从同名的大写环境变量补（AWS_ACCESS_KEY_ID 等），密钥可以不落盘。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rust_i18n::t;
use serde::Deserialize;

#[derive(Deserialize, Default, Debug)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub remote: Option<RemoteConfig>,
    #[serde(default)]
    pub update: UpdateConfig,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct RemoteConfig {
    pub url: String,
    #[serde(default = "yes")]
    pub pull: bool,
    #[serde(default = "default_interval")]
    pub interval: u64,
    #[serde(default)]
    pub options: BTreeMap<String, String>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct UpdateConfig {
    /// 定期查 GitHub Releases 有没有新版本
    pub check: bool,
    /// serve / agent 在后台自动安装新版本并重启；关掉就只在页面上提示
    pub auto: bool,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self { check: true, auto: true }
    }
}

fn yes() -> bool {
    true
}

fn default_interval() -> u64 {
    60
}

pub fn path(data_dir: &Path) -> PathBuf {
    data_dir.join("config.toml")
}

pub fn load(data_dir: &Path) -> Result<Config> {
    let p = path(data_dir);
    match std::fs::read_to_string(&p) {
        Ok(s) => toml::from_str(&s).with_context(|| t!("main.config_parse", path = p.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(e).with_context(|| t!("main.config_read", path = p.display())),
    }
}
