//! harl — raw 层（行级内容寻址 + zstd pack，逐字节可逆）和 canonical 层（跨 harness 的事件流）。

pub mod event;
pub mod hash;
pub mod normalize;
pub mod pack;
pub mod pricing;
pub mod stats;
pub mod store;
