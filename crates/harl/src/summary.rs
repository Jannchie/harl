//! `/api/summary`：把所有最新快照的会话统计聚合成仪表盘要的形状。
//! 每个快照的统计按 tree 缓存在 index.db 里，第一次算完以后只有新快照要算。
//!
//! 时间范围过滤在 turn 这一级做：所有聚合（totals、by_model、by_tool、按天、按会话）都只从落在
//! `[from, to]` 里的 turn 推导，这样各个数字彼此一致。不属于任何 turn 的零星事件不计。

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use chrono::{DateTime, Datelike, FixedOffset, NaiveDate, TimeZone, Timelike, Utc};
use serde::Serialize;

use harl_core::event::ToolCategory;
use harl_core::normalize;
use harl_core::stats::{self, ModelAgg, SessionStats, Tokens, ToolAgg};
use harl_core::store::{Record, Store};

#[derive(Serialize, Default)]
pub struct Totals {
    pub sessions: u32,
    pub turns: u32,
    pub steps: u32,
    pub tool_calls: u32,
    pub tool_failures: u32,
    pub tokens: Tokens,
    pub cost_usd: f64,
    pub unpriced_steps: u32,
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
    pub cost_usd: f64,
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
    pub cost_usd: f64,
    pub model_ms: u64,
    pub tool_ms: u64,
    pub wall_ms: u64,
    pub by_model: BTreeMap<String, ModelAgg>,
}

/// 一个会话的紧凑统计（treemap / 排行用）
#[derive(Serialize)]
pub struct SessionRow {
    pub id: String,
    pub provider: String,
    pub session: String,
    pub parent_session: Option<String>,
    pub source: String,
    pub turns: u32,
    pub steps: u32,
    pub tool_calls: u32,
    pub tokens: Tokens,
    pub cost_usd: f64,
    pub model_ms: u64,
    pub tool_ms: u64,
    pub wall_ms: u64,
    pub first_ts: Option<DateTime<Utc>>,
    pub last_ts: Option<DateTime<Utc>>,
}

/// 请求的时间范围：本地日期，两端都含
#[derive(Serialize, Clone, Copy, Default)]
pub struct Range {
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
}

#[derive(Serialize)]
pub struct Summary {
    /// 分桶用的时区偏移（分钟，东为正）
    pub tz_offset_min: i32,
    pub range: Range,
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
    /// [weekday 0=Mon..6][hour 0..23] 的 turn 数
    pub hourly_turns: [[u32; 24]; 7],
    /// 同上，费用
    pub hourly_cost: [[f64; 24]; 7],
    pub sessions: Vec<SessionRow>,
}

/// 取或算一个快照的统计
pub fn stats_for(store: &Store, rec: &Record) -> Result<SessionStats> {
    if let Some(s) = store.cached_stats(&rec.tree)? {
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
    store.put_stats(&rec.tree, &s)?;
    Ok(s)
}

pub fn build(store: &Store, tz_offset_min: i32, range: Range) -> Result<Summary> {
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
    let mut hourly_turns = [[0u32; 24]; 7];
    let mut hourly_cost = [[0f64; 24]; 7];
    let mut sessions = Vec::new();

    for rec in store.latest()?.into_values() {
        let s = stats_for(store, &rec)?;
        available.0 = min_opt(available.0, s.first_ts);
        available.1 = max_opt(available.1, s.last_ts);

        // 这个会话落在范围里的 turn 之和
        let mut row = SessionRow {
            id: rec.tree,
            provider: rec.provider,
            session: String::new(),
            parent_session: None,
            source: rec.source,
            turns: 0,
            steps: 0,
            tool_calls: 0,
            tokens: Tokens::default(),
            cost_usd: 0.0,
            model_ms: 0,
            tool_ms: 0,
            wall_ms: 0,
            first_ts: None,
            last_ts: None,
        };
        let mut unpriced_steps = 0;
        let mut tool_failures = 0;
        let p = by_provider.entry(row.provider.clone()).or_default();

        for t in s.turn_stats.iter().filter(|t| t.ts.is_some_and(in_range)) {
            let ts = t.ts.unwrap();
            row.turns += 1;
            row.steps += t.steps;
            row.tool_calls += t.tool_calls;
            row.tokens.add(&t.tokens);
            row.cost_usd += t.cost_usd;
            row.model_ms += t.model_ms;
            row.tool_ms += t.tool_ms;
            row.wall_ms += t.wall_ms;
            row.first_ts = min_opt(row.first_ts, Some(ts));
            row.last_ts = max_opt(row.last_ts, Some(ts));
            tool_failures += t.tool_failures;

            let local = ts.with_timezone(&tz);
            let d = days.entry(local.format("%Y-%m-%d").to_string()).or_default();
            d.turns += 1;
            d.steps += t.steps;
            d.tool_calls += t.tool_calls;
            d.tokens.add(&t.tokens);
            d.cost_usd += t.cost_usd;
            d.model_ms += t.model_ms;
            d.tool_ms += t.tool_ms;
            d.wall_ms += t.wall_ms;
            let (wd, h) = (local.weekday().num_days_from_monday() as usize, local.hour() as usize);
            hourly_turns[wd][h] += 1;
            hourly_cost[wd][h] += t.cost_usd;

            for (m, a) in &t.by_model {
                by_model.entry(m.clone()).or_default().add(a);
                p.by_model.entry(m.clone()).or_default().add(a);
                d.by_model.entry(m.clone()).or_default().add(a);
                if a.unpriced {
                    unpriced_steps += a.steps;
                }
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
        totals.tool_failures += tool_failures;
        totals.tokens.add(&row.tokens);
        totals.cost_usd += row.cost_usd;
        totals.unpriced_steps += unpriced_steps;
        totals.model_ms += row.model_ms;
        totals.tool_ms += row.tool_ms;
        totals.wall_ms += row.wall_ms;
        totals.first_ts = min_opt(totals.first_ts, row.first_ts);
        totals.last_ts = max_opt(totals.last_ts, row.last_ts);

        p.sessions += 1;
        p.turns += row.turns;
        p.tokens.add(&row.tokens);
        p.cost_usd += row.cost_usd;
        p.model_ms += row.model_ms;
        p.tool_ms += row.tool_ms;
        p.wall_ms += row.wall_ms;

        (row.session, row.parent_session) = match normalize::by_name(&row.provider) {
            Some(pr) => normalize::identity(pr, &row.source),
            None => (normalize::file_stem(&row.source).to_string(), None),
        };
        sessions.push(row);
    }
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
        available,
        totals,
        by_provider,
        by_model,
        by_tool,
        by_category,
        daily,
        hourly_turns,
        hourly_cost,
        sessions,
    })
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
