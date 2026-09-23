# 后续事项

架构评审（2026-09）里判断"现在不改、到时候再改"的项，附触发时机。现状为什么够用也写在每条里，免得提前动。

## 存储

- **manifest 按机器分文件** `raw/manifest/<machine>.jsonl`。触发：做多机同步之前。
  现在单文件多机共写，任何双向同步工具都会冲突；`records()` 兼容读旧的 `manifest.jsonl` 即可，几十行。
- **Record 镜像进 index.db，`latest()` / `record_by_tree()` 走 SQL**。触发：manifest 到十万行级、每个命令解析肉眼可感。
  manifest 仍是唯一事实来源，`reindex` 同时重建 chunks 和 records。
- **`Record.v` 的语义定为"读这一行需要的最低 reader 版本"**：加可选字段（`#[serde(default)]`）不 bump，改 tree 编码或加新行种类才 bump。触发：第一次改 Record schema。
  否则加个字段就让旧版本跳过所有新行，然后重复 capture。
- **删除 / GC**：tombstone 行（`kind` 字段）+ mark（遍历 Record 走 tree）+ sweep（写新 pack、删旧 pack、reindex）。触发：第一次真的要删。
  packs 不可变、内容命名，sweep 与现有 `Writer` / `register_pack` 完全兼容。
- **并发 capture 对 `raw/` 拿目录锁**。触发：有空就做，定时任务和手动跑撞车早晚碰到。
  现在结果正确（各自 tmp、`INSERT OR IGNORE`），只是同一批 chunk 可能进两个 pack，且 `append_records` 的补换行不是原子的。
- **`known_chunks` 全量载入 → 逐 hash 点查 + 本批小集合**。触发：chunk 千万级。
- **`Reader` 读时校验 chunk hash**。触发：有空。现在 pack 内 bit rot 只有 `check` 能发现；zstd 解压大概率先报错，优先级低。
- **`ReadError { Missing, Corrupt, Io }`**。触发：加 fsck / GC。`check` 需要区分"跑 reindex 能修"和"数据丢了"。
- **`records()` 不 eprintln，返回 skipped 列表由 main 打印**。触发：lib 化或投影。

## canonical

- **`LineNormalizer::line` 改 sink 风格支持一行多事件**，事件身份变成 `(seq, sub)`。触发：第一个非"一行一事件"的 harness（Gemini CLI 整文件 JSON；Cursor 在 SQLite 里根本不是文件）。
  同时解决 `Parts::take` 每行只认第一个 tool block 的脆弱点。
- **`Provider.discover`** 取代 `.jsonl` 扩展名过滤 + `owns` 子串匹配。触发：同上。
- **`Usage` 的 serde alias 改为各 provider 自建**。触发：两个 harness 用同一个键名表示不同语义。
- **投影**：`Committed` 加 `records: Vec<Record>` 告诉投影哪些 source 变了；投影主键用 `(machine, source, seq)`，**不能用 `prov.chunk`**（chunk 跨文件去重，同一 hash 可能来自多个源文件）。`Event` 的 `&'static str` 字段要改成 `String` 才能反序列化。触发：做投影时。
- **`(provider, session)` 唯一性依赖各 harness 用 UUID 命名文件**；pfh 开发版分散在各 workspace，可能撞。投影时以 `(machine, source, seq)` 为主键即可绕开。

## 统计

- **capture 时顺手算 stats 写缓存**，而不是等第一次打开仪表盘（445 个会话首次 4.5 s）。触发：会话数上千。
- **`/api/summary` 增量聚合**：现在每次请求都把全部缓存的 SessionStats 读出来重新汇总（50 ms）；到万级会话时按 (machine, source) 的 latest 变化做增量。
