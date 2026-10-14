//! rust-embed 要求 web/dist 在编译时存在。没构建过前端时放一个占位 index.html，让 cargo build 不依赖 pnpm。

use std::fs;
use std::path::Path;

fn main() {
    // 自更新时按目标三元组挑 Release 里的包
    println!("cargo:rustc-env=HARL_TARGET={}", std::env::var("TARGET").unwrap());
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/dist");
    println!("cargo:rerun-if-changed={}", dist.display());
    // i18n! 在编译时读 locales/，cargo 不知道；改了译文要让 harl 重新编译
    println!("cargo:rerun-if-changed=locales");
    if !dist.join("index.html").exists() {
        fs::create_dir_all(&dist).unwrap();
        fs::write(
            dist.join("index.html"),
            "<!doctype html><meta charset=utf-8><title>harl</title><p>The web UI was not built: run pnpm install &amp;&amp; pnpm build in web/, then rebuild harl.</p>",
        )
        .unwrap();
    }
}
