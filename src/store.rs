//! 存储：manifest（事实来源）+ packs（chunk 实体）+ index.db（可重建的位置索引）。
//!
//! ```text
//! machine-id
//! raw/manifest.jsonl    只追加：一个源文件每次内容变化写一条 {source, tree, blob, …}
//! raw/packs/*.pack      chunk（行）和 tree（行 hash 列表）都存在这里
//! index.db              chunk hash → pack 位置；packs 清单
//! ```
//!
//! chunk = 一行（含结尾 `\n`；文件末尾没有换行的半行单独一个 chunk）。
//! tree  = 两级的行 hash 列表：行 hash 每 BLOCK_LINES 个拼成一个块，块也是 chunk；
//!         tree 本身是块 hash 顺序拼接的 chunk（空文件 = 零块 = 空 chunk）。
//! 于是 concat(每个块里每个行 chunk) == 原文件，逐字节。
//! JSONL 只追加，所以再次快照时除最后一块外都已存在，增量只有新行 + 最后一块 + 顶层列表。
//!
//! manifest 记录的 `v` 决定 tree 的编码：v≤2 是平铺的一级列表（tree chunk 直接是行 hash），v3 是上面的两级。
//! 旧记录照旧能读，新记录只写 v3。

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::pack::{self, Finished, Hash, Loc, hash, hex, unhex};

const MANIFEST_VERSION: u32 = 3;
/// 一个 tree 块最多装这么多行 hash（32 KiB）。
/// 每次 capture 固定重存最后一块（平均半块 16 KiB）加顶层列表（每 1024 行 32 B），
/// 与一次增量的新行本身（通常几十 KiB）同量级；再小块数就多了，index.db 每块一行、顶层列表也随之变长。
const BLOCK_LINES: usize = 1024;

/// manifest.jsonl 的一行
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Record {
    /// 记录格式版本，同时决定 tree 的编码（见模块注释）
    pub v: u32,
    pub provider: String,
    pub machine: String,
    pub source: String,
    /// tree 对象的 hash：v3 是块 hash 列表（每块是 ≤ BLOCK_LINES 个行 hash），v≤2 是平铺的行 hash 列表
    pub tree: String,
    /// 整个文件的 blake3，check 时的比对目标
    pub blob: String,
    pub size: u64,
    pub lines: u32,
    pub mtime: DateTime<Utc>,
    pub captured_at: DateTime<Utc>,
}

/// 每个 (machine, source) 的最新记录
pub type Latest = BTreeMap<(String, String), Record>;

pub fn latest_of(records: Vec<Record>) -> Latest {
    records.into_iter().map(|r| ((r.machine.clone(), r.source.clone()), r)).collect()
}

pub struct Stats {
    pub packs: u64,
    pub pack_bytes: u64,
    pub chunks: u64,
    pub index_bytes: u64,
}

pub struct Store {
    root: PathBuf,
    machine: String,
    db: Connection,
}

impl Store {
    pub fn open(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(root.join("raw/packs"))?;
        let id_path = root.join("machine-id");
        let machine = match fs::read_to_string(&id_path) {
            Ok(s) => s.trim().to_string(),
            Err(_) => {
                let id = new_machine_id();
                fs::write(&id_path, &id)?;
                id
            }
        };
        let db = Connection::open(root.join("index.db"))?;
        db.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             CREATE TABLE IF NOT EXISTS chunks(
               hash BLOB PRIMARY KEY, pack TEXT NOT NULL,
               frame_off INTEGER NOT NULL, inner_off INTEGER NOT NULL, len INTEGER NOT NULL
             ) WITHOUT ROWID;
             CREATE TABLE IF NOT EXISTS packs(
               name TEXT PRIMARY KEY, bytes INTEGER NOT NULL, frames INTEGER NOT NULL, chunks INTEGER NOT NULL
             );",
        )?;
        Ok(Self { root, machine, db })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn machine(&self) -> &str {
        &self.machine
    }

    fn packs_dir(&self) -> PathBuf {
        self.root.join("raw/packs")
    }

    fn manifest_path(&self) -> PathBuf {
        self.root.join("raw/manifest.jsonl")
    }

    // ---- manifest ----

    /// 逐行解析。坏行（半行、非 JSON）和版本比本程序新的行都 warning 跳过，不让一行毁掉整个 manifest。
    pub fn records(&self) -> Result<Vec<Record>> {
        let Ok(file) = fs::File::open(self.manifest_path()) else {
            return Ok(Vec::new());
        };
        let mut records = Vec::new();
        for (i, line) in BufReader::new(file).split(b'\n').enumerate() {
            let line = line.context("读 manifest")?;
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            match serde_json::from_slice::<Record>(&line) {
                Ok(rec) if rec.v > MANIFEST_VERSION => {
                    eprintln!("warning: manifest 第 {} 行是 v{}，本程序只认到 v{MANIFEST_VERSION}，已跳过", i + 1, rec.v);
                }
                Ok(rec) => records.push(rec),
                Err(_) => eprintln!("warning: manifest 第 {} 行损坏，已跳过", i + 1),
            }
        }
        Ok(records)
    }

    pub fn latest(&self) -> Result<Latest> {
        Ok(latest_of(self.records()?))
    }

    fn append_records(&self, recs: &[Record]) -> Result<()> {
        if recs.is_empty() {
            return Ok(());
        }
        let mut file = OpenOptions::new().create(true).append(true).read(true).open(self.manifest_path())?;
        // 上次崩溃可能留下没写完的半行；先补个换行隔开，别让新记录接在它后面一起坏掉
        let len = file.metadata()?.len();
        if len > 0 {
            let mut last = [0u8; 1];
            file.seek(SeekFrom::Start(len - 1))?;
            file.read_exact(&mut last)?;
            if last != [b'\n'] {
                file.write_all(b"\n")?;
            }
        }
        // 整批一次 write：进程被杀不会留半行（掉电仍可能，靠上面的补换行和 records() 的容错兜底）
        let mut buf = Vec::new();
        for rec in recs {
            serde_json::to_writer(&mut buf, rec)?;
            buf.push(b'\n');
        }
        file.write_all(&buf)?;
        file.sync_all()?;
        Ok(())
    }

    // ---- chunks ----

    fn loc(&self, hash: &Hash) -> Result<Option<(String, Loc)>> {
        Ok(self
            .db
            .prepare_cached("SELECT pack, frame_off, inner_off, len FROM chunks WHERE hash=?1")?
            .query_row(params![&hash[..]], |r| {
                Ok((
                    r.get(0)?,
                    Loc { frame_off: r.get::<_, i64>(1)? as u64, inner_off: r.get(2)?, len: r.get(3)? },
                ))
            })
            .optional()?)
    }

    fn known_chunks(&self) -> Result<HashSet<Hash>> {
        let mut stmt = self.db.prepare("SELECT hash FROM chunks")?;
        let rows = stmt.query_map([], |r| r.get::<_, Vec<u8>>(0))?;
        rows.map(|r| Ok(r?.as_slice().try_into()?)).collect()
    }

    /// 从 packs 目录重建 index.db，返回 (packs, chunks)。
    /// 全程一个事务：任一 pack 坏了就整体回滚，旧索引原样保留。
    pub fn reindex(&mut self) -> Result<(usize, usize)> {
        let mut names: Vec<String> = fs::read_dir(self.packs_dir())?
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x == "pack"))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        let packs_dir = self.packs_dir();
        let tx = self.db.transaction()?;
        tx.execute_batch("DELETE FROM chunks; DELETE FROM packs;")?;
        let mut chunks = 0;
        for name in &names {
            chunks += register_pack(&tx, pack::scan(&packs_dir, name)?)?;
        }
        tx.commit()?;
        Ok((names.len(), chunks))
    }

    pub fn stats(&self) -> Result<Stats> {
        let (packs, pack_bytes): (i64, i64) = self
            .db
            .query_row("SELECT COUNT(*), COALESCE(SUM(bytes),0) FROM packs", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
        let chunks: i64 = self.db.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))?;
        let index_bytes = fs::metadata(self.root.join("index.db")).map(|m| m.len()).unwrap_or(0);
        Ok(Stats { packs: packs as u64, pack_bytes: pack_bytes as u64, chunks: chunks as u64, index_bytes })
    }

    pub fn reader(&self) -> Reader<'_> {
        Reader { store: self, frames: VecDeque::new() }
    }

    pub fn batch(&mut self) -> Result<Batch<'_>> {
        Batch::new(self)
    }
}

// ---- 读 ----

/// 最近 FRAME_CACHE 个已解压 frame（每个 ≤ 4 MiB），LRU。
/// 按 tree 顺序读时相邻 chunk 多在同一 frame；跨文件去重的行会跳到别的 frame，多缓存几个避免来回解压。
const FRAME_CACHE: usize = 16;

pub struct Reader<'a> {
    store: &'a Store,
    frames: VecDeque<(String, u64, Vec<u8>)>,
}

impl Reader<'_> {
    /// 把一个 chunk 追加到 out
    pub fn chunk_into(&mut self, hash: &Hash, out: &mut Vec<u8>) -> Result<()> {
        let (pack, loc) = self.store.loc(hash)?.with_context(|| format!("chunk 不在索引里: {}", hex(hash)))?;
        let pos = self.frames.iter().position(|(p, off, _)| *p == pack && *off == loc.frame_off);
        match pos {
            Some(0) => {}
            Some(i) => {
                let f = self.frames.remove(i).unwrap();
                self.frames.push_front(f);
            }
            None => {
                if self.frames.len() >= FRAME_CACHE {
                    self.frames.pop_back();
                }
                let data = pack::read_frame(&self.store.packs_dir(), &pack, loc.frame_off)?;
                self.frames.push_front((pack, loc.frame_off, data));
            }
        }
        let data = &self.frames[0].2;
        let (s, e) = (loc.inner_off as usize, (loc.inner_off + loc.len) as usize);
        if e > data.len() {
            bail!("chunk 越界: {}", hex(hash));
        }
        out.extend_from_slice(&data[s..e]);
        Ok(())
    }

    /// 读一个"hash 列表"chunk
    fn hashes(&mut self, h: &Hash) -> Result<Vec<Hash>> {
        let mut bytes = Vec::new();
        self.chunk_into(h, &mut bytes)?;
        if bytes.len() % 32 != 0 {
            bail!("hash 列表对象长度不是 32 的倍数: {}", hex(h));
        }
        Ok(bytes.chunks_exact(32).map(|c| c.try_into().unwrap()).collect())
    }

    /// 按顺序对快照的每个行 hash 调 f。编码由记录版本决定，所以要拿 Record 而不是裸 tree hash。
    /// v3 一块一块走：块 chunk 和它的行写在同一个 frame 里，紧挨着读能命中缓存，不用先把全部行 hash 物化出来。
    fn walk(&mut self, rec: &Record, mut f: impl FnMut(&mut Self, Hash) -> Result<()>) -> Result<()> {
        let tree = unhex(&rec.tree).with_context(|| format!("记录的 tree {:?}", rec.tree))?;
        if rec.v <= 2 {
            for h in self.hashes(&tree)? {
                f(self, h)?;
            }
            return Ok(());
        }
        for b in self.hashes(&tree)? {
            for h in self.hashes(&b)? {
                f(self, h)?;
            }
        }
        Ok(())
    }

    /// 逐字节还原一个快照
    pub fn reconstruct(&mut self, rec: &Record) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(rec.size as usize);
        self.walk(rec, |r, h| r.chunk_into(&h, &mut out))?;
        Ok(out)
    }

    /// 快照的每一行，连同它的 chunk hash（normalizer 用它做 prov）
    pub fn lines(&mut self, rec: &Record) -> Result<Vec<(Hash, Vec<u8>)>> {
        let mut lines = Vec::with_capacity(rec.lines as usize);
        self.walk(rec, |r, h| {
            let mut line = Vec::new();
            r.chunk_into(&h, &mut line)?;
            lines.push((h, line));
            Ok(())
        })?;
        Ok(lines)
    }
}

// ---- 写 ----

pub enum Outcome {
    New,
    Changed,
    Unchanged,
}

pub struct Committed {
    /// 新 pack 的名字和字节数；没有新 chunk 时为 None
    pub pack: Option<(String, u64)>,
    pub stored_bytes: u64,
    pub new_chunks: usize,
    /// 这次存下的快照的逻辑大小之和
    pub logical: u64,
}

/// 一次 capture 的写入会话：攒新 chunk，最后一起落盘。
/// 已知 chunk 集合和 pack writer 到第一次真要写时才建：定时 capture 的常态是全部没变，那时什么都不用碰。
pub struct Batch<'a> {
    store: &'a mut Store,
    /// 本机每个 source 的最新记录
    latest: BTreeMap<String, Record>,
    known: Option<HashSet<Hash>>,
    writer: Option<pack::Writer>,
    records: Vec<Record>,
    stored_bytes: u64,
    new_chunks: usize,
    logical: u64,
}

impl<'a> Batch<'a> {
    fn new(store: &'a mut Store) -> Result<Self> {
        let latest = store
            .latest()?
            .into_iter()
            .filter(|((m, _), _)| *m == store.machine)
            .map(|((_, source), rec)| (source, rec))
            .collect();
        Ok(Self {
            latest,
            known: None,
            writer: None,
            store,
            records: Vec::new(),
            stored_bytes: 0,
            new_chunks: 0,
            logical: 0,
        })
    }

    fn put(&mut self, h: Hash, bytes: &[u8]) -> Result<()> {
        let known = match &mut self.known {
            Some(k) => k,
            None => self.known.insert(self.store.known_chunks()?),
        };
        if !known.insert(h) {
            return Ok(());
        }
        let writer = match &mut self.writer {
            Some(w) => w,
            None => self.writer.insert(pack::Writer::create(&self.store.packs_dir())?),
        };
        writer.add(h, bytes)?;
        self.stored_bytes += bytes.len() as u64;
        self.new_chunks += 1;
        Ok(())
    }

    fn prev(&self, source: &str) -> Option<&Record> {
        self.latest.get(source)
    }

    /// size/mtime 与上次一致就当没变，调用方可以不读文件（rsync 式的取舍）
    pub fn unchanged_by_stat(&self, source: &str, size: u64, mtime: DateTime<Utc>) -> bool {
        self.prev(source).is_some_and(|p| p.size == size && p.mtime == mtime)
    }

    /// 存一个源文件的快照。内容 hash 与上次一致就不存。
    /// 读文件由调用方负责，所以这里出的错都是存储层的。
    pub fn put_snapshot(
        &mut self,
        provider: &str,
        source: &str,
        mtime: DateTime<Utc>,
        bytes: &[u8],
    ) -> Result<Outcome> {
        let prev = self.prev(source);
        let blob = hex(&hash(bytes));
        let outcome = match prev {
            Some(p) if p.blob == blob => return Ok(Outcome::Unchanged),
            Some(_) => Outcome::Changed,
            None => Outcome::New,
        };

        let lines: Vec<&[u8]> = bytes.split_inclusive(|&c| c == b'\n').collect();
        // 每 BLOCK_LINES 行的 hash 拼成一块；块 hash 拼成顶层 tree。满块的 hash 只取决于那些行，再次快照时直接去重
        let mut tree_bytes = Vec::with_capacity(lines.len().div_ceil(BLOCK_LINES) * 32);
        let mut block_bytes = Vec::with_capacity(BLOCK_LINES * 32);
        for block in lines.chunks(BLOCK_LINES) {
            block_bytes.clear();
            for line in block {
                let h = hash(line);
                block_bytes.extend_from_slice(&h);
                self.put(h, line)?;
            }
            let bh = hash(&block_bytes);
            tree_bytes.extend_from_slice(&bh);
            self.put(bh, &block_bytes)?;
        }
        let tree = hash(&tree_bytes);
        self.put(tree, &tree_bytes)?;

        self.logical += bytes.len() as u64;
        self.records.push(Record {
            v: MANIFEST_VERSION,
            provider: provider.into(),
            machine: self.store.machine.clone(),
            source: source.into(),
            tree: hex(&tree),
            blob,
            size: bytes.len() as u64,
            lines: lines.len() as u32,
            mtime,
            captured_at: Utc::now(),
        });
        Ok(outcome)
    }

    /// 顺序：pack 落盘 → 索引提交 → manifest 追加。中途崩溃只会多出孤儿 chunk，不会有指向空处的记录。
    pub fn commit(self) -> Result<Committed> {
        let mut pack = None;
        if let Some(done) = self.writer.map(pack::Writer::finish).transpose()?.flatten() {
            pack = Some((done.name.clone(), done.bytes));
            let tx = self.store.db.transaction()?;
            register_pack(&tx, done)?;
            tx.commit()?;
        }
        self.store.append_records(&self.records)?;
        Ok(Committed { pack, stored_bytes: self.stored_bytes, new_chunks: self.new_chunks, logical: self.logical })
    }
}

/// 把一个写完（或扫描完）的 pack 登记进索引，返回实际新插入的 chunk 数（已在别的 pack 里的不算）。
/// packs.chunks 记的是 pack 自己装了多少块，与登记顺序无关。调用方负责事务。按 hash 排序后插入，B-tree 走顺序插入路径。
fn register_pack(db: &Connection, mut done: Finished) -> Result<usize> {
    done.locs.sort_unstable_by_key(|(h, _)| *h);
    let mut ins = db
        .prepare_cached("INSERT OR IGNORE INTO chunks(hash, pack, frame_off, inner_off, len) VALUES(?1,?2,?3,?4,?5)")?;
    let mut inserted = 0;
    for (h, l) in &done.locs {
        inserted += ins.execute(params![&h[..], done.name, l.frame_off as i64, l.inner_off, l.len])?;
    }
    db.execute(
        "INSERT OR REPLACE INTO packs(name, bytes, frames, chunks) VALUES(?1,?2,?3,?4)",
        params![done.name, done.bytes as i64, done.frames, done.locs.len() as i64],
    )?;
    Ok(inserted)
}

/// `(machine, source)` 里 source 的规范形式：绝对路径、`/` 分隔、`.`/`..` 词法折叠掉（不解 symlink）。
/// 不同 root 或相对路径扫到同一个文件得到同一个 key；Windows 上记下的 key 在 Linux 上也能读。
pub fn source_key(path: &Path) -> String {
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut out = PathBuf::new();
    for c in abs.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            c => out.push(c),
        }
    }
    out.to_string_lossy().replace('\\', "/")
}

fn new_machine_id() -> String {
    let seed = format!(
        "{}|{}|{}",
        std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_default(),
        std::process::id(),
        Utc::now().timestamp_nanos_opt().unwrap_or_default()
    );
    hex(&hash(seed.as_bytes()))[..32].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::tests::TempDir;

    const P: &str = "claude";

    /// 超过一个 frame（4 MiB），每行都不同
    fn big() -> Vec<u8> {
        let mut v = Vec::new();
        for i in 0..120_000u32 {
            writeln!(v, "{{\"i\":{i},\"t\":\"{}\"}}", "x".repeat((i % 50) as usize)).unwrap();
        }
        assert!(v.len() > 4 << 20);
        v
    }

    fn samples() -> Vec<(&'static str, Vec<u8>)> {
        vec![
            ("/h/.claude/projects/p/empty.jsonl", Vec::new()),
            ("/h/.claude/projects/p/notail.jsonl", b"{\"a\":1}\n{\"b\":2}".to_vec()),
            ("/h/.claude/projects/p/crlf.jsonl", b"{\"a\":1}\r\n{\"b\":2}\r\n".to_vec()),
            ("/h/.claude/projects/p/big.jsonl", big()),
        ]
    }

    fn rec<'a>(latest: &'a Latest, store: &Store, source: &str) -> &'a Record {
        &latest[&(store.machine().to_string(), source.to_string())]
    }

    /// 每个 sample 的最新快照都能逐字节还原
    fn assert_reconstructs(store: &Store, samples: &[(&str, Vec<u8>)]) {
        let latest = store.latest().unwrap();
        let mut reader = store.reader();
        for (source, want) in samples {
            let r = rec(&latest, store, source);
            let got = reader.reconstruct(r).unwrap();
            assert_eq!(&got, want, "{source}");
            assert_eq!(hex(&hash(want)), r.blob, "{source}");
            assert_eq!(r.size, want.len() as u64, "{source}");
        }
    }

    fn capture_all(store: &mut Store, samples: &[(&str, Vec<u8>)]) -> Committed {
        let now = Utc::now();
        let mut batch = store.batch().unwrap();
        for (source, bytes) in samples {
            batch.put_snapshot(P, source, now, bytes).unwrap();
        }
        batch.commit().unwrap()
    }

    #[test]
    fn roundtrip_empty_notail_crlf_multiframe() {
        let t = TempDir::new("store-roundtrip");
        let mut store = Store::open(t.0.clone()).unwrap();
        let samples = samples();
        let now = Utc::now();

        let mut batch = store.batch().unwrap();
        for (source, bytes) in &samples {
            assert!(matches!(batch.put_snapshot(P, source, now, bytes).unwrap(), Outcome::New));
        }
        let done = batch.commit().unwrap();
        assert!(done.pack.is_some());
        assert_reconstructs(&store, &samples);

        let latest = store.latest().unwrap();
        assert_eq!(rec(&latest, &store, samples[0].0).lines, 0);
        assert_eq!(rec(&latest, &store, samples[1].0).lines, 2);
        assert_eq!(rec(&latest, &store, samples[2].0).lines, 2);
        assert_eq!(rec(&latest, &store, samples[3].0).lines, 120_000);

        // 第二轮：内容一样 Unchanged；追加一行 Changed，只多存新行、新的最后一块和新 tree
        let mut batch = store.batch().unwrap();
        let (src1, bytes1) = &samples[1];
        assert!(batch.unchanged_by_stat(src1, bytes1.len() as u64, now));
        assert!(!batch.unchanged_by_stat(src1, bytes1.len() as u64 + 1, now));
        assert!(matches!(batch.put_snapshot(P, src1, now, bytes1).unwrap(), Outcome::Unchanged));
        let mut grown = samples[3].1.clone();
        grown.extend_from_slice(b"{\"z\":1}\n");
        assert!(matches!(batch.put_snapshot(P, samples[3].0, now, &grown).unwrap(), Outcome::Changed));
        let done = batch.commit().unwrap();
        assert_eq!(done.new_chunks, 3);
        assert_reconstructs(&store, &[(samples[3].0, grown)]);
        assert_eq!(store.records().unwrap().len(), 5);
        assert_eq!(store.stats().unwrap().packs, 2);
        assert!(!t.0.join("raw/packs").join(format!(".writing-{}.tmp", std::process::id())).exists());
    }

    #[test]
    fn manifest_bad_lines_skipped() {
        let t = TempDir::new("store-manifest");
        let mut store = Store::open(t.0.clone()).unwrap();
        let samples = samples();
        capture_all(&mut store, &samples[..2]);

        // 追加：空行、版本太新的一行、半行（模拟写到一半崩溃）
        let mut future = store.records().unwrap()[0].clone();
        future.v = MANIFEST_VERSION + 1;
        let mut f = OpenOptions::new().append(true).open(store.manifest_path()).unwrap();
        writeln!(f).unwrap();
        serde_json::to_writer(&mut f, &future).unwrap();
        writeln!(f).unwrap();
        f.write_all(b"{\"v\":2,\"provider\":\"cla").unwrap();
        drop(f);

        assert_eq!(store.records().unwrap().len(), 2);
        assert_reconstructs(&store, &samples[..2]);

        // 半行之后再 capture，新记录不能接在半行后面一起坏掉
        capture_all(&mut store, &samples[2..]);
        assert_eq!(store.records().unwrap().len(), 4);
        assert_reconstructs(&store, &samples);
    }

    #[test]
    fn reindex_rebuilds_and_fails_atomically() {
        let t = TempDir::new("store-reindex");
        let mut store = Store::open(t.0.clone()).unwrap();
        let samples = samples();
        let done = capture_all(&mut store, &samples);
        let pack_name = done.pack.unwrap().0;
        let packs_dir = store.packs_dir();

        store.db.execute_batch("DELETE FROM chunks; DELETE FROM packs;").unwrap();
        assert!(store.reader().reconstruct(&store.records().unwrap()[1]).is_err());
        let (packs, chunks) = store.reindex().unwrap();
        assert_eq!((packs, chunks), (1, done.new_chunks));
        assert_reconstructs(&store, &samples);

        // 同样的 chunk 出现在两个 pack：只登记一次，packs.chunks 只算真正插入的
        fs::copy(packs_dir.join(&pack_name), packs_dir.join("0000000000000000.pack")).unwrap();
        let (packs, chunks) = store.reindex().unwrap();
        assert_eq!((packs, chunks), (2, done.new_chunks));
        assert_reconstructs(&store, &samples);

        // 有一个截断的 pack：整体失败，旧索引原样保留
        let bytes = fs::read(packs_dir.join(&pack_name)).unwrap();
        fs::write(packs_dir.join("ffffffffffffffff.pack"), &bytes[..bytes.len() - 1]).unwrap();
        assert!(store.reindex().is_err());
        assert_eq!(store.stats().unwrap().packs, 2);
        assert_reconstructs(&store, &samples);
    }

    /// n 行的合成文件
    fn lines_file(n: usize) -> Vec<u8> {
        let mut v = Vec::new();
        for i in 0..n {
            writeln!(v, "{{\"n\":{i}}}").unwrap();
        }
        v
    }

    #[test]
    fn two_level_tree_increment_is_small() {
        let t = TempDir::new("store-increment");
        let mut store = Store::open(t.0.clone()).unwrap();
        let src = "/h/.claude/projects/p/big.jsonl";
        let first = capture_all(&mut store, &[(src, big())]);
        // 12 万行 = 118 块（117 满 + 1 个 192 行的尾块）+ 顶层
        assert_eq!(first.new_chunks, 120_000 + 118 + 1);

        let mut grown = big();
        grown.extend_from_slice(b"{\"z\":1}\n");
        let done = capture_all(&mut store, &[(src, grown.clone())]);
        // 增量 = 新行 + 重写的尾块（193 × 32 B）+ 顶层（118 × 32 B），远小于平铺时的 3.7 MiB
        assert_eq!(done.new_chunks, 3);
        assert!(done.stored_bytes < 64 << 10, "stored {} bytes", done.stored_bytes);
        assert_reconstructs(&store, &[(src, grown)]);
    }

    /// 块边界：0、1、恰好一块、一块多一行都能往返；从恰好一块长到一块多一行时满块直接去重
    #[test]
    fn block_boundaries_roundtrip() {
        let t = TempDir::new("store-blocks");
        let mut store = Store::open(t.0.clone()).unwrap();
        let src = |n: usize| format!("/h/.claude/projects/p/{n}.jsonl");
        let samples: Vec<(String, Vec<u8>)> =
            [0, 1, BLOCK_LINES, BLOCK_LINES + 1].into_iter().map(|n| (src(n), lines_file(n))).collect();
        let borrowed: Vec<(&str, Vec<u8>)> = samples.iter().map(|(s, b)| (s.as_str(), b.clone())).collect();
        capture_all(&mut store, &borrowed);
        assert_reconstructs(&store, &borrowed);

        let latest = store.latest().unwrap();
        let mut reader = store.reader();
        for ((source, want), n) in borrowed.iter().zip([0, 1, BLOCK_LINES, BLOCK_LINES + 1]) {
            let r = rec(&latest, &store, source);
            assert_eq!(r.v, MANIFEST_VERSION);
            assert_eq!(r.lines as usize, n);
            // 顶层块数
            assert_eq!(reader.hashes(&unhex(&r.tree).unwrap()).unwrap().len(), n.div_ceil(BLOCK_LINES));
            let lines = reader.lines(r).unwrap();
            assert_eq!(lines.len(), n);
            assert_eq!(lines.iter().flat_map(|(_, l)| l.iter().copied()).collect::<Vec<u8>>(), *want);
            assert!(lines.iter().all(|(h, l)| *h == hash(l)));
        }
        drop(reader);

        // 恰好一块的文件长一行（内容要和 1025.jsonl 不同，否则整个被跨文件去重）：满块已在，只多新行 + 新块 + 新顶层
        let mut grown = lines_file(BLOCK_LINES);
        grown.extend_from_slice(b"{\"z\":1}\n");
        let done = capture_all(&mut store, &[(&src(BLOCK_LINES), grown.clone())]);
        assert_eq!(done.new_chunks, 3);
        assert_eq!(done.stored_bytes, 8 + 32 + 64);
        assert_reconstructs(&store, &[(&src(BLOCK_LINES), grown)]);
    }

    /// 手工按 v2 编码（平铺的行 hash 列表）写一条记录，走 Reader 的 v2 分支
    fn put_v2(store: &mut Store, source: &str, bytes: &[u8]) {
        let mut batch = store.batch().unwrap();
        let mut tree_bytes = Vec::new();
        for line in bytes.split_inclusive(|&c| c == b'\n') {
            let h = hash(line);
            tree_bytes.extend_from_slice(&h);
            batch.put(h, line).unwrap();
        }
        let tree = hash(&tree_bytes);
        batch.put(tree, &tree_bytes).unwrap();
        batch.records.push(Record {
            v: 2,
            provider: P.into(),
            machine: batch.store.machine.clone(),
            source: source.into(),
            tree: hex(&tree),
            blob: hex(&hash(bytes)),
            size: bytes.len() as u64,
            lines: bytes.split_inclusive(|&c| c == b'\n').count() as u32,
            mtime: Utc::now(),
            captured_at: Utc::now(),
        });
        batch.commit().unwrap();
    }

    #[test]
    fn v2_flat_tree_still_readable() {
        let t = TempDir::new("store-v2");
        let mut store = Store::open(t.0.clone()).unwrap();
        let samples = samples();
        for (source, bytes) in &samples {
            put_v2(&mut store, source, bytes);
        }
        let latest = store.latest().unwrap();
        assert!(latest.values().all(|r| r.v == 2));
        assert_reconstructs(&store, &samples);
        let r = rec(&latest, &store, samples[1].0);
        let lines = store.reader().lines(r).unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1].1, b"{\"b\":2}");
        assert_eq!(lines[1].0, hash(b"{\"b\":2}"));

        // 新代码在同一个 store 里追加快照：新记录是 v3，旧 v2 记录仍然能读
        let mut grown = samples[3].1.clone();
        grown.extend_from_slice(b"{\"z\":1}\n");
        capture_all(&mut store, &[(samples[3].0, grown.clone())]);
        let records = store.records().unwrap();
        assert_eq!(records.len(), 5);
        assert_eq!(records[4].v, 3);
        let mut reader = store.reader();
        for (r, want) in records.iter().zip(samples.iter().map(|(_, b)| b).chain([&grown])) {
            assert_eq!(&reader.reconstruct(r).unwrap(), want, "{} v{}", r.source, r.v);
        }
    }

    #[test]
    fn source_key_folds_dots_and_slashes() {
        let base = std::path::absolute("x").unwrap();
        let base = base.parent().unwrap();
        let want = format!("{}/a/b.jsonl", base.to_string_lossy().replace('\\', "/"));
        assert_eq!(source_key(Path::new("a/b.jsonl")), want);
        assert_eq!(source_key(Path::new("./a/./b.jsonl")), want);
        assert_eq!(source_key(Path::new("a/c/../b.jsonl")), want);
        assert_eq!(source_key(&base.join("a").join("b.jsonl")), want);
    }
}
