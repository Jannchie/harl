//! raw 行 → canonical Event。每个 harness 一个模块，这里放注册表和公用工具。
//!
//! 一个 provider 要回答三件事：哪些文件是我的（`owns`）、默认去哪找（`default_roots`）、
//! 怎么逐行归一化（`new`）。加一个 harness 只需在 `PROVIDERS` 里加一项。

pub mod claude;
pub mod pfh;

use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::event::{Event, Prov, Usage};
use crate::hash::{Hash, hex};

#[derive(Clone, Copy)]
pub struct Provider {
    pub name: &'static str,
    /// 这个路径（已换成 `/` 分隔）是不是本 provider 的会话文件
    owns: fn(&str) -> bool,
    /// 默认扫描根目录（只返回存在的）
    default_roots: fn(&Path) -> Vec<PathBuf>,
    new: fn(&str) -> Box<dyn LineNormalizer>,
}

pub static PROVIDERS: &[Provider] = &[pfh::PROVIDER, claude::PROVIDER];

/// `source` 已是 `/` 分隔的绝对路径（见 store::source_key）
pub fn provider_of(source: &str) -> Option<&'static Provider> {
    PROVIDERS.iter().find(|pr| (pr.owns)(source))
}

pub fn by_name(name: &str) -> Option<&'static Provider> {
    PROVIDERS.iter().find(|pr| pr.name == name)
}

pub fn default_roots(home: &Path) -> Vec<PathBuf> {
    PROVIDERS.iter().flat_map(|pr| (pr.default_roots)(home)).filter(|r| r.is_dir()).collect()
}

/// 逐行、带状态（Claude Code 要记住当前 turn/step）
pub trait LineNormalizer {
    /// 这个文件的会话 id 和父会话（子 agent 时有）
    fn identity(&self) -> (String, Option<String>);
    fn line(&mut self, seq: u32, obj: Map<String, Value>, prov: Prov) -> Event;
}

/// 不归一化内容，只问这个文件是哪个会话的
pub fn identity(provider: &Provider, path: &str) -> (String, Option<String>) {
    (provider.new)(path).identity()
}

/// 把一个快照的每一行变成事件，交给 sink。不丢行：空行和解析不了的半行也各是一个 Meta 事件。
pub fn normalize(
    provider: &Provider,
    path: &str,
    lines: &[(Hash, Vec<u8>)],
    mut sink: impl FnMut(Event) -> Result<()>,
) -> Result<()> {
    let mut ctx = (provider.new)(path);
    for (i, (h, line)) in lines.iter().enumerate() {
        let prov = Prov { chunk: hex(h) };
        let body = trim_line(line);
        let obj = match serde_json::from_slice::<Value>(body) {
            Ok(Value::Object(m)) => m,
            _ => {
                let mut m = Map::new();
                m.insert("_unparsed".into(), Value::String(String::from_utf8_lossy(body).into_owned()));
                m
            }
        };
        sink(ctx.line(i as u32, obj, prov))?;
    }
    Ok(())
}

fn trim_line(line: &[u8]) -> &[u8] {
    let mut end = line.len();
    while end > 0 && matches!(line[end - 1], b'\n' | b'\r') {
        end -= 1;
    }
    &line[..end]
}

// ---- 路径 ----

/// 统一成 `/` 分隔。不用 std::path：Windows 上记下的路径要能在 Linux 上解析。
pub fn slashed(path: &str) -> String {
    path.replace('\\', "/")
}

/// 文件名去掉 .jsonl
pub fn file_stem(path: &str) -> &str {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    name.strip_suffix(".jsonl").unwrap_or(name)
}

// ---- JSON 对象工具：映射过的字段从对象里拿走，剩下的留给 ext ----

/// 能解析成 T 才拿走；形状不对（比如 `input_tokens: "10"`）就原样留在对象里，不丢
pub fn take<T: DeserializeOwned>(obj: &mut Map<String, Value>, key: &str) -> Option<T> {
    let v = T::deserialize(obj.get(key)?).ok()?;
    obj.remove(key);
    Some(v)
}

pub fn take_str(obj: &mut Map<String, Value>, key: &str) -> Option<String> {
    take(obj, key)
}

pub fn take_u64(obj: &mut Map<String, Value>, key: &str) -> Option<u64> {
    take(obj, key)
}

/// RFC3339，转成 UTC
pub fn take_ts(obj: &mut Map<String, Value>, key: &str) -> Option<DateTime<Utc>> {
    take(obj, key)
}

pub fn take_usage(obj: &mut Map<String, Value>, key: &str) -> Option<Usage> {
    take(obj, key)
}

/// 一个文本类 content part：`key` 字段移进 out（不复制），`type` 丢掉，还有剩的字段留进 rest
pub fn take_text_part(mut p: Value, key: &str, out: &mut Vec<String>, rest: &mut Vec<Value>) {
    if let Some(m) = p.as_object_mut() {
        out.push(take_str(m, key).unwrap_or_default());
        m.remove("type");
        if !m.is_empty() {
            rest.push(p);
        }
    }
}

/// 把 part 里映射掉的键拿走；还有剩就留进 rest
pub fn keep_rest(mut p: Value, taken: &[&str], rest: &mut Vec<Value>) {
    if let Some(m) = p.as_object_mut() {
        for k in taken {
            m.remove(*k);
        }
        if !m.is_empty() {
            rest.push(p);
        }
    }
}

/// 多段文本拼成正文；没有就是 None
pub fn join_texts(mut v: Vec<String>) -> Option<String> {
    match v.len() {
        0 => None,
        1 => v.pop(),
        _ => Some(v.join("
")),
    }
}

/// 测试用：把内联的行喂给 `normalize`，收成事件列表
#[cfg(test)]
pub fn run(provider: &Provider, path: &str, lines: &[&str]) -> Vec<Event> {
    let lines: Vec<(Hash, Vec<u8>)> = lines
        .iter()
        .map(|l| {
            let b = format!("{l}\n").into_bytes();
            (crate::hash::hash(&b), b)
        })
        .collect();
    let mut out = Vec::new();
    normalize(provider, path, &lines, |ev| {
        out.push(ev);
        Ok(())
    })
    .unwrap();
    out
}
