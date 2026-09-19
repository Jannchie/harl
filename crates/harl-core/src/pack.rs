//! pack 文件：自描述、不可变、内容命名。
//!
//! ```text
//! "HARLPACK" u8(version=1)
//! frame*:
//!   u32 n_chunks   u32 compressed_len   u32 uncompressed_len
//!   n_chunks × ( [u8;32] hash, u32 len )      顺序 = 未压缩数据里的顺序
//!   zstd(compressed_len bytes)
//! ```
//!
//! 头里带 hash 和长度，所以 index.db 丢了可以从 pack 完整重建。

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::hash::Hash;

const MAGIC: &[u8; 8] = b"HARLPACK";
const VERSION: u8 = 1;
/// 未压缩数据攒到这么多就封一个 frame
const FRAME_TARGET: usize = 4 << 20;
const ZSTD_LEVEL: i32 = 3;
const HEADER_LEN: usize = 12;
const ENTRY_LEN: usize = 32 + 4;

/// chunk 在某个 pack 里的位置；pack 名由持有者记
#[derive(Debug, Clone, Copy)]
pub struct Loc {
    pub frame_off: u64,
    pub inner_off: u32,
    pub len: u32,
}

/// 一个写完（或扫描完）的 pack
#[derive(Debug)]
pub struct Finished {
    pub name: String,
    pub bytes: u64,
    pub frames: u32,
    pub locs: Vec<(Hash, Loc)>,
}

/// 正在写的 pack：先写到 .tmp，finish 时按内容 hash 改名。没 finish 就 drop 会删掉 .tmp。
pub struct Writer {
    tmp: PathBuf,
    file: File,
    hasher: blake3::Hasher,
    pos: u64,
    buf: Vec<u8>,
    entries: Vec<(Hash, u32)>,
    locs: Vec<(Hash, Loc)>,
    frames: u32,
}

impl Writer {
    /// 先写到 `.writing-<pid>.tmp`，写的期间持有文件锁；顺手清掉别的进程崩溃留下的、已经没人锁着的 tmp。
    pub fn create(dir: &Path) -> Result<Self> {
        fs::create_dir_all(dir)?;
        sweep_orphan_tmps(dir);
        let tmp = dir.join(tmp_name(std::process::id()));
        let file = File::create(&tmp)?;
        file.lock()?;
        let mut w = Self {
            file,
            tmp,
            hasher: blake3::Hasher::new(),
            pos: 0,
            buf: Vec::with_capacity(FRAME_TARGET),
            entries: Vec::new(),
            locs: Vec::new(),
            frames: 0,
        };
        w.write(MAGIC)?;
        w.write(&[VERSION])?;
        Ok(w)
    }

    pub fn add(&mut self, hash: Hash, bytes: &[u8]) -> Result<()> {
        self.buf.extend_from_slice(bytes);
        self.entries.push((hash, bytes.len() as u32));
        if self.buf.len() >= FRAME_TARGET {
            self.flush_frame()?;
        }
        Ok(())
    }

    fn write(&mut self, bytes: &[u8]) -> Result<()> {
        self.file.write_all(bytes)?;
        self.hasher.update(bytes);
        self.pos += bytes.len() as u64;
        Ok(())
    }

    fn flush_frame(&mut self) -> Result<()> {
        if self.entries.is_empty() {
            return Ok(());
        }
        let compressed = zstd::bulk::compress(&self.buf, ZSTD_LEVEL)?;
        let frame_off = self.pos;
        let mut header = Vec::with_capacity(HEADER_LEN + self.entries.len() * ENTRY_LEN);
        header.extend_from_slice(&(self.entries.len() as u32).to_le_bytes());
        header.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        header.extend_from_slice(&(self.buf.len() as u32).to_le_bytes());
        for (hash, len) in &self.entries {
            header.extend_from_slice(hash);
            header.extend_from_slice(&len.to_le_bytes());
        }
        self.locs.extend(locs_for(frame_off, self.entries.drain(..)));
        self.write(&header)?;
        self.write(&compressed)?;
        self.frames += 1;
        self.buf.clear();
        Ok(())
    }

    /// 封尾、fsync、按内容 hash 命名。空 pack 直接删掉返回 None。
    pub fn finish(mut self) -> Result<Option<Finished>> {
        self.flush_frame()?;
        if self.frames == 0 {
            return Ok(None);
        }
        self.file.sync_all()?;
        let name = format!("{}.pack", &self.hasher.finalize().to_hex()[..16]);
        let dest = self.tmp.with_file_name(&name);
        if dest.exists() {
            // 内容完全相同的 pack 已存在（理论上只会在重复导入时发生）
            fs::remove_file(&self.tmp)?;
        } else {
            fs::rename(&self.tmp, &dest)?;
        }
        Ok(Some(Finished { name, bytes: self.pos, frames: self.frames, locs: std::mem::take(&mut self.locs) }))
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        // finish 成功时 tmp 已改名，这里删不到就算了；空 pack、提前返回（读文件出错、panic……）留下的半个 pack 在这里清掉
        let _ = fs::remove_file(&self.tmp);
    }
}

fn tmp_name(pid: u32) -> String {
    format!(".writing-{pid}.tmp")
}

/// 删掉没人锁着的 `.writing-*.tmp`：写它的进程已经不在了。正在写的那些锁着，try_lock 拿不到就跳过。
fn sweep_orphan_tmps(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for e in entries.filter_map(Result::ok) {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if !(name.starts_with(".writing-") && name.ends_with(".tmp")) {
            continue;
        }
        let Ok(f) = File::open(e.path()) else { continue };
        if f.try_lock().is_ok() {
            drop(f);
            let _ = fs::remove_file(e.path());
        }
    }
}

/// 按 (hash, len) 顺序算出每个 chunk 在 frame 内的偏移
fn locs_for(frame_off: u64, entries: impl Iterator<Item = (Hash, u32)>) -> impl Iterator<Item = (Hash, Loc)> {
    let mut inner = 0u32;
    entries.map(move |(hash, len)| {
        let loc = Loc { frame_off, inner_off: inner, len };
        inner += len;
        (hash, loc)
    })
}

struct FrameHeader {
    n: usize,
    clen: usize,
    ulen: usize,
}

impl FrameHeader {
    /// frame 结束的偏移。头、entries、压缩数据都得落在文件内，否则就是截断的 pack
    fn end(&self, pack: &str, pos: u64, total: u64) -> Result<u64> {
        let end = pos + (HEADER_LEN + self.n * ENTRY_LEN + self.clen) as u64;
        if end > total {
            bail!("{pack}: frame@{pos} 越出文件末尾（pack 被截断？）");
        }
        Ok(end)
    }
}

fn read_header(f: &mut File, pack: &str, pos: u64) -> Result<FrameHeader> {
    let mut fixed = [0u8; HEADER_LEN];
    f.read_exact(&mut fixed).with_context(|| format!("{pack}: frame@{pos} 头不完整（pack 被截断？）"))?;
    let u = |i: usize| u32::from_le_bytes(fixed[i..i + 4].try_into().unwrap()) as usize;
    Ok(FrameHeader { n: u(0), clen: u(4), ulen: u(8) })
}

/// 解压一个 frame
pub fn read_frame(dir: &Path, pack: &str, frame_off: u64) -> Result<Vec<u8>> {
    let mut f = File::open(dir.join(pack)).with_context(|| format!("打开 pack {pack}"))?;
    let total = f.metadata()?.len();
    f.seek(SeekFrom::Start(frame_off))?;
    let h = read_header(&mut f, pack, frame_off)?;
    h.end(pack, frame_off, total)?;
    f.seek(SeekFrom::Current((h.n * ENTRY_LEN) as i64))?;
    let mut compressed = vec![0u8; h.clen];
    f.read_exact(&mut compressed)?;
    let data = zstd::bulk::decompress(&compressed, h.ulen)?;
    if data.len() != h.ulen {
        bail!("pack {pack} frame@{frame_off}: 解压长度不符");
    }
    Ok(data)
}

/// 只读 pack 的头，产出每个 chunk 的位置（重建索引用）
pub fn scan(dir: &Path, pack: &str) -> Result<Finished> {
    let mut f = File::open(dir.join(pack))?;
    let mut magic = [0u8; 9];
    f.read_exact(&mut magic)?;
    if &magic[..8] != MAGIC || magic[8] != VERSION {
        bail!("{pack}: 不是 v{VERSION} pack");
    }
    let total = f.metadata()?.len();
    let mut pos = 9u64;
    let mut locs = Vec::new();
    let mut frames = 0u32;
    while pos < total {
        let h = read_header(&mut f, pack, pos)?;
        let end = h.end(pack, pos, total)?;
        let mut entries = vec![0u8; h.n * ENTRY_LEN];
        f.read_exact(&mut entries)?;
        let before = locs.len();
        locs.extend(locs_for(
            pos,
            entries.chunks_exact(ENTRY_LEN).map(|e| {
                (e[..32].try_into().unwrap(), u32::from_le_bytes(e[32..].try_into().unwrap()))
            }),
        ));
        let sum: u64 = locs[before..].iter().map(|(_, l)| l.len as u64).sum();
        if sum != h.ulen as u64 {
            bail!("{pack}: frame@{pos} entries 长度之和 {sum} ≠ 未压缩长度 {}", h.ulen);
        }
        f.seek(SeekFrom::Start(end))?;
        pos = end;
        frames += 1;
    }
    Ok(Finished { name: pack.to_string(), bytes: total, frames, locs })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::hash::hash;

    /// temp_dir 下的唯一子目录，drop 时删掉
    pub(crate) struct TempDir(pub PathBuf);

    impl TempDir {
        pub(crate) fn new(name: &str) -> Self {
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
            let dir = std::env::temp_dir().join(format!("harl-test-{name}-{}-{nanos}", std::process::id()));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn tmp_name() -> String {
        super::tmp_name(std::process::id())
    }

    /// 一个超过 FRAME_TARGET 的大块把第一帧撑满，后面的小块（含空块）落到第二帧
    fn chunks() -> Vec<Vec<u8>> {
        vec![vec![b'a'; FRAME_TARGET + 1], b"line one\n".to_vec(), b"line two\n".to_vec(), Vec::new()]
    }

    fn write_pack(dir: &Path) -> (Finished, Vec<Vec<u8>>) {
        let data = chunks();
        let mut w = Writer::create(dir).unwrap();
        for c in &data {
            w.add(hash(c), c).unwrap();
        }
        (w.finish().unwrap().unwrap(), data)
    }

    fn flat(locs: &[(Hash, Loc)]) -> Vec<(Hash, u64, u32, u32)> {
        locs.iter().map(|(h, l)| (*h, l.frame_off, l.inner_off, l.len)).collect()
    }

    #[test]
    fn roundtrip_write_scan_read() {
        let t = TempDir::new("pack-roundtrip");
        let (done, data) = write_pack(&t.0);
        assert_eq!(done.frames, 2);
        assert!(!t.0.join(tmp_name()).exists());
        assert_eq!(fs::metadata(t.0.join(&done.name)).unwrap().len(), done.bytes);

        let scanned = scan(&t.0, &done.name).unwrap();
        assert_eq!(scanned.frames, 2);
        assert_eq!(scanned.bytes, done.bytes);
        assert_eq!(flat(&scanned.locs), flat(&done.locs));

        for ((h, loc), want) in scanned.locs.iter().zip(&data) {
            let frame = read_frame(&t.0, &done.name, loc.frame_off).unwrap();
            let got = &frame[loc.inner_off as usize..(loc.inner_off + loc.len) as usize];
            assert_eq!(got, want.as_slice());
            assert_eq!(hash(got), *h);
        }
    }

    #[test]
    fn truncated_pack_rejected() {
        let t = TempDir::new("pack-truncated");
        let (done, _) = write_pack(&t.0);
        let bytes = fs::read(t.0.join(&done.name)).unwrap();
        let last = done.locs.last().unwrap().1.frame_off as usize;
        // 分别截在最后一帧的压缩数据里、entries 里、头里
        for cut in [bytes.len() - 1, last + HEADER_LEN + 3, last + 5] {
            fs::write(t.0.join("cut.pack"), &bytes[..cut]).unwrap();
            let err = scan(&t.0, "cut.pack").unwrap_err().to_string();
            assert!(err.contains("截断"), "cut@{cut}: {err}");
            assert!(read_frame(&t.0, "cut.pack", last as u64).is_err(), "cut@{cut}");
        }
    }

    #[test]
    fn entry_len_mismatch_rejected() {
        let t = TempDir::new("pack-entrylen");
        let (done, _) = write_pack(&t.0);
        let mut bytes = fs::read(t.0.join(&done.name)).unwrap();
        // 把第二帧第一个 entry 的 len 加一
        let off = done.locs.last().unwrap().1.frame_off as usize + HEADER_LEN + 32;
        bytes[off] = bytes[off].wrapping_add(1);
        fs::write(t.0.join("bad.pack"), &bytes).unwrap();
        let err = scan(&t.0, "bad.pack").unwrap_err().to_string();
        assert!(err.contains("长度之和"), "{err}");
    }

    #[test]
    fn unfinished_writer_removes_tmp() {
        let t = TempDir::new("pack-drop");
        let tmp = t.0.join(tmp_name());
        let mut w = Writer::create(&t.0).unwrap();
        w.add(hash(b"x"), b"x").unwrap();
        assert!(tmp.exists());
        drop(w);
        assert!(!tmp.exists());

        // 空 pack finish 也不留东西
        assert!(Writer::create(&t.0).unwrap().finish().unwrap().is_none());
        assert!(!tmp.exists());
    }

    /// 没人锁着的 tmp 是崩溃留下的，建 Writer 时顺手删；别的进程正在写的（锁着）留下
    #[test]
    fn create_sweeps_unlocked_tmps_only() {
        let t = TempDir::new("pack-sweep");
        let (stale, busy) = (t.0.join(".writing-1.tmp"), t.0.join(".writing-2.tmp"));
        fs::write(&stale, b"HARLPACK").unwrap();
        fs::write(&busy, b"HARLPACK").unwrap();
        let holder = File::open(&busy).unwrap();
        holder.lock().unwrap();

        let w = Writer::create(&t.0).unwrap();
        assert!(!stale.exists());
        assert!(busy.exists());
        drop(w);
        drop(holder);
    }
}
