//! rust-embed 要求 web/dist 在编译时存在。没构建过前端时放一个占位 index.html，让 cargo build 不依赖 pnpm。

use std::fs;
use std::path::Path;

fn main() {
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/dist");
    println!("cargo:rerun-if-changed={}", dist.display());
    if !dist.join("index.html").exists() {
        fs::create_dir_all(&dist).unwrap();
        fs::write(
            dist.join("index.html"),
            "<!doctype html><meta charset=utf-8><title>harl</title><p>web 没有构建产物：在 web/ 里 pnpm install && pnpm build，再重新编译 harl。</p>",
        )
        .unwrap();
    }
}
