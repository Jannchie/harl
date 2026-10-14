//! 存储：manifest（事实来源）+ packs（chunk 实体）+ index.db（可重建的位置索引）。
//!
//! ```text
//! machine-id
//! raw/machines/<id>.json   这台机器是谁：主机名、系统（id 是随机的，多机汇到一处时靠它认机器）
//! raw/manifest/<id>.jsonl  只追加：一个源文件每次内容变化写一条 {source, tree, blob, …}。
//!                          每台机器只写自己那份，同步（S3、网盘）时不会有两边同时改一个文件
//! raw/packs/*.pack         chunk（行）和 tree（行 hash 列表）都存在这里；不可变、内容命名
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
//!
//! 压缩的源文件（dsh 的 `.jsonl.zstd`）按解压后的内容存：tree / blob / size 都是解压后的，
//! `cat` / `check` 还原的也是解压后的 JSONL。磁盘上的文件大小另记在 `file_size`，只给 stat 快速跳过用。

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::ops::ControlFlow::{self, Break, Continue};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::hash::{Hash, hash, hex, unhex};
use crate::pack::{self, Finished, Loc};

const MANIFEST_VERSION: u32 = 3;
/// 一个 tree 块最多装这么多行 hash（32 KiB）。
/// 每次 capture 固定重存最后一块（平均半块 16 KiB）加顶层列表（每 1024 行 32 B），
/// 与一次增量的新行本身（通常几十 KiB）同量级；再小块数就多了，index.db 每块一行、顶层列表也随之变长。
const BLOCK_LINES: usize = 1024;

/// manifest 的一行
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
    /// 源文件在磁盘上的大小，和 `size` 不同时才有（压缩文件）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_size: Option<u64>,
    pub lines: u32,
    pub mtime: DateTime<Utc>,
    pub captured_at: DateTime<Utc>,
}

/// `raw/machines/<id>.json`：machine-id 是随机串，这里记下它对应哪台机器。
/// 放在 raw/ 下，和 manifest、packs 一起同步，汇总处拿 id 就能查到主机名
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MachineInfo {
    pub id: String,
    pub hostname: String,
    /// `std::env::consts::OS`：windows / linux / macos …
    pub os: String,
    /// 最后一次写这个文件的时间：主机名或系统变了才重写
    pub updated_at: DateTime<Utc>,
}

/// 每个 (machine, source) 的最新记录
pub type Latest = BTreeMap<(String, String), Record>;

/// 早期（v2）在 Windows 上记的 source 是 `C:\…` 写法，现在的 source_key 统一成 `/`。
/// 读的时候折成一样，否则同一个文件新旧两种写法会被当成两个源、会话重复计算。
/// 只动带盘符的路径：Linux 的文件名里 `\` 是合法字符
fn legacy_source(s: String) -> String {
    let b = s.as_bytes();
    if b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        crate::normalize::slashed(&s)
    } else {
        s
    }
}

/// 按 captured_at 取最新，而不是按行序：manifest 是集合，拼接、合并别的机器的记录都不改变结果
pub fn latest_of(records: Vec<Record>) -> Latest {
    let mut m = Latest::new();
    for r in records {
        let k = (r.machine.clone(), r.source.clone());
        if m.get(&k).is_none_or(|p| p.captured_at <= r.captured_at) {
            m.insert(k, r);
        }
    }
    m
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
        register_machine(&root, &machine)?;
        migrate_legacy_manifest(&root)?;
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
             );
             DROP TABLE IF EXISTS session_stats;
             CREATE TABLE IF NOT EXISTS derived(
               kind TEXT NOT NULL, tree TEXT NOT NULL, v INTEGER NOT NULL, json TEXT NOT NULL,
               PRIMARY KEY(kind, tree)
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

    /// 存储里出现过的所有机器（raw/machines/ 下的描述）。读不了的文件跳过
    pub fn machines(&self) -> Result<BTreeMap<String, MachineInfo>> {
        let mut out = BTreeMap::new();
        let Ok(dir) = fs::read_dir(self.root.join(MACHINES_DIR)) else { return Ok(out) };
        for entry in dir {
            let path = entry?.path();
            if path.extension().is_none_or(|e| e != "json") {
                continue;
            }
            match read_machine(&path) {
                Ok(m) => {
                    out.insert(m.id.clone(), m);
                }
                Err(e) => eprintln!("warning: skipping {}: {e}", path.display()),
            }
        }
        Ok(out)
    }

    pub fn packs_dir(&self) -> PathBuf {
        self.root.join("raw/packs")
    }

    /// 某台机器的 manifest 文件
    pub fn manifest_path_of(&self, machine: &str) -> PathBuf {
        manifest_path(&self.root, machine)
    }

    /// 本机的 manifest：只有本机的 capture 写它
    pub fn manifest_path(&self) -> PathBuf {
        self.manifest_path_of(&self.machine)
    }

    /// 本机描述文件 `raw/machines/<id>.json`
    pub fn machine_path(&self) -> PathBuf {
        self.root.join(MACHINES_DIR).join(format!("{}.json", self.machine))
    }

    // ---- manifest ----

    /// 所有机器的记录：本机的加上同步来的。坏行和版本太新的行 warning 跳过（见 `read_manifest`）
    pub fn records(&self) -> Result<Vec<Record>> {
        let mut records = Vec::new();
        for path in manifest_files(&self.root)? {
            read_manifest(&path, &mut records)?;
        }
        Ok(records)
    }

    /// 只读本机的记录：capture 只关心本机每个源的上一版
    fn own_records(&self) -> Result<Vec<Record>> {
        let mut records = Vec::new();
        read_manifest(&self.manifest_path(), &mut records)?;
        Ok(records)
    }

    /// 某台机器的 manifest 在本地有多少字节（没有就是 0）；同步从这里接着拉
    pub fn manifest_len(&self, machine: &str) -> u64 {
        fs::metadata(self.manifest_path_of(machine)).map(|m| m.len()).unwrap_or(0)
    }

    /// 把同步拉下来的一段字节接到别的机器的 manifest 后面。`at` 是这段在对方 manifest 里的起始偏移，
    /// 必须正好等于本地现有长度：按字节偏移拼，拼出来和对方的文件逐字节一致（中途崩溃留下的半段下次从断处接上）。
    /// 本机那份只由 capture 写；id 要像个 id，免得远端的对象名拼出奇怪的路径
    pub fn append_manifest(&self, machine: &str, at: u64, bytes: &[u8]) -> Result<()> {
        self.check_foreign(machine)?;
        let path = self.manifest_path_of(machine);
        fs::create_dir_all(path.parent().context("manifest path has no parent directory")?)?;
        let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
        let len = file.metadata()?.len();
        if len != at {
            bail!(
                "the local manifest of {machine} has {len} bytes, but the pulled segment starts at {at}; they do not line up"
            );
        }
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(())
    }

    /// 解析一段 manifest 字节（同步拉下来的，还没落盘），坏行跳过
    pub fn parse_records(bytes: &[u8]) -> Vec<Record> {
        bytes
            .split(|&b| b == b'\n')
            .filter_map(|l| serde_json::from_slice::<Record>(l).ok())
            .filter(|r| r.v <= MANIFEST_VERSION)
            .collect()
    }

    /// 这条记录的 tree 根 chunk 在不在本地：同步时用来判断它引用的 pack 拉全了没有
    pub fn has_tree(&self, rec: &Record) -> bool {
        unhex(&rec.tree).ok().is_some_and(|h| self.loc(&h).ok().flatten().is_some())
    }

    /// 写入别的机器的描述，同上
    pub fn install_machine(&self, machine: &str, bytes: &[u8]) -> Result<()> {
        self.check_foreign(machine)?;
        serde_json::from_slice::<MachineInfo>(bytes).context("machine description is not valid JSON")?;
        write_atomic(&self.root.join(MACHINES_DIR).join(format!("{machine}.json")), bytes)
    }

    fn check_foreign(&self, machine: &str) -> Result<()> {
        if machine == self.machine {
            bail!("refusing to overwrite this machine's data with synced content");
        }
        if !is_machine_id(machine) {
            bail!("invalid machine id: {machine:?}");
        }
        Ok(())
    }

    /// packs 目录里的 pack 文件名，排好序
    pub fn pack_names(&self) -> Result<Vec<String>> {
        let mut names: Vec<String> = fs::read_dir(self.packs_dir())?
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| is_pack_name(n))
            .collect();
        names.sort();
        Ok(names)
    }

    /// 已经放进 packs 目录的 pack（同步拉下来的）登记进索引，返回新登记的 chunk 数
    pub fn add_pack(&mut self, name: &str) -> Result<usize> {
        let done = pack::scan(&self.packs_dir(), name)?;
        let tx = self.db.transaction()?;
        let n = register_pack(&tx, done)?;
        tx.commit()?;
        Ok(n)
    }

    pub fn latest(&self) -> Result<Latest> {
        Ok(latest_of(self.records()?))
    }

    /// 引用这个 tree 的任意一条记录（哪台机器的都行）；tree 的编码由记录版本决定，所以裸 hash 不够用
    pub fn record_by_tree(&self, tree: &str) -> Result<Option<Record>> {
        Ok(self.records()?.into_iter().find(|r| r.tree == tree))
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
        let names = self.pack_names()?;
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

    // ---- 派生数据缓存：按 tree 存，快照不可变所以永不失效；v 变了就当没有 ----

    /// 缓存过的派生数据（会话统计、会话头……），按 (kind, tree) 存；v 由调用方管，口径变了 +1 旧的就当没有
    pub fn cached<T: serde::de::DeserializeOwned>(&self, kind: &str, v: u32, tree: &str) -> Result<Option<T>> {
        let json: Option<String> = self
            .db
            .prepare_cached("SELECT json FROM derived WHERE kind=?1 AND tree=?2 AND v=?3")?
            .query_row(params![kind, tree, v], |r| r.get(0))
            .optional()?;
        Ok(json.map(|j| serde_json::from_str(&j)).transpose()?)
    }

    pub fn put_cached<T: Serialize>(&self, kind: &str, v: u32, tree: &str, value: &T) -> Result<()> {
        self.db
            .prepare_cached("INSERT OR REPLACE INTO derived(kind, tree, v, json) VALUES(?1,?2,?3,?4)")?
            .execute(params![kind, tree, v, serde_json::to_string(value)?])?;
        Ok(())
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
        let (pack, loc) = self.store.loc(hash)?.with_context(|| format!("chunk not in index: {}", hex(hash)))?;
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
            bail!("chunk out of bounds: {}", hex(hash));
        }
        out.extend_from_slice(&data[s..e]);
        Ok(())
    }

    /// 读一个"hash 列表"chunk
    fn hashes(&mut self, h: &Hash) -> Result<Vec<Hash>> {
        let mut bytes = Vec::new();
        self.chunk_into(h, &mut bytes)?;
        if bytes.len() % 32 != 0 {
            bail!("hash list object length is not a multiple of 32: {}", hex(h));
        }
        Ok(bytes.chunks_exact(32).map(|c| c.try_into().unwrap()).collect())
    }

    /// 按顺序对快照的每个行 hash 调 f。编码由记录版本决定，所以要拿 Record 而不是裸 tree hash。
    /// v3 一块一块走：块 chunk 和它的行写在同一个 frame 里，紧挨着读能命中缓存，不用先把全部行 hash 物化出来。
    /// 回调返回 Break 就停：只想看开头几行时不用把所有块读一遍。
    fn walk(&mut self, rec: &Record, mut f: impl FnMut(&mut Self, Hash) -> Result<ControlFlow<()>>) -> Result<()> {
        let tree = unhex(&rec.tree).with_context(|| format!("tree of record {:?}", rec.tree))?;
        if rec.v <= 2 {
            for h in self.hashes(&tree)? {
                if f(self, h)?.is_break() {
                    break;
                }
            }
            return Ok(());
        }
        for b in self.hashes(&tree)? {
            for h in self.hashes(&b)? {
                if f(self, h)?.is_break() {
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    /// 逐字节还原一个快照
    pub fn reconstruct(&mut self, rec: &Record) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(rec.size as usize);
        self.walk(rec, |r, h| r.chunk_into(&h, &mut out).map(Continue))?;
        Ok(out)
    }

    /// 还原并与记录的 blob / size 比对
    pub fn verify(&mut self, rec: &Record) -> Result<Verified> {
        let bytes = self.reconstruct(rec)?;
        if hex(&hash(&bytes)) == rec.blob && bytes.len() as u64 == rec.size {
            Ok(Verified::Ok)
        } else {
            Ok(Verified::Mismatch { got_len: bytes.len() as u64 })
        }
    }

    /// 快照的每一行，连同它的 chunk hash（normalizer 用它做 prov）
    pub fn lines(&mut self, rec: &Record) -> Result<Vec<(Hash, Vec<u8>)>> {
        self.lines_prefix(rec, usize::MAX)
    }

    /// 开头至多 max 行
    pub fn lines_prefix(&mut self, rec: &Record, max: usize) -> Result<Vec<(Hash, Vec<u8>)>> {
        let mut lines = Vec::with_capacity((rec.lines as usize).min(max));
        if max == 0 {
            return Ok(lines);
        }
        self.walk(rec, |r, h| {
            let mut line = Vec::new();
            r.chunk_into(&h, &mut line)?;
            lines.push((h, line));
            Ok(if lines.len() >= max { Break(()) } else { Continue(()) })
        })?;
        Ok(lines)
    }
}

pub enum Verified {
    Ok,
    Mismatch { got_len: u64 },
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
    /// 这次新写的记录
    pub records: Vec<Record>,
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
        let latest = latest_of(store.own_records()?).into_iter().map(|((_, source), rec)| (source, rec)).collect();
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
        self.prev(source).is_some_and(|p| p.file_size.unwrap_or(p.size) == size && p.mtime == mtime)
    }

    /// 存一个源文件的快照。内容 hash 与上次一致就不存。`file_size` 是磁盘上的大小（压缩文件和 bytes 不同）。
    /// 读文件由调用方负责（见 `read_source`），所以这里出的错都是存储层的。
    pub fn put_snapshot(
        &mut self,
        provider: &str,
        source: &str,
        mtime: DateTime<Utc>,
        file_size: u64,
        bytes: &[u8],
    ) -> Result<Outcome> {
        let prev = self.prev(source);
        let blob = hex(&hash(bytes));
        let outcome = match prev {
            Some(p) if p.blob == blob => return Ok(Outcome::Unchanged),
            Some(_) => Outcome::Changed,
            None => Outcome::New,
        };

        let tree = tree_of(bytes);
        for (h, chunk) in tree.chunks() {
            self.put(h, chunk)?;
        }

        self.logical += bytes.len() as u64;
        self.records.push(Record {
            v: MANIFEST_VERSION,
            provider: provider.into(),
            machine: self.store.machine.clone(),
            source: source.into(),
            tree: hex(&tree.root.0),
            blob,
            size: bytes.len() as u64,
            file_size: (file_size != bytes.len() as u64).then_some(file_size),
            lines: tree.lines.len() as u32,
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
        append_records(&self.store.manifest_path(), &self.records)?;
        Ok(Committed {
            pack,
            stored_bytes: self.stored_bytes,
            new_chunks: self.new_chunks,
            logical: self.logical,
            records: self.records,
        })
    }
}

/// 一个文件按 v3 编码切出来的全部 chunk：行、块、顶层 tree。纯函数，格式契约本身，不碰存储。
pub struct Tree<'a> {
    pub lines: Vec<(Hash, &'a [u8])>,
    pub blocks: Vec<(Hash, Vec<u8>)>,
    pub root: (Hash, Vec<u8>),
}

impl Tree<'_> {
    /// 写入顺序：每块的行紧跟着块本身，最后是顶层——读的时候块和它的行才会落在同一个 frame
    pub fn chunks(&self) -> impl Iterator<Item = (Hash, &[u8])> {
        self.lines
            .chunks(BLOCK_LINES)
            .zip(&self.blocks)
            .flat_map(|(lines, (bh, bb))| lines.iter().map(|(h, l)| (*h, *l)).chain(std::iter::once((*bh, bb.as_slice()))))
            .chain(std::iter::once((self.root.0, self.root.1.as_slice())))
    }
}

/// 行（含结尾 `\n`）→ 每 BLOCK_LINES 个行 hash 拼成一块 → 块 hash 拼成顶层。
/// 满块的 hash 只取决于那些行，文件追加后再切一次，前面的块全部相同。
pub fn tree_of(bytes: &[u8]) -> Tree<'_> {
    let lines: Vec<(Hash, &[u8])> = bytes.split_inclusive(|&c| c == b'\n').map(|l| (hash(l), l)).collect();
    let blocks: Vec<(Hash, Vec<u8>)> = lines
        .chunks(BLOCK_LINES)
        .map(|block| {
            let bb: Vec<u8> = block.iter().flat_map(|(h, _)| *h).collect();
            (hash(&bb), bb)
        })
        .collect();
    let rb: Vec<u8> = blocks.iter().flat_map(|(h, _)| *h).collect();
    Tree { lines, blocks, root: (hash(&rb), rb) }
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

/// 会话文件：`.jsonl`，或 zstd 压缩的 `.jsonl.zstd` / `.jsonl.zst`
pub fn is_session_file(name: &str) -> bool {
    name.ends_with(".jsonl") || name.ends_with(".jsonl.zstd") || name.ends_with(".jsonl.zst")
}

/// 读一个会话文件的内容，压缩的先解压（多个 frame 顺序拼接）
pub fn read_source(path: &str) -> std::io::Result<Vec<u8>> {
    let raw = fs::read(path)?;
    if path.ends_with(".zstd") || path.ends_with(".zst") { zstd::decode_all(raw.as_slice()) } else { Ok(raw) }
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

// ---- manifest 文件 ----

const MANIFEST_DIR: &str = "raw/manifest";
/// 按机器分文件之前的单个 manifest；打开存储时拆进 MANIFEST_DIR
const LEGACY_MANIFEST: &str = "raw/manifest.jsonl";

fn manifest_path(root: &Path, machine: &str) -> PathBuf {
    root.join(MANIFEST_DIR).join(format!("{machine}.jsonl"))
}

/// MANIFEST_DIR 下每台机器的 manifest，按文件名排序
fn manifest_files(root: &Path) -> Result<Vec<PathBuf>> {
    let Ok(dir) = fs::read_dir(root.join(MANIFEST_DIR)) else { return Ok(Vec::new()) };
    let mut out: Vec<PathBuf> = dir
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.file_stem().and_then(|s| s.to_str()).is_some_and(is_machine_id) && p.extension().is_some_and(|x| x == "jsonl"))
        .collect();
    out.sort();
    Ok(out)
}

/// 逐行解析一份 manifest，追加到 out。文件不存在就是空。
/// 坏行（半行、非 JSON）和版本比本程序新的行都 warning 跳过，不让一行毁掉整个 manifest
fn read_manifest(path: &Path, out: &mut Vec<Record>) -> Result<()> {
    let Ok(file) = fs::File::open(path) else { return Ok(()) };
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    for (i, line) in BufReader::new(file).split(b'\n').enumerate() {
        let line = line.with_context(|| format!("read {name}"))?;
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        match serde_json::from_slice::<Record>(&line) {
            Ok(rec) if rec.v > MANIFEST_VERSION => {
                eprintln!(
                    "warning: {name} line {} is v{}, but this build only reads up to v{MANIFEST_VERSION}; skipped",
                    i + 1,
                    rec.v
                );
            }
            Ok(mut rec) => {
                rec.source = legacy_source(rec.source);
                out.push(rec);
            }
            Err(_) => eprintln!("warning: {name} line {} is corrupt; skipped", i + 1),
        }
    }
    Ok(())
}

/// 追加到一份 manifest
fn append_records(path: &Path, recs: &[Record]) -> Result<()> {
    if recs.is_empty() {
        return Ok(());
    }
    fs::create_dir_all(path.parent().context("manifest path has no parent directory")?)?;
    let mut file = OpenOptions::new().create(true).append(true).read(true).open(path)?;
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
    // 整批一次 write：进程被杀不会留半行（掉电仍可能，靠上面的补换行和 read_manifest 的容错兜底）
    let mut buf = Vec::new();
    for rec in recs {
        serde_json::to_writer(&mut buf, rec)?;
        buf.push(b'\n');
    }
    file.write_all(&buf)?;
    file.sync_all()?;
    Ok(())
}

/// 旧的单文件 manifest 按机器拆进 MANIFEST_DIR，然后改名留底。
/// 每台机器的目标文件 = 已有内容 + 旧文件里属于它的行（按行去重），整份重写：中途崩溃再跑一遍结果一样
fn migrate_legacy_manifest(root: &Path) -> Result<()> {
    let legacy = root.join(LEGACY_MANIFEST);
    if !legacy.is_file() {
        return Ok(());
    }
    let mut old = Vec::new();
    read_manifest(&legacy, &mut old)?;
    let mut by_machine: BTreeMap<String, Vec<Record>> = BTreeMap::new();
    for rec in old {
        by_machine.entry(rec.machine.clone()).or_default().push(rec);
    }
    for (machine, recs) in by_machine {
        if !is_machine_id(&machine) {
            eprintln!(
                "warning: invalid machine id {machine:?} in manifest; {} records not migrated (old file kept)",
                recs.len()
            );
            continue;
        }
        let path = manifest_path(root, &machine);
        let mut merged = Vec::new();
        read_manifest(&path, &mut merged)?;
        merged.extend(recs);
        let mut seen = HashSet::new();
        let mut buf = Vec::new();
        for rec in &merged {
            let line = serde_json::to_vec(rec)?;
            if seen.insert(line.clone()) {
                buf.extend_from_slice(&line);
                buf.push(b'\n');
            }
        }
        write_atomic(&path, &buf)?;
    }
    fs::rename(&legacy, root.join(format!("{LEGACY_MANIFEST}.migrated"))).context("rename old manifest")?;
    Ok(())
}

/// 先写临时文件再改名：并发的读者看不到半个文件
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().context("path has no parent directory")?;
    fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".{}.tmp", path.file_name().unwrap_or_default().to_string_lossy()));
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

/// 机器 id：`new_machine_id` 产生的是 32 位小写 hex；放宽到字母数字、`-`、`_`，同步时拿它拼路径
pub fn is_machine_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// pack 文件名：内容命名的 hex + `.pack`
pub fn is_pack_name(s: &str) -> bool {
    s.strip_suffix(".pack").is_some_and(|h| !h.is_empty() && h.len() <= 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
}

const MACHINES_DIR: &str = "raw/machines";

pub fn hostname() -> String {
    gethostname::gethostname().to_string_lossy().into_owned()
}

/// 写本机的描述。和已有的一样就不动（每次打开存储都会调到，不该每次都改文件）；
/// 主机名改了就覆盖，id 不变，旧快照仍归这台机器
fn read_machine(path: &Path) -> Result<MachineInfo> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn register_machine(root: &Path, id: &str) -> Result<()> {
    let dir = root.join(MACHINES_DIR);
    let path = dir.join(format!("{id}.json"));
    let (hostname, os) = (hostname(), std::env::consts::OS.to_string());
    if let Ok(old) = read_machine(&path)
        && old.hostname == hostname
        && old.os == os
    {
        return Ok(());
    }
    let info = MachineInfo { id: id.to_string(), hostname, os, updated_at: Utc::now() };
    write_atomic(&path, &serde_json::to_vec_pretty(&info)?)
}

fn new_machine_id() -> String {
    let seed = format!(
        "{}|{}|{}",
        hostname(),
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

    // 打开存储就登记本机主机名；再打开、主机名没变就不重写
    #[test]
    fn machine_registered_once() {
        let t = TempDir::new("store-machine");
        let store = Store::open(t.0.clone()).unwrap();
        let ms = store.machines().unwrap();
        let me = &ms[store.machine()];
        assert_eq!(me.hostname, hostname());
        assert_eq!(me.os, std::env::consts::OS);
        drop(store);
        let again = Store::open(t.0.clone()).unwrap();
        assert_eq!(again.machines().unwrap()[again.machine()], *me);
    }

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
            batch.put_snapshot(P, source, now, bytes.len() as u64, bytes).unwrap();
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
            assert!(matches!(batch.put_snapshot(P, source, now, bytes.len() as u64, bytes).unwrap(), Outcome::New));
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
        assert!(matches!(batch.put_snapshot(P, src1, now, bytes1.len() as u64, bytes1).unwrap(), Outcome::Unchanged));
        let mut grown = samples[3].1.clone();
        grown.extend_from_slice(b"{\"z\":1}\n");
        assert!(matches!(batch.put_snapshot(P, samples[3].0, now, grown.len() as u64, &grown).unwrap(), Outcome::Changed));
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
    /// 纯函数层面的块边界：不碰存储
    #[test]
    fn tree_of_block_boundaries() {
        for n in [0, 1, BLOCK_LINES, BLOCK_LINES + 1, 2 * BLOCK_LINES] {
            let bytes = lines_file(n);
            let t = tree_of(&bytes);
            assert_eq!(t.lines.len(), n);
            assert_eq!(t.blocks.len(), n.div_ceil(BLOCK_LINES));
            assert_eq!(t.root.1.len(), t.blocks.len() * 32);
            assert_eq!(t.chunks().count(), n + t.blocks.len() + 1);
            // 行拼回去就是原文
            assert_eq!(t.lines.iter().flat_map(|(_, l)| l.iter().copied()).collect::<Vec<u8>>(), bytes);
        }
        // 空文件：零块、顶层是空 chunk
        assert_eq!(tree_of(b"").root, (hash(b""), Vec::new()));
        // 无尾换行的半行单独一块
        assert_eq!(tree_of(b"a\nb").lines.len(), 2);
        // 追加一行：满块不变，只有尾块和顶层变
        let (a, b) = (lines_file(BLOCK_LINES + 1), lines_file(BLOCK_LINES + 2));
        let (ta, tb) = (tree_of(&a), tree_of(&b));
        assert_eq!(ta.blocks[0], tb.blocks[0]);
        assert_ne!(ta.blocks[1], tb.blocks[1]);
        assert_ne!(ta.root, tb.root);
    }

    #[test]
    fn latest_is_by_captured_at_not_line_order() {
        let t = TempDir::new("store-latest");
        let mut store = Store::open(t.0.clone()).unwrap();
        let src = "/h/.claude/projects/p/a.jsonl";
        capture_all(&mut store, &[(src, b"1\n".to_vec())]);
        capture_all(&mut store, &[(src, b"1\n2\n".to_vec())]);
        let mut recs = store.records().unwrap();
        assert_eq!(recs.len(), 2);
        recs.reverse();
        assert_eq!(latest_of(recs).values().next().unwrap().lines, 2);
    }

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
            file_size: None,
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

    /// 旧的单文件 manifest：打开时按机器拆开、旧文件改名；拆出来的记录照样能读、能接着 capture
    #[test]
    fn legacy_manifest_split_by_machine() {
        let t = TempDir::new("store-legacy");
        let store = Store::open(t.0.clone()).unwrap();
        let me = store.machine().to_string();
        drop(store);
        let mk = |machine: &str, source: &str| Record {
            v: MANIFEST_VERSION,
            provider: P.into(),
            machine: machine.into(),
            source: source.into(),
            tree: hex(&hash(b"")),
            blob: hex(&hash(b"")),
            size: 0,
            file_size: None,
            lines: 0,
            mtime: Utc::now(),
            captured_at: Utc::now(),
        };
        let other = "0123456789abcdef0123456789abcdef";
        let legacy = t.0.join(LEGACY_MANIFEST);
        append_records(&legacy, &[mk(&me, "/a.jsonl"), mk(other, "/b.jsonl"), mk(&me, "/c.jsonl")]).unwrap();

        let store = Store::open(t.0.clone()).unwrap();
        assert!(!legacy.exists());
        assert!(t.0.join(format!("{LEGACY_MANIFEST}.migrated")).exists());
        assert_eq!(store.records().unwrap().len(), 3);
        assert_eq!(store.own_records().unwrap().len(), 2);
        let mut theirs = Vec::new();
        read_manifest(&store.manifest_path_of(other), &mut theirs).unwrap();
        assert_eq!(theirs.len(), 1);

        // 再跑一次迁移（模拟改名前崩溃）：不重复
        fs::copy(t.0.join(format!("{LEGACY_MANIFEST}.migrated")), &legacy).unwrap();
        drop(store);
        let store = Store::open(t.0.clone()).unwrap();
        assert_eq!(store.records().unwrap().len(), 3);
    }

    /// 同步来的别的机器的数据：manifest 和 pack 放进来就能读；本机的 manifest 和不合法的 id 拒绝
    #[test]
    fn foreign_manifest_and_pack_install() {
        let (ta, tb) = (TempDir::new("store-sync-a"), TempDir::new("store-sync-b"));
        let mut a = Store::open(ta.0.clone()).unwrap();
        let samples = samples();
        let done = capture_all(&mut a, &samples);
        let pack = done.pack.unwrap().0;

        let mut b = Store::open(tb.0.clone()).unwrap();
        fs::copy(a.packs_dir().join(&pack), b.packs_dir().join(&pack)).unwrap();
        assert_eq!(b.pack_names().unwrap(), vec![pack.clone()]);
        assert_eq!(b.add_pack(&pack).unwrap(), done.new_chunks);
        // 分两段接上：和同步时一段一段拉一样
        let bytes = fs::read(a.manifest_path()).unwrap();
        let cut = bytes.len() / 2;
        let rec0 = &Store::parse_records(&bytes)[0];
        b.append_manifest(a.machine(), 0, &bytes[..cut]).unwrap();
        assert!(b.append_manifest(a.machine(), 0, &bytes[cut..]).is_err());
        b.append_manifest(a.machine(), cut as u64, &bytes[cut..]).unwrap();
        assert_eq!(b.manifest_len(a.machine()), bytes.len() as u64);
        assert!(b.has_tree(rec0));
        b.install_machine(a.machine(), &fs::read(a.machine_path()).unwrap()).unwrap();

        let latest = b.latest().unwrap();
        let mut reader = b.reader();
        for (source, want) in &samples {
            let r = &latest[&(a.machine().to_string(), source.to_string())];
            assert_eq!(&reader.reconstruct(r).unwrap(), want);
        }
        drop(reader);
        assert!(b.machines().unwrap().contains_key(a.machine()));
        // b 自己 capture 不受 a 的记录影响：同一路径在 b 上是新文件
        let mut batch = b.batch().unwrap();
        assert!(matches!(batch.put_snapshot(P, samples[1].0, Utc::now(), 1, &samples[1].1).unwrap(), Outcome::New));
        drop(batch);

        assert!(b.append_manifest(b.machine(), 0, b"").is_err());
        assert!(b.append_manifest("../evil", 0, b"").is_err());
        assert!(!is_pack_name("../x.pack") && !is_pack_name(".writing-1.tmp") && is_pack_name("00ff.pack"));
    }

    #[test]
    fn legacy_windows_sources_fold_to_slashes() {
        assert_eq!(legacy_source(r"C:\Users\a\x.jsonl".into()), "C:/Users/a/x.jsonl");
        assert_eq!(legacy_source("C:/Users/a/x.jsonl".into()), "C:/Users/a/x.jsonl");
        assert_eq!(legacy_source(r"/home/a/we\ird.jsonl".into()), r"/home/a/we\ird.jsonl");
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
