//! 一个会话的统计：事件流 → tokens / 费用 / 时间 / 工具，纯函数。
//!
//! 时间的定义（都是从事件时间戳推的，harness 没有显式记录）：
//! - step 的模型时间 = 这一步最后一条 assistant 事件的 ts − 这一步开始前最后一个事件的 ts（tool_result / 用户输入）。
//!   含思考和流式输出的全过程。
//! - 工具时间 = tool_result.ts − tool_call.ts，按 call_id 配对。
//! - turn 的墙钟时间 = turn_completed 里显式给的时长（Claude Code 的 `durationMs`），没有就 turn 内首尾事件之差。
//! - usage 每步取最后一份（Claude Code 同一 requestId 的每行都带一份快照，pfh 在 step_completed 上）。

use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::event::{Event, Kind, Role, ToolCategory, Usage};
use crate::pricing;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

impl Tokens {
    fn add_usage(&mut self, u: &Usage) {
        self.input += u.input.unwrap_or(0);
        self.output += u.output.unwrap_or(0);
        self.cache_read += u.cache_read.unwrap_or(0);
        self.cache_write += u.cache_write.unwrap_or(0);
    }

    pub fn add(&mut self, o: &Tokens) {
        self.input += o.input;
        self.output += o.output;
        self.cache_read += o.cache_read;
        self.cache_write += o.cache_write;
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ModelAgg {
    pub steps: u32,
    pub tokens: Tokens,
    pub cost_usd: f64,
    /// 定价表里没有这个模型：cost_usd 是 0，不代表免费
    pub unpriced: bool,
    pub model_ms: u64,
}

impl ModelAgg {
    pub fn add(&mut self, o: &ModelAgg) {
        self.steps += o.steps;
        self.tokens.add(&o.tokens);
        self.cost_usd += o.cost_usd;
        self.unpriced |= o.unpriced;
        self.model_ms += o.model_ms;
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ToolAgg {
    pub category: ToolCategory,
    pub calls: u32,
    pub failures: u32,
    pub ms: u64,
}

impl ToolAgg {
    pub fn add(&mut self, o: &ToolAgg) {
        self.category = o.category;
        self.calls += o.calls;
        self.failures += o.failures;
        self.ms += o.ms;
    }
}

/// 一个 turn：仪表盘按天 / 按小时分桶的最小单位
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct TurnStat {
    pub ts: Option<DateTime<Utc>>,
    pub steps: u32,
    pub tool_calls: u32,
    pub tool_failures: u32,
    pub tokens: Tokens,
    pub cost_usd: f64,
    pub model_ms: u64,
    pub tool_ms: u64,
    pub wall_ms: u64,
    pub by_model: BTreeMap<String, ModelAgg>,
    pub by_tool: BTreeMap<String, ToolAgg>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct SessionStats {
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
    pub by_model: BTreeMap<String, ModelAgg>,
    pub by_tool: BTreeMap<String, ToolAgg>,
    pub by_category: BTreeMap<ToolCategory, ToolAgg>,
    pub turn_stats: Vec<TurnStat>,
}

/// 正在累计的一步
struct StepAcc {
    id: String,
    model: Option<String>,
    usage: Option<Usage>,
    /// 这一步开始前最后一个事件的 ts
    prev_ts: Option<DateTime<Utc>>,
    last_ts: Option<DateTime<Utc>>,
}

struct TurnAcc {
    id: String,
    stat: TurnStat,
    first_ts: Option<DateTime<Utc>>,
    last_ts: Option<DateTime<Utc>>,
    /// turn_completed 显式给的时长
    explicit_ms: Option<u64>,
    step: Option<StepAcc>,
}

fn ms_between(a: Option<DateTime<Utc>>, b: Option<DateTime<Utc>>) -> u64 {
    match (a, b) {
        (Some(a), Some(b)) => (b - a).num_milliseconds().max(0) as u64,
        _ => 0,
    }
}

#[derive(Default)]
pub struct Folder {
    out: SessionStats,
    turn: Option<TurnAcc>,
    /// 任何事件的最后一个 ts，给下一步当 prev_ts
    last_ts: Option<DateTime<Utc>>,
    /// call_id → (ts, 工具名, 类别)
    pending: HashMap<String, (Option<DateTime<Utc>>, String, ToolCategory)>,
}

impl Folder {
    pub fn push(&mut self, ev: &Event) {
        if let Some(ts) = ev.ts {
            self.out.first_ts.get_or_insert(ts);
            self.out.last_ts = Some(ts);
        }
        // turn 边界
        match (&ev.turn, &mut self.turn) {
            (Some(t), Some(cur)) if cur.id != t.id => {
                self.finish_turn();
                self.turn = Some(TurnAcc::new(t.id.clone()));
            }
            (Some(t), None) => self.turn = Some(TurnAcc::new(t.id.clone())),
            _ => {}
        }
        if let Some(cur) = &mut self.turn
            && ev.turn.is_some()
        {
            if let Some(ts) = ev.ts {
                cur.first_ts.get_or_insert(ts);
                cur.last_ts = Some(ts);
            }
            if ev.kind == Kind::TurnCompleted {
                cur.explicit_ms = ev.ext.get("durationMs").and_then(Value::as_u64);
            }
        }

        // step 边界与模型调用
        let is_model_event = ev.role == Some(Role::Assistant) || ev.kind == Kind::StepCompleted;
        if let (Some(cur), Some(step)) = (&mut self.turn, &ev.step)
            && is_model_event
        {
            if cur.step.as_ref().is_none_or(|s| s.id != step.id) {
                if let Some(done) = cur.step.take() {
                    finish_step(&mut cur.stat, &mut self.out, done);
                }
                cur.step =
                    Some(StepAcc { id: step.id.clone(), model: None, usage: None, prev_ts: self.last_ts, last_ts: None });
            }
            let s = cur.step.as_mut().unwrap();
            if s.model.is_none() {
                s.model = ev.model.clone();
            }
            if let Some(u) = &ev.usage {
                s.usage = Some(u.clone());
            }
            if ev.ts.is_some() {
                s.last_ts = ev.ts;
            }
        }

        // 工具配对
        if let Some(tool) = &ev.tool {
            match ev.kind {
                Kind::ToolCall => {
                    let cat = tool.category.unwrap_or_default();
                    self.pending.insert(tool.call_id.clone(), (ev.ts, tool.name.clone(), cat));
                    self.out.tool_calls += 1;
                    if let Some(cur) = &mut self.turn {
                        cur.stat.tool_calls += 1;
                    }
                }
                Kind::ToolResult => {
                    let (start, name, cat) = self
                        .pending
                        .remove(&tool.call_id)
                        .unwrap_or((None, tool.name.clone(), tool.category.unwrap_or_default()));
                    let name = if name.is_empty() { tool.name.clone() } else { name };
                    let failed = matches!(tool.status.as_deref(), Some("failed" | "error"));
                    let one = ToolAgg { category: cat, calls: 1, failures: failed as u32, ms: ms_between(start, ev.ts) };
                    self.out.by_tool.entry(name.clone()).or_default().add(&one);
                    self.out.by_category.entry(cat).or_default().add(&one);
                    self.out.tool_ms += one.ms;
                    self.out.tool_failures += one.failures;
                    if let Some(cur) = &mut self.turn {
                        cur.stat.by_tool.entry(name).or_default().add(&one);
                        cur.stat.tool_ms += one.ms;
                        cur.stat.tool_failures += one.failures;
                    }
                }
                _ => {}
            }
        }

        if ev.ts.is_some() {
            self.last_ts = ev.ts;
        }
    }

    fn finish_turn(&mut self) {
        let Some(mut cur) = self.turn.take() else { return };
        if let Some(done) = cur.step.take() {
            finish_step(&mut cur.stat, &mut self.out, done);
        }
        cur.stat.ts = cur.first_ts;
        cur.stat.wall_ms = cur.explicit_ms.unwrap_or_else(|| ms_between(cur.first_ts, cur.last_ts));
        self.out.wall_ms += cur.stat.wall_ms;
        self.out.turns += 1;
        self.out.turn_stats.push(cur.stat);
    }

    pub fn finish(mut self) -> SessionStats {
        self.finish_turn();
        self.out
    }
}

impl TurnAcc {
    fn new(id: String) -> Self {
        Self { id, stat: TurnStat::default(), first_ts: None, last_ts: None, explicit_ms: None, step: None }
    }
}

/// 一步结束：tokens、费用、模型时间记到 turn 和会话两级
fn finish_step(turn: &mut TurnStat, out: &mut SessionStats, s: StepAcc) {
    let model = s.model.unwrap_or_else(|| "unknown".into());
    let mut agg = ModelAgg { steps: 1, model_ms: ms_between(s.prev_ts, s.last_ts), ..Default::default() };
    if let Some(u) = &s.usage {
        agg.tokens.add_usage(u);
        match pricing::cost_usd(&model, u) {
            Some(c) => agg.cost_usd = c,
            None => agg.unpriced = true,
        }
    }
    for (stat_steps, stat_tokens, stat_cost, stat_ms, by_model) in [
        (&mut turn.steps, &mut turn.tokens, &mut turn.cost_usd, &mut turn.model_ms, &mut turn.by_model),
        (&mut out.steps, &mut out.tokens, &mut out.cost_usd, &mut out.model_ms, &mut out.by_model),
    ] {
        *stat_steps += 1;
        stat_tokens.add(&agg.tokens);
        *stat_cost += agg.cost_usd;
        *stat_ms += agg.model_ms;
        by_model.entry(model.clone()).or_default().add(&agg);
    }
    if agg.unpriced {
        out.unpriced_steps += 1;
    }
}

/// 一次算完
pub fn session_stats<'a>(events: impl IntoIterator<Item = &'a Event>) -> SessionStats {
    let mut f = Folder::default();
    for ev in events {
        f.push(ev);
    }
    f.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::normalize::{self, claude};

    fn stats_of(lines: &[&str]) -> SessionStats {
        let evs = normalize::run(&claude::PROVIDER, "/h/.claude/projects/p/s.jsonl", lines);
        session_stats(&evs)
    }

    #[test]
    fn claude_code_turn_with_tool() {
        let s = stats_of(&[
            r#"{"type":"user","uuid":"u1","promptId":"p1","timestamp":"2026-09-19T00:00:00Z","message":{"role":"user","content":"hi"}}"#,
            r#"{"type":"assistant","requestId":"r1","timestamp":"2026-09-19T00:00:03Z","message":{"role":"assistant","model":"claude-opus-5","usage":{"input_tokens":100,"output_tokens":10,"cache_read_input_tokens":1000,"cache_creation_input_tokens":0},"content":[{"type":"text","text":"ok"}]}}"#,
            r#"{"type":"assistant","requestId":"r1","timestamp":"2026-09-19T00:00:04Z","message":{"role":"assistant","model":"claude-opus-5","usage":{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":1000,"cache_creation_input_tokens":0},"content":[{"type":"tool_use","id":"t1","name":"Bash","input":{}}]}}"#,
            r#"{"type":"user","uuid":"u2","promptId":"p1","timestamp":"2026-09-19T00:00:06Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"x","is_error":true}]}}"#,
            r#"{"type":"assistant","requestId":"r2","timestamp":"2026-09-19T00:00:08Z","message":{"role":"assistant","model":"claude-opus-5","usage":{"input_tokens":20,"output_tokens":5},"content":[{"type":"text","text":"done"}]}}"#,
            r#"{"type":"system","subtype":"turn_duration","durationMs":9000,"timestamp":"2026-09-19T00:00:09Z"}"#,
        ]);
        assert_eq!((s.turns, s.steps, s.tool_calls, s.tool_failures), (1, 2, 1, 1));
        // 每步取最后一份 usage：r1 是 (100, 50, 1000)，r2 是 (20, 5)
        assert_eq!(s.tokens, Tokens { input: 120, output: 55, cache_read: 1000, cache_write: 0 });
        // r1：00:00 → 00:04 = 4 s；r2：00:06 → 00:08 = 2 s
        assert_eq!(s.model_ms, 6000);
        assert_eq!(s.tool_ms, 2000);
        assert_eq!(s.wall_ms, 9000);
        assert_eq!(s.by_tool["Bash"].failures, 1);
        assert_eq!(s.by_tool["Bash"].category, ToolCategory::Shell);
        assert_eq!(s.by_category[&ToolCategory::Shell].calls, 1);
        assert_eq!(s.by_model["claude-opus-5"].steps, 2);
        assert_eq!(s.unpriced_steps, 0);
        assert!(s.cost_usd > 0.0);
        assert_eq!(s.turn_stats.len(), 1);
        assert_eq!(s.turn_stats[0].steps, 2);
        assert_eq!(s.turn_stats[0].by_tool["Bash"].failures, 1);
    }

    #[test]
    fn unknown_model_is_unpriced_and_turns_split() {
        let s = stats_of(&[
            r#"{"type":"user","uuid":"u1","promptId":"p1","timestamp":"2026-09-19T00:00:00Z","message":{"role":"user","content":"a"}}"#,
            r#"{"type":"assistant","requestId":"r1","timestamp":"2026-09-19T00:00:01Z","message":{"role":"assistant","model":"gpt-x","usage":{"input_tokens":1,"output_tokens":1},"content":[{"type":"text","text":"ok"}]}}"#,
            r#"{"type":"user","uuid":"u2","promptId":"p2","timestamp":"2026-09-19T00:01:00Z","message":{"role":"user","content":"b"}}"#,
            r#"{"type":"assistant","requestId":"r2","timestamp":"2026-09-19T00:01:05Z","message":{"role":"assistant","model":"gpt-x","usage":{"input_tokens":1,"output_tokens":1},"content":[{"type":"text","text":"ok"}]}}"#,
        ]);
        assert_eq!(s.turns, 2);
        assert_eq!(s.unpriced_steps, 2);
        assert_eq!(s.cost_usd, 0.0);
        assert!(s.by_model["gpt-x"].unpriced);
        // 没有显式时长：turn 内首尾之差
        assert_eq!(s.turn_stats[0].wall_ms, 1000);
        assert_eq!(s.turn_stats[1].wall_ms, 5000);
    }
}
