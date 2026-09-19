//! 模型定价（USD / 1M tokens）。只认识 Anthropic 一方 API 的价格；别的模型返回 None，调用方按"未定价"处理。
//!
//! 价格表快照于 2026-06。cache 写按 5 分钟 TTL 算（1.25× 输入价），cache 读 0.1× 输入价，Fable 5.1 的读价另算。

use crate::event::Usage;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Price {
    pub input: f64,
    pub output: f64,
    pub cache_write: f64,
    pub cache_read: f64,
}

const fn p(input: f64, output: f64) -> Price {
    Price { input, output, cache_write: input * 1.25, cache_read: input * 0.1 }
}

/// 按前缀匹配，所以带日期后缀的 id（`claude-sonnet-4-5-20250929`）也能对上。长的在前，免得 `claude-opus-4` 抢了 `claude-opus-4-6`。
const TABLE: &[(&str, Price)] = &[
    ("claude-fable-5-1", Price { cache_read: 0.25, ..p(10.0, 50.0) }),
    ("claude-mythos-5-1", Price { cache_read: 0.25, ..p(10.0, 50.0) }),
    ("claude-fable-5", p(10.0, 50.0)),
    ("claude-mythos-5", p(10.0, 50.0)),
    ("claude-opus-5", p(5.0, 25.0)),
    ("claude-opus-4-8", p(5.0, 25.0)),
    ("claude-opus-4-7", p(5.0, 25.0)),
    ("claude-opus-4-6", p(5.0, 25.0)),
    ("claude-opus-4-5", p(5.0, 25.0)),
    ("claude-opus-4-1", p(15.0, 75.0)),
    ("claude-opus-4", p(15.0, 75.0)),
    ("claude-sonnet-5", p(2.0, 10.0)),
    ("claude-sonnet-4-6", p(3.0, 15.0)),
    ("claude-sonnet-4-5", p(3.0, 15.0)),
    ("claude-sonnet-4", p(3.0, 15.0)),
    ("claude-haiku-4-5", p(1.0, 5.0)),
    ("claude-3-5-haiku", p(0.8, 4.0)),
];

pub fn price_of(model: &str) -> Option<Price> {
    TABLE.iter().find(|(prefix, _)| model.starts_with(prefix)).map(|(_, p)| *p)
}

/// 一次调用的费用；模型不认识就 None
pub fn cost_usd(model: &str, u: &Usage) -> Option<f64> {
    let p = price_of(model)?;
    let m = |n: Option<u64>, price: f64| n.unwrap_or(0) as f64 * price / 1e6;
    Some(m(u.input, p.input) + m(u.output, p.output) + m(u.cache_write, p.cache_write) + m(u.cache_read, p.cache_read))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_match_prefers_longest() {
        assert_eq!(price_of("claude-opus-4-6").unwrap().input, 5.0);
        assert_eq!(price_of("claude-opus-4-1-20250805").unwrap().input, 15.0);
        assert_eq!(price_of("claude-opus-4-20250514").unwrap().input, 15.0);
        assert_eq!(price_of("claude-fable-5-1").unwrap().cache_read, 0.25);
        assert_eq!(price_of("claude-fable-5").unwrap().cache_read, 1.0);
        assert!(price_of("gpt-5").is_none());
    }

    #[test]
    fn cost_sums_four_buckets() {
        let u = Usage {
            input: Some(1_000_000),
            output: Some(100_000),
            cache_read: Some(2_000_000),
            cache_write: Some(400_000),
            ..Default::default()
        };
        // 5 + 2.5 + 2×0.5 + 0.4×6.25
        assert!((cost_usd("claude-opus-5", &u).unwrap() - 11.0).abs() < 1e-9);
    }
}
