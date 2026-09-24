//! 会话头：标题、父会话、工作目录。只看快照开头几行，按 tree 缓存在 index.db（快照不可变）。
//! 会话列表用它显示和分组，汇总用 cwd 当项目。

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use harl_core::event::{Kind, Role};
use harl_core::normalize::{self, Provider};
use harl_core::store::{Reader, Record, Store};

/// 开头 TITLE_SCAN 行里第一个 turn_started 的文字（没有就是第一条 user 消息）的第一行，截到 TITLE_LEN 字符；
/// 顺带拿事件上的父会话（dsh 的父会话只写在文件头里，光看路径不知道）和工作目录（各家都在某些行上带 `cwd`）
const TITLE_SCAN: usize = 40;
const TITLE_LEN: usize = 120;
/// head() 的口径版本，变了就 +1，index.db 里的旧缓存自动作废
const HEAD_VERSION: u32 = 2;

#[derive(Default, Serialize, Deserialize)]
pub struct Head {
    pub title: Option<String>,
    /// 路径上看得出的父会话优先，其次是事件里写的（dsh 只写在文件头）
    pub parent: Option<String>,
    pub cwd: Option<String>,
}

fn head(reader: &mut Reader<'_>, provider: &Provider, rec: &Record) -> Result<Head> {
    let lines = reader.lines_prefix(rec, TITLE_SCAN)?;
    let (mut found, mut parent, mut cwd) = (None, None, None);
    normalize::normalize(provider, &rec.source, &lines, |ev| {
        if parent.is_none() {
            parent = ev.parent_session.clone();
        }
        if cwd.is_none() {
            cwd = ev.ext.get("cwd").and_then(Value::as_str).map(normalize::slashed);
        }
        let is_prompt = ev.kind == Kind::TurnStarted || (ev.kind == Kind::Message && ev.role == Some(Role::User));
        if found.is_none() && is_prompt {
            found = ev.text.as_deref().and_then(|t| t.lines().find(|l| !l.trim().is_empty())).map(|l| {
                let l = l.trim();
                match l.char_indices().nth(TITLE_LEN) {
                    Some((i, _)) => format!("{}…", &l[..i]),
                    None => l.to_string(),
                }
            });
        }
        Ok(())
    })?;
    Ok(Head { title: found, parent, cwd })
}

/// 取或算一个快照的 head。缓存按 tree（内容）存，所以只缓存从内容读出来的部分；
/// 从源文件路径推出来的（父会话、pfh 的 workspace）每次现补——同样的内容可能出现在不同路径下
pub fn head_for(store: &Store, reader: &mut Reader<'_>, provider: &Provider, rec: &Record) -> Option<Head> {
    let mut h = match store.cached::<Head>("head", HEAD_VERSION, &rec.tree) {
        Ok(Some(h)) => h,
        _ => {
            let h = head(reader, provider, rec).ok()?;
            let _ = store.put_cached("head", HEAD_VERSION, &rec.tree, &h);
            h
        }
    };
    h.parent = normalize::identity(provider, &rec.source).1.or(h.parent);
    // pfh 开发版的日志就在 workspace 下面
    h.cwd = h.cwd.or_else(|| rec.source.split_once("/.pfh/").map(|(ws, _)| ws.to_string()));
    Some(h)
}
