//! `/api/summary`：把所有最新快照的会话统计聚合成仪表盘要的形状。
//! 每个快照的统计按 tree 缓存在 index.db 里，第一次算完以后只有新快照要算。
//!
//! 时间范围过滤在 turn 这一级做：所有聚合（totals、by_model、by_tool、按天、按会话）都只从落在
//! `[from, to]` 里的 turn 推导，这样各个数字彼此一致。不属于任何 turn 的零星事件不计。
//!
//! provider、项目过滤按会话；模型过滤也在 turn 这一级：只留用过所选模型的 turn，其中 tokens / 步数 / 模型时间
//! 只算所选模型的部分，工具和墙钟时间拆不到模型上，按整个 turn 算。

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result};
use chrono::{DateTime, Datelike, FixedOffset, NaiveDate, TimeZone, Timelike, Utc};
use serde::Serialize;

use harl_core::event::ToolCategory;
use harl_core::normalize;
use harl_core::stats::{self, ModelAgg, SessionStats, Tokens, ToolAgg, TurnStat};
use harl_core::store::{Record, Store};

use crate::head;

#[derive(Serialize, Default)]
pub struct Totals {
    pub sessions: u32,
    pub turns: u32,
    pub steps: u32,
    pub tool_calls: u32,
    pub tool_failures: u32,
    pub tokens: Tokens,
    pub model_ms: u64,
    pub tool_ms: u64,
    pub wall_ms: u64,
    pub first_ts: Option<DateTime<Utc>>,
    pub last_ts: Option<DateTime<Utc>>,
}

#[derive(Serialize, Default)]
pub struct ProviderAgg {
    pub sessions: u32,
    pub turns: u32,
    pub tokens: Tokens,
    pub model_ms: u64,
    pub tool_ms: u64,
    pub wall_ms: u64,
    pub by_model: BTreeMap<String, ModelAgg>,
}

/// 一天（按客户端时区）
#[derive(Serialize, Default)]
pub struct Day {
    pub date: String,
    pub turns: u32,
    pub steps: u32,
    pub tool_calls: u32,
    pub tokens: Tokens,
    pub model_ms: u64,
    pub tool_ms: u64,
    pub wall_ms: u64,
    pub by_model: BTreeMap<String, ModelAgg>,
    /// provider → 模型 → 聚合：web 端按天定价时 provider 的费用也从这里累加，和总费用同一个口径
    pub by_provider: BTreeMap<String, BTreeMap<String, ModelAgg>>,
}

/// 一个会话落在范围里的 turn 之和。web 端拿它做会话排行，按 cwd 归成项目
#[derive(Serialize, Default)]
pub struct SessionRow {
    /// 快照 tree hash，会话页的路由参数
    pub id: String,
    pub provider: String,
    pub title: Option<String>,
    /// 工作目录（`/` 分隔），即项目；拿不到就 None
    pub cwd: Option<String>,
    pub parent_session: Option<String>,
    pub turns: u32,
    pub steps: u32,
    pub tool_calls: u32,
    pub tool_failures: u32,
    pub tokens: Tokens,
    pub model_ms: u64,
    pub tool_ms: u64,
    pub wall_ms: u64,
    pub first_ts: Option<DateTime<Utc>>,
    pub last_ts: Option<DateTime<Utc>>,
    /// web 端按模型定价
    pub by_model: BTreeMap<String, ModelAgg>,
}

/// 一个 (星期, 小时) 格子；只输出有 turn 的格子
#[derive(Serialize, Default)]
pub struct HourCell {
    /// 0 = 周一
    pub weekday: u8,
    pub hour: u8,
    pub turns: u32,
    pub by_model: BTreeMap<String, Tokens>,
}

/// 请求的时间范围：本地日期，两端都含
#[derive(Serialize, Clone, Copy, Default)]
pub struct Range {
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
}

/// 按 provider / 模型 / 项目过滤；空集合表示不限
#[derive(Serialize, Clone, Default)]
pub struct Filter {
    pub providers: BTreeSet<String>,
    pub models: BTreeSet<String>,
    /// 项目键（见 project_key）；拿不到 cwd 的会话记作空串
    pub projects: BTreeSet<String>,
}

/// 能选的 provider、模型和项目：全部数据里出现过的，不受过滤和时间范围影响
#[derive(Serialize, Default)]
pub struct Options {
    pub providers: BTreeSet<String>,
    pub models: BTreeSet<String>,
    pub projects: BTreeSet<String>,
}

#[derive(Serialize)]
pub struct Summary {
    /// 分桶用的时区偏移（分钟，东为正）
    pub tz_offset_min: i32,
    pub range: Range,
    pub filter: Filter,
    pub options: Options,
    /// 全部数据的时间跨度（不受 range 影响），给范围选择器定边界
    pub available: (Option<DateTime<Utc>>, Option<DateTime<Utc>>),
    pub totals: Totals,
    pub by_provider: BTreeMap<String, ProviderAgg>,
    pub by_model: BTreeMap<String, ModelAgg>,
    /// 工具名 → 统计（每项带 category）
    pub by_tool: BTreeMap<String, ToolAgg>,
    /// 跨 harness 的工具类别：shell / read / edit / search / web / agent / user / mcp / meta / other
    pub by_category: BTreeMap<ToolCategory, ToolAgg>,
    pub daily: Vec<Day>,
    /// 本地时区的 星期 × 小时 分桶，稀疏
    pub hourly: Vec<HourCell>,
    /// 范围内有 turn 的会话，按最后活动时间倒序
    pub sessions: Vec<SessionRow>,
}

/// 取或算一个快照的统计
pub fn stats_for(store: &Store, rec: &Record) -> Result<SessionStats> {
    if let Some(s) = store.cached::<SessionStats>("stats", stats::STATS_VERSION, &rec.tree)? {
        return Ok(s);
    }
    let provider =
        normalize::by_name(&rec.provider).with_context(|| format!("没有 {} 的 normalizer", rec.provider))?;
    let lines = store.reader().lines(rec)?;
    let mut folder = stats::Folder::default();
    normalize::normalize(provider, &rec.source, &lines, |ev| {
        folder.push(&ev);
        Ok(())
    })?;
    let s = folder.finish();
    store.put_cached("stats", stats::STATS_VERSION, &rec.tree, &s)?;
    Ok(s)
}

pub fn build(store: &Store, tz_offset_min: i32, range: Range, filter: Filter) -> Result<Summary> {
    let tz = FixedOffset::east_opt(tz_offset_min * 60).unwrap_or(FixedOffset::east_opt(0).unwrap());
    // 本地日期 → UTC 边界；to 是含的那天，所以取次日零点
    let bound = |d: NaiveDate| tz.from_local_datetime(&d.and_hms_opt(0, 0, 0).unwrap()).single().map(|t| t.to_utc());
    let from_utc = range.from.and_then(bound);
    let to_utc = range.to.and_then(|d| bound(d.succ_opt().unwrap_or(d)));
    let in_range = |ts: DateTime<Utc>| from_utc.is_none_or(|f| ts >= f) && to_utc.is_none_or(|t| ts < t);

    let mut totals = Totals::default();
    let mut available = (None, None);
    let mut by_provider: BTreeMap<String, ProviderAgg> = BTreeMap::new();
    let mut by_model: BTreeMap<String, ModelAgg> = BTreeMap::new();
    let mut by_tool: BTreeMap<String, ToolAgg> = BTreeMap::new();
    let mut by_category: BTreeMap<ToolCategory, ToolAgg> = BTreeMap::new();
    let mut days: BTreeMap<String, Day> = BTreeMap::new();
    let mut hourly: BTreeMap<(u8, u8), HourCell> = BTreeMap::new();
    let mut options = Options::default();
    let mut sessions: Vec<SessionRow> = Vec::new();
    let mut reader = store.reader();

    for rec in normalize::live(store.latest()?.into_values()) {
        let s = stats_for(store, &rec)?;
        let head =
            normalize::by_name(&rec.provider).and_then(|p| head::head_for(store, &mut reader, p, &rec)).unwrap_or_default();
        let project = head.cwd.clone().unwrap_or_default();
        options.projects.insert(project.clone());
        available.0 = min_opt(available.0, s.first_ts);
        available.1 = max_opt(available.1, s.last_ts);
        options.providers.insert(rec.provider.clone());
        options.models.extend(s.by_model.keys().cloned());
        if !filter.providers.is_empty() && !filter.providers.contains(&rec.provider) {
            continue;
        }
        if !filter.projects.is_empty() && !filter.projects.contains(&project_key(&project)) {
            continue;
        }

        let mut row = SessionRow {
            id: rec.tree.clone(),
            provider: rec.provider.clone(),
            title: head.title,
            cwd: head.cwd,
            parent_session: head.parent,
            ..Default::default()
        };
        let provider = rec.provider.clone();
        let p = by_provider.entry(rec.provider).or_default();

        for t in s.turn_stats.iter().filter(|t| t.ts.is_some_and(in_range)) {
            let Some(t) = filter.pick(t) else { continue };
            let t = &*t;
            let ts = t.ts.unwrap();
            row.turns += 1;
            row.steps += t.steps;
            row.tool_calls += t.tool_calls;
            row.tokens.add(&t.tokens);
            row.model_ms += t.model_ms;
            row.tool_ms += t.tool_ms;
            row.wall_ms += t.wall_ms;
            row.first_ts = min_opt(row.first_ts, Some(ts));
            row.last_ts = max_opt(row.last_ts, Some(ts));
            row.tool_failures += t.tool_failures;

            let local = ts.with_timezone(&tz);
            let d = days.entry(local.format("%Y-%m-%d").to_string()).or_default();
            d.turns += 1;
            d.steps += t.steps;
            d.tool_calls += t.tool_calls;
            d.tokens.add(&t.tokens);
            d.model_ms += t.model_ms;
            d.tool_ms += t.tool_ms;
            d.wall_ms += t.wall_ms;
            let (wd, h) = (local.weekday().num_days_from_monday() as u8, local.hour() as u8);
            let cell = hourly.entry((wd, h)).or_insert_with(|| HourCell { weekday: wd, hour: h, ..Default::default() });
            cell.turns += 1;

            for (m, a) in &t.by_model {
                row.by_model.entry(m.clone()).or_default().add(a);
                by_model.entry(m.clone()).or_default().add(a);
                p.by_model.entry(m.clone()).or_default().add(a);
                d.by_model.entry(m.clone()).or_default().add(a);
                d.by_provider.entry(provider.clone()).or_default().entry(m.clone()).or_default().add(a);
                cell.by_model.entry(m.clone()).or_default().add(&a.tokens);
            }
            for (name, a) in &t.by_tool {
                by_tool.entry(name.clone()).or_default().add(a);
                by_category.entry(a.category).or_default().add(a);
            }
        }
        if row.turns == 0 {
            continue;
        }

        totals.sessions += 1;
        totals.turns += row.turns;
        totals.steps += row.steps;
        totals.tool_calls += row.tool_calls;
        totals.tool_failures += row.tool_failures;
        totals.tokens.add(&row.tokens);
        totals.model_ms += row.model_ms;
        totals.tool_ms += row.tool_ms;
        totals.wall_ms += row.wall_ms;
        totals.first_ts = min_opt(totals.first_ts, row.first_ts);
        totals.last_ts = max_opt(totals.last_ts, row.last_ts);

        p.sessions += 1;
        p.turns += row.turns;
        p.tokens.add(&row.tokens);
        p.model_ms += row.model_ms;
        p.tool_ms += row.tool_ms;
        p.wall_ms += row.wall_ms;
        sessions.push(row);
    }
    sessions.sort_by(|a, b| b.last_ts.cmp(&a.last_ts));
    by_provider.retain(|_, p| p.sessions > 0);

    let daily = days
        .into_iter()
        .map(|(date, mut d)| {
            d.date = date;
            d
        })
        .collect();
    Ok(Summary {
        tz_offset_min,
        range,
        filter,
        options,
        available,
        totals,
        by_provider,
        by_model,
        by_tool,
        by_category,
        daily,
        hourly: hourly.into_values().collect(),
        sessions,
    })
}

impl Filter {
    /// 按模型过滤一个 turn：没选模型原样返回；没用到所选模型的 turn 丢掉；
    /// 否则只留所选模型的 by_model，并据此重算 steps / tokens / model_ms
    fn pick<'a>(&self, t: &'a TurnStat) -> Option<Cow<'a, TurnStat>> {
        if self.models.is_empty() {
            return Some(Cow::Borrowed(t));
        }
        let by_model: BTreeMap<String, ModelAgg> =
            t.by_model.iter().filter(|(m, _)| self.models.contains(*m)).map(|(m, a)| (m.clone(), a.clone())).collect();
        if by_model.is_empty() {
            return None;
        }
        let mut sum = ModelAgg::default();
        for a in by_model.values() {
            sum.add(a);
        }
        Some(Cow::Owned(TurnStat {
            steps: sum.steps,
            tokens: sum.tokens,
            model_ms: sum.model_ms,
            by_model,
            by_tool: t.by_tool.clone(),
            ..*t
        }))
    }
}

/// 项目键：和 web 侧栏的分组一致（lib/project.ts）——小写、`/` 分隔、去掉结尾斜杠，
/// 所以 Windows 上大小写不同的同一个目录算一个项目
fn project_key(cwd: &str) -> String {
    normalize::slashed(cwd).trim_end_matches('/').to_lowercase()
}

fn min_opt(a: Option<DateTime<Utc>>, b: Option<DateTime<Utc>>) -> Option<DateTime<Utc>> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

fn max_opt(a: Option<DateTime<Utc>>, b: Option<DateTime<Utc>>) -> Option<DateTime<Utc>> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agg(steps: u32, input: u64, ms: u64) -> ModelAgg {
        ModelAgg { steps, tokens: Tokens { input, ..Default::default() }, model_ms: ms }
    }

    fn turn() -> TurnStat {
        TurnStat {
            ts: Some(Utc::now()),
            steps: 3,
            tool_calls: 2,
            tokens: Tokens { input: 30, ..Default::default() },
            model_ms: 300,
            wall_ms: 1000,
            by_model: BTreeMap::from([("a".into(), agg(2, 20, 200)), ("b".into(), agg(1, 10, 100))]),
            ..Default::default()
        }
    }

    fn only(models: &[&str]) -> Filter {
        Filter { models: models.iter().map(|m| m.to_string()).collect(), ..Default::default() }
    }

    // 模型过滤：只留所选模型的份额，工具和墙钟按整个 turn；没用到所选模型的 turn 丢掉
    #[test]
    fn model_filter_splits_turn() {
        let t = turn();
        let picked = only(&["b"]).pick(&t).expect("用到了 b");
        assert_eq!((picked.steps, picked.tokens.input, picked.model_ms), (1, 10, 100));
        assert_eq!((picked.tool_calls, picked.wall_ms), (2, 1000));
        assert_eq!(picked.by_model.keys().collect::<Vec<_>>(), ["b"]);
        assert!(only(&["c"]).pick(&t).is_none());
        assert!(matches!(Filter::default().pick(&t), Some(Cow::Borrowed(_))));
    }

    // 项目键和 web 侧栏同一口径：大小写、分隔符、结尾斜杠不同的同一目录是一个项目
    #[test]
    fn project_key_normalizes() {
        assert_eq!(project_key(r"E:\code\Harl\"), "e:/code/harl");
        assert_eq!(project_key("e:/code/harl"), "e:/code/harl");
        assert_eq!(project_key(""), "");
    }
}
