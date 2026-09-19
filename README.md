# harl

Append-only, content-addressed archive of AI coding-agent sessions.

raw 层：把会话 JSONL 无损快照进本地存储。canonical 层：把各 harness 归一成同一形状的事件流。
目前支持 Claude Code 和 preferred-harness。

```
harl capture [--root DIR]...   扫描 .jsonl，内容变化的文件存新快照（只存没见过的行）
harl ls                        每个源文件的最新快照
harl status                    存储总览与压缩比
harl check [--all]             逐字节还原每个快照并与记录的 blake3 比对
harl cat <tree|source-path>    还原一个快照到 stdout
harl reindex                   从 packs 重建 index.db
harl events <path-substring>   把一个会话归一化成 canonical 事件流（NDJSON）
```

## canonical 事件

形状取自 preferred-harness 的协议日志：session → turn → step → message / tool_call / tool_result，
外加 request_header（模型这一步实际收到的上下文）和 meta。一行 raw 对一个事件，`prov.chunk` 指回那一行。

- 显式给的边界直接用（pfh 全部；Claude Code 的 `promptId`、`turn_duration`、`requestId`），推断的在 `inferred` 里声明。
- 跨事件关系用 `links` 连边，normalizer 不合并不丢弃：`spawned`（Agent → 子会话）、`notifies`（后台任务通知 → 原调用）、`delivered_from`（SendMessage → 子会话收到的消息）。
- 没映射的字段原样留在 `ext`。

## 存储

数据目录 `$HARL_DATA_DIR` 或 `~/.harl`：

```
machine-id             本机标识
raw/manifest.jsonl     事实来源。只追加，每行 {v:3, provider, machine, source, tree, blob, size, lines, mtime, captured_at}
raw/packs/*.pack       chunk 实体。自描述、不可变、按内容命名
index.db               chunk hash → pack 位置。可由 packs 重建
```

- source = 源文件的绝对路径，统一 `/` 分隔（Windows 也是），所以从不同 root 或相对路径扫到同一个文件只算一个。
- chunk = 一行（含 `\n`；末尾没换行的半行单独一块），blake3 寻址。JSONL 是追加写的，所以一次 capture 只存新增的行；fork/resume 复制的历史、未变的文件不重复存。
- tree = 一个文件快照的行 hash 列表，分两级：每 1024 个行 hash 拼成一块（32 KiB，也是 chunk），tree 是块 hash 的列表。文件只追加，所以再次快照时除最后一块外全部去重，增量只有新行 + 最后一块 + 顶层列表。`concat(tree 展开)` 逐字节等于原文件，`harl check` 就是验这个。记录的 `v` 决定 tree 编码：v2 是旧的平铺列表，仍能读；新记录都是 v3。
- pack = 若干 zstd 帧，每帧 ≤ 4 MiB 未压缩数据，帧头列出所含 chunk 的 hash 与长度。

实测（404 个文件，521.6 MiB）：首次 capture 3.5 s → 151 MiB（29%）；之后每次增量只存新行，一个活跃会话的变化通常是几十 KiB；`check` 2.3 s。大文件的固定开销也小：12 万行（8.6 MiB）的会话追加一行再 capture，只存 3 个 chunk、9.8 KiB（平铺 tree 时是 3.7 MiB）。升级后旧 v2 记录的文件第一次再变化时会一次性存全部块（32 B × 行数），之后才是这个量级。
