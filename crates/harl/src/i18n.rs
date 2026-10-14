//! 界面语言。译文在 `locales/*.toml`（rust-i18n 的 `_version = 2` 格式：每个 key 下并排写 en / zh-CN / ja），编译时嵌进二进制。
//!
//! 交互运行时语言按这个顺序定：`HARL_LANG` → `LC_ALL` / `LC_MESSAGES` / `LANG`（`C`、`POSIX` 不算）→ 系统界面语言 → en。
//! 只认 en、zh-CN、ja 三种，别的语言都回退英文。

/// 支持的语言；第一个是回退语言
pub const LOCALES: [&str; 3] = ["en", "zh-CN", "ja"];

/// 把 `zh_CN.UTF-8`、`ja-JP`、`en_US` 这类写法归到支持的语言；不认识的返回 None
fn normalize(tag: &str) -> Option<&'static str> {
    let lower = tag.trim().to_ascii_lowercase();
    let lang = lower.split(['_', '-', '.', '@']).next()?;
    match lang {
        "zh" => Some("zh-CN"),
        "ja" => Some("ja"),
        "en" => Some("en"),
        _ => None,
    }
}

fn detect() -> &'static str {
    for var in ["HARL_LANG", "LC_ALL", "LC_MESSAGES", "LANG"] {
        let Ok(v) = std::env::var(var) else { continue };
        if v.is_empty() || v == "C" || v == "POSIX" || v.starts_with("C.") {
            continue;
        }
        // 显式设了但不认识（比如 fr_FR），就用英文，不再往下看系统语言
        return normalize(&v).unwrap_or(LOCALES[0]);
    }
    sys_locale::get_locale()
        .as_deref()
        .and_then(normalize)
        .unwrap_or(LOCALES[0])
}

/// 进程启动时调用一次。被守护运行（harlw / launchd / systemd 拉起、输出写进日志）时固定英文：
/// 日志里不混语言，harlw 自己写的也是英文，贴到 issue 里别人也看得懂
pub fn init() {
    let supervised = std::env::var_os(crate::common::SUPERVISED_ENV).is_some();
    rust_i18n::set_locale(if supervised { LOCALES[0] } else { detect() });
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;

    #[test]
    fn normalizes_tags() {
        assert_eq!(normalize("zh_CN.UTF-8"), Some("zh-CN"));
        assert_eq!(normalize("zh-TW"), Some("zh-CN"));
        assert_eq!(normalize("ja-JP"), Some("ja"));
        assert_eq!(normalize("en_US"), Some("en"));
        assert_eq!(normalize("fr_FR"), None);
    }

    fn placeholders(s: &str) -> BTreeSet<String> {
        s.match_indices("%{")
            .filter_map(|(i, _)| s[i + 2..].split_once('}').map(|(name, _)| name.to_string()))
            .collect()
    }

    /// 每个 key 三种语言都要有，而且各语言用到的 %{占位符} 一致
    #[test]
    fn locales_complete() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("locales");
        let mut seen = BTreeMap::new();
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }
            let file = path.file_name().unwrap().to_string_lossy().into_owned();
            let table: toml::Table =
                toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            assert_eq!(
                table.get("_version").and_then(|v| v.as_integer()),
                Some(2),
                "{file}: 缺 _version = 2"
            );
            for (key, value) in table.iter().filter(|(k, _)| *k != "_version") {
                if let Some(prev) = seen.insert(key.clone(), file.clone()) {
                    panic!("{key} 在 {prev} 和 {file} 里重复定义");
                }
                let tr = value
                    .as_table()
                    .unwrap_or_else(|| panic!("{file}: {key} 不是表"));
                let texts: Vec<&str> = LOCALES
                    .iter()
                    .map(|l| {
                        tr.get(*l)
                            .and_then(|v| v.as_str())
                            .unwrap_or_else(|| panic!("{file}: {key} 缺 {l}"))
                    })
                    .collect();
                assert_eq!(tr.len(), LOCALES.len(), "{file}: {key} 有多余的语言");
                let want = placeholders(texts[0]);
                for (l, text) in LOCALES.iter().zip(&texts).skip(1) {
                    assert_eq!(
                        placeholders(text),
                        want,
                        "{file}: {key} 的 {l} 占位符和 en 不一致"
                    );
                }
            }
        }

        // 代码里 t! 宏用到的 key 都得有译文（clap 帮助的 key 是拼出来的，由 main.rs 的测试管）
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        for entry in std::fs::read_dir(&src).unwrap() {
            let path = entry.unwrap().path();
            let code = std::fs::read_to_string(&path).unwrap();
            for (i, _) in code.match_indices("t!(\"") {
                // format!( / print!( 之类的结尾也是 t!(
                if code[..i]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_')
                {
                    continue;
                }
                let key = code[i + 4..].split('"').next().unwrap();
                assert!(
                    seen.contains_key(key),
                    "{}: t!(\"{key}\") 没有译文",
                    path.display()
                );
            }
        }
    }
}
