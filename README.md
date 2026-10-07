# harl

Append-only, content-addressed archive of AI coding-agent sessions.

raw 层：把会话 JSONL 无损快照进本地存储。canonical 层：把各 harness 归一成同一形状的事件流。
目前支持 Claude Code、Codex、preferred-harness（pfh）和 dsh。

## 安装

```sh
# Windows（PowerShell）
irm https://github.com/Jannchie/harl/releases/latest/download/install.ps1 | iex
# macOS / Linux
curl -fsSL https://github.com/Jannchie/harl/releases/latest/download/install.sh | sh
```

装到 `~/.harl/bin`，校验 sha256。之后：

```
harl service install           开机自启 harl serve（看板 http://127.0.0.1:7461），挂了自动拉起
harl service install --agent   开机自启 harl agent：只采集、不起 UI，装在要采集的远程机器上
harl service status | stop | start | uninstall
harl update [--check]          升级到最新版本；看板侧栏有新版本时也会提示，点一下即可更新并重启
```

开机自启都是用户级的：Windows 是 HKCU Run 启动无窗口的 `harlw.exe`（它负责拉起和重启 harl，日志在 `~/.harl/logs/harl.log`），
macOS 是 launchd agent，Linux 是 systemd 用户服务（`journalctl --user -u harl`；服务器上没登录也要跑的话执行一次 `sudo loginctl enable-linger $USER`）。

## 多机：同步到对象存储

不需要服务器。每台机器把自己的数据推到一个 S3 兼容的存储（S3、Cloudflare R2、MinIO、B2……）或一个共享目录，
看板所在的机器把别的机器的数据拉下来，会话、统计就按机器合在一起。`~/.harl/config.toml`：

```toml
[remote]
url = "s3://my-bucket/harl"        # 或者目录：url = "D:/nas/harl"
pull = true                        # 只采集的机器设 false
interval = 60                      # 秒；有新快照会提前推

[remote.options]                   # 交给 object_store；没写的 aws_* 从同名大写环境变量读
aws_endpoint = "https://<account>.r2.cloudflarestorage.com"
aws_region = "auto"
aws_access_key_id = "…"
aws_secret_access_key = "…"
```

`harl serve` / `harl agent` 配了远端就在后台定时同步；`harl sync [--push-only|--pull-only]` 手动同步一次。
同步全程增量：

```
packs/<上传毫秒>-<hash>.pack        不可变、内容命名，只传对方没有的；键名带时间，拉的时候只列最近上传的
manifest/<机器>/seg/<偏移>.jsonl     每次推只传本机 manifest 新增的字节
manifest/<机器>/base.jsonl           段攒到 64 个合并成一份、删掉旧段（需要删除权限；没有也能用，只是段不收）
machines/<机器>.json
```

每台机器只写自己那份 manifest，多台同时推不冲突；拉的时候从本地已有的长度接着取字节，拼出来和对方的逐字节一致。
推的时候先 pack 后 manifest，拉的时候先取 manifest 再拉 pack 最后落盘，读到的记录引用的数据一定都在。
没有新数据时一轮只有几个列表请求（每台对端机器一个）；进程启动后第一轮、以及每 6 小时，会完整列一次 pack 兜底。

```
harl capture [--root DIR]...   扫描 .jsonl，内容变化的文件存新快照（只存没见过的行）
harl ls                        每个源文件的最新快照
harl status                    存储总览与压缩比
harl check [--all]             逐字节还原每个快照并与记录的 blake3 比对
harl cat <tree|source-path>    还原一个快照到 stdout
harl reindex                   从 packs 重建 index.db
harl events <path-substring>   把一个会话归一化成 canonical 事件流（NDJSON）
harl serve [--port 7461] [--open] [--no-watch] [--root DIR]...
                              本地 HTTP：web UI + JSON API；后台监听会话目录，有变化自动增量 capture
harl agent [--root DIR]...     同上但不起 HTTP：只监听、capture、同步
harl sync                      和配置的远端同步一次
harl service …                 开机自启（见上）
harl update [--check]          自更新
```

## 仓库结构

```
crates/harl-core   库：hash / pack / store（raw 层）、event / normalize（canonical 层）
crates/harl        CLI 二进制，含 serve（axum；web 的构建产物用 rust-embed 嵌进去）
web                Vue 3 + Vite + TS + UnoCSS 的 UI（设计语言同 pfh）：按项目分组的会话侧栏、对话 / 事件两种视图、用量仪表盘
docs/backlog.md    架构评审里判断"到时候再改"的项，附触发时机
```

```
pnpm install && pnpm build     # 构建 web/dist
cargo build --release          # 嵌入 dist；没构建过 web 时 build.rs 放一个占位页，cargo 不依赖 pnpm
pnpm dev                       # 前端开发：Vite（127.0.0.1:7462）代理 /api 到 127.0.0.1:7461，另开一个 harl serve
```

API（都在 `/api` 下，JSON）：`version`（当前与最新版本）、`POST update`（下载新版本并重启，只接受同源请求）、`status`、`sessions`（每个源文件的最新快照，id 是 tree hash；带标题、父会话、工作目录）、
`sessions/{id}/events`（canonical 事件数组）、`sessions/{id}/raw`（原始 JSONL）、
`summary?tz=<分钟>&from=&to=`（仪表盘：tokens / 费用 / 时间 / 工具，按天、按周×小时、按模型、按会话聚合；from/to 是本地日期、两端含，不给就是全部，过滤在 turn 级做；`provider=`、`model=` 逗号分隔多选，provider 按会话过滤，model 按 turn 过滤、只计所选模型的 tokens，工具与墙钟时间按整轮计；`project=` 可重复，值是项目键——cwd 小写、`/` 分隔、去掉结尾斜杠，按会话过滤）。返回里的 `sessions` 是范围内每个会话的聚合，web 用它做会话排行，并按 cwd 归成项目。

## 统计口径

`harl-core::stats` 从事件流算，每个快照的结果按 tree 缓存在 index.db（快照不可变，缓存永不失效；口径变了 `STATS_VERSION` +1 作废）。

- usage 每步取最后一份（Claude Code 同一 requestId 的每行都带快照，pfh 在 step_completed 上，dsh 在 assistant/message 上）。
  Codex 不写调用边界，一步从 usage 上报推断（`token_usage_record`，老版本是去重后的 `token_count`）；它的 input 含缓存，已换成不含缓存的口径。
- 后端不算钱，只出 (model, tokens, 时间)；费用由 web 端用 [llm-pricing](https://github.com/Jannchie/llm-pricing) 按各家一方 API 价折成**等价费用**（`web/src/lib/pricing.ts`）：目录来自 models.dev，联网拉不到用库内置快照；按天窗口算，峰谷价 / 历史价能对上。目录里没有的模型记为未定价，费用 0。
- 模型时间 = 一步最后一条 assistant 事件 − 这步开始前最后一个事件（含思考与流式输出）；工具时间 = tool_result − tool_call；
  turn 墙钟 = 显式时长（Claude Code `durationMs`）或 turn 内首尾之差。都是从时间戳推的近似。

## canonical 事件

形状取自 preferred-harness 的协议日志：session → turn → step → message / tool_call / tool_result，
外加 request_header（模型这一步实际收到的上下文）和 meta。一行 raw 对一个事件，`prov.chunk` 指回那一行。

- 显式给的边界直接用（pfh、dsh 全部；Claude Code 的 `promptId`、`turn_duration`、`requestId`；Codex 的 turn），推断的在 `inferred` 里声明。
- 跨事件关系用 `links` 连边，normalizer 不合并不丢弃：`spawned`（Agent → 子会话）、`notifies`（后台任务通知 → 原调用）、`delivered_from`（SendMessage → 子会话收到的消息）。
- 没映射的字段原样留在 `ext`。

## 存储

数据目录 `$HARL_DATA_DIR` 或 `~/.harl`：

```
machine-id             本机标识（第一次打开存储时随机生成）
raw/machines/<id>.json 这个 id 是哪台机器：{id, hostname, os, updated_at}。主机名变了才重写
raw/manifest/<id>.jsonl 事实来源。只追加，每行 {v:3, provider, machine, source, tree, blob, size, lines, mtime, captured_at}；
                       每台机器只写自己的那份（旧的单文件 raw/manifest.jsonl 打开时自动拆开，原文件改名为 .migrated）
raw/packs/*.pack       chunk 实体。自描述、不可变、按内容命名
index.db               chunk hash → pack 位置。可由 packs 重建
```

- dsh 的会话文件是 zstd 压缩的（`~/.dsh/sessions/<cwd>/<session>/session[.v3].jsonl.zstd`），按**解压后**的 JSONL 存：`size`/`blob`/`cat`/`check` 都是解压后的内容，磁盘上的大小另记在 `file_size`，只给 stat 跳过用。老会话迁到 v3 后旧的 `session.jsonl.zstd` 还在，raw 层照存，会话列表和统计只算 v3。
- 每条记录都带 `machine`，最新快照按 `(machine, source)` 取，所以多台机器的数据可以汇到一个存储里而不互相覆盖。machine-id 是随机串，靠 `raw/machines/` 认出是哪台机器；它和 manifest、packs 一样在 `raw/` 下，同步时一起走。
- source = 源文件的绝对路径，统一 `/` 分隔（Windows 也是），所以从不同 root 或相对路径扫到同一个文件只算一个。
- chunk = 一行（含 `\n`；末尾没换行的半行单独一块），blake3 寻址。JSONL 是追加写的，所以一次 capture 只存新增的行；fork/resume 复制的历史、未变的文件不重复存。
- tree = 一个文件快照的行 hash 列表，分两级：每 1024 个行 hash 拼成一块（32 KiB，也是 chunk），tree 是块 hash 的列表。文件只追加，所以再次快照时除最后一块外全部去重，增量只有新行 + 最后一块 + 顶层列表。`concat(tree 展开)` 逐字节等于原文件，`harl check` 就是验这个。记录的 `v` 决定 tree 编码：v2 是旧的平铺列表，仍能读；新记录都是 v3。
- pack = 若干 zstd 帧，每帧 ≤ 4 MiB 未压缩数据，帧头列出所含 chunk 的 hash 与长度。

实测（404 个文件，521.6 MiB）：首次 capture 3.5 s → 151 MiB（29%）；之后每次增量只存新行，一个活跃会话的变化通常是几十 KiB；`check` 2.3 s。大文件的固定开销也小：12 万行（8.6 MiB）的会话追加一行再 capture，只存 3 个 chunk、9.8 KiB（平铺 tree 时是 3.7 MiB）。升级后旧 v2 记录的文件第一次再变化时会一次性存全部块（32 B × 行数），之后才是这个量级。
