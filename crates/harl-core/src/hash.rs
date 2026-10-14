//! 内容寻址的原语：blake3，hex 表示。pack、store、canonical 层的 prov 都用它。

use anyhow::{Context, Result};

pub type Hash = [u8; 32];

pub fn hash(b: &[u8]) -> Hash {
    *blake3::hash(b).as_bytes()
}

pub fn hex(h: &Hash) -> String {
    blake3::Hash::from_bytes(*h).to_hex().to_string()
}

pub fn unhex(s: &str) -> Result<Hash> {
    Ok(*blake3::Hash::from_hex(s).context("not a valid hash")?.as_bytes())
}
