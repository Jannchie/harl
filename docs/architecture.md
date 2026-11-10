# harl 架构

README 之外的实现细节：同步协议、仓库结构、API、统计口径、canonical 事件和存储格式。

## 多机同步协议

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

API（都在 `/api` 下，JSON）：`version`（当前与最新版本）、`POST update`（下载新版本并重启，只接受同源请求）、`status`、`sessions`（每个源文件的最新快照，id 是 tree hash；带标题、父会话、工作目录和 agent 身份，见下面「会话列表」）、
`sessions/{id}/events`（canonical 事件数组；`?slim=1` 把图片、文件的 base64 换成占位 `harl:elided:<seq>:<序号>:<原长度>`，上下文估算不读 base64，折出来的数字不变）、`sessions/{id}/blobs/{seq}/{序号}`（占位的原文）、`sessions/{id}/raw`（原始 JSONL）、
`summary?tz=<分钟>&from=&to=`（仪表盘：tokens / 费用 / 时间 / 工具，按天、按周×小时、按模型、按会话聚合；from/to 是本地日期、两端含，不给就是全部，过滤在 turn 级做；`provider=`、`model=` 逗号分隔多选，provider 按会话过滤，model 按 turn 过滤、只计所选模型的 tokens，工具与墙钟时间按整轮计；`project=` 可重复，值是项目键——cwd 小写、`/` 分隔、去掉结尾斜杠，按会话过滤）。返回里的 `sessions` 是范围内每个会话的聚合，web 用它做会话排行，并按 cwd 归成项目。
洞察页用的几张表（都只从范围内的 turn 推导，过滤口径同上）：`by_tool` 每项带 `last_ts`、`sessions`；`skills`（skill 加载：`n`、`last_ts`、`sessions`、`path`、`source`）；
`programs`（shell 命令用到的程序：`n`、`failures`、`last_ts`、`sessions`）；`composition`（全部 prompt 按上下文组成拆成 system / tools / user / inject / skill / assistant / tool / other）。
`daily[]` 另带 `tools` / `skills` / `programs`（名字 → 次数）和 `composition`；`sessions[]` 另带同样的三张计数表、范围内的 `composition`，
以及不受范围影响的 `last_user`（最后一条人打的消息，单行、≤ 200 字符）、`ctx` / `ctx_parts` / `ctx_estimated`（会话末尾的上下文 tokens 及组成）。

### 会话列表

`/api/sessions` 的每一行来自会话头（`crates/harl/src/head.rs`：只读快照开头 40 行，按 tree 缓存，口径版本 `HEAD_VERSION`）。
内容里读出来的进缓存；只能从路径看出来的（路径上的父会话、pfh 的 workspace、workflow 的 run 目录）每次现补——同样的内容可能出现在不同路径下。

- `title`：第一句像样的用户提示的第一行（≤ 120 字符）。剥掉各家的包装：workflow harness 的说明段（`[Workflow harness — …]`，取正文第一行；
  转述的用户请求只在没有算出来的任务时才用）、Codex IDE 插件的 `# Context from my IDE setup:`（取 `## My request for Codex:` 下面的话）、
  开头整块的 `<system-reminder>` 等、`<conversation_history>` 里的第一句 `<user>`、`<pasted_content>` 的第一行。
  斜杠命令（`/goal 继续…`）和 `<user_action>`（`/review`）只在找不到真正的提示时当标题，`/clear` 这类不带参数的跳过。
- `parent_session`：路径上看得出的优先（Claude Code 的 `subagents/`、pfh 的 `.sub-call_`）；其次是内容：Claude Code 子 agent 每行都带的
  `sessionId`（`isSidechain: true` 且不等于自己）、fork agent 首行 `fork-context-ref` 的 `parentSessionId`，Codex 的 `parent_thread_id`，dsh 文件头的 `parentSession`。
  Claude Desktop 的 ssh 镜像（`projects/ssh-<session>/`）把子 agent 平铺在一个目录里，只能靠内容；镜像里的 `workflow-agent-<id>.jsonl` 会话 id 记成 `agent-<id>`，和原文件对得上。
- `kind`：`main`、`subagent`、`fork`、`workflow`（workflow 脚本起的 agent）、`journal`（`subagents/workflows/wf_<run>/journal.jsonl`，workflow 的运行日志，不是对话），
  以及 provider 自己的类别：Codex 的 `source.subagent` 是字符串时原样给（`review`），dsh 是文件头的 `origin`。写不出来的按有没有父会话补成 `main` / `subagent`。
- `label`：短名字。Codex 是 `thread_spawn` 的 agent 路径末段或昵称（带角色时 `昵称 · 角色`）；workflow agent 取同一 run 目录 journal 里 `started` 行的 `label`（`fix:creator`）。
  Claude Code Agent 工具起的子 agent 的 `description` 只记在父会话的 tool_result 里，要读整个父会话才拿得到，列表不读（Fleet 卡片读父会话事件时会用上）。
- `run` / `run_title`：同一次编排的分组键和组名，只在同一个根会话下面有意义。workflow agent 转述的用户请求（同一次运行里每个 agent 都一样）的哈希是 `req:<12 位>`，
  第一行是 `run_title`；没有转述的用 run 目录名 `wf_<id>`，平铺的镜像副本再退到发起它的 prompt（`prompt:<promptId>`）。

web 侧栏只列根会话（父链拉平，子 agent 不论几层都收进根会话），同一 `session` 的几份副本只留 mtime 最新的那份，journal 不列；
展开后普通子 agent 直接列出，每次 workflow 运行一组。显示名统一走 `web/src/lib/agents.ts` 的 `sessionTitle`：`label` 优先，其次 `title`。

## 统计口径

`harl-core::stats` 从事件流算，每个快照的结果按 tree 缓存在 index.db（快照不可变，缓存永不失效；口径变了 `STATS_VERSION` +1 作废，见下面「存储」的派生缓存）。
活跃会话每次 capture 换一个 tree：serve 进程里留着最近改过的几个源文件读完时的 normalizer + 统计状态（`harl-core::resume`），新快照只是追加（旧 tree 的块是新 tree 的前缀）时只读新行接着算，结果和从头算逐字节一致；不是追加（改了旧行、半行写完）就从头算。没缓存的快照并行算。

- usage 每步取最后一份（Claude Code 同一 requestId 的每行都带快照，pfh 在 step_completed 上，dsh 在 assistant/message 上）。
  Codex 不写调用边界，一步从 usage 上报推断（`token_usage_record`，老版本是去重后的 `token_count`）；它的 input 含缓存，已换成不含缓存的口径。
- 后端不算钱，只出 (model, tokens, 时间)；费用由 web 端用 [llm-pricing](https://github.com/Jannchie/llm-pricing) 按各家一方 API 价折成**等价费用**（`web/src/lib/pricing.ts`）：目录来自 models.dev，联网拉不到用库内置快照；按天窗口算，峰谷价 / 历史价能对上。目录里没有的模型记为未定价，费用 0。
- 模型时间 = 一步最后一条 assistant 事件 − 这步开始前最后一个事件（含思考与流式输出）；工具时间 = tool_result − tool_call；
  turn 墙钟 = 显式时长（Claude Code `durationMs`）或 turn 内首尾之差。都是从时间戳推的近似。
- shell 程序（`harl-core::programs`，算法移植自 dsh-context）：按 shell 语法切段（引号、heredoc、`$()` / 括号深度、注释；PowerShell 另认反引号、here-string、`<# #>`），
  每段剥掉重定向、`cd X &&`、`VAR=val` / export、包装命令（env、sudo、time、nohup、nice、timeout……）后取第一个词，去路径和 .exe；关键字、函数定义、子 shell 记作 `shell` / `pwsh`。
  一次调用里的程序去重后各计一次，调用失败记到每个程序的 `failures`。Codex 的 `shell_command` / `exec_command` 工作目录是盘符路径时按 PowerShell 解析。
- skill 加载（`harl-core::composition::SkillLoad`）：skill 工具的结果（名字取 `<skill_content name=…>`，没有就取参数的 `name` / `skill`）、dsh 的 `skill-invocation` 注入、
  Codex 的 `<skill>` 注入、Claude Code 的「Base directory for this skill:」（紧跟 Skill 工具结果时只补路径，否则是斜杠命令直接加载）。
  `source` 在汇总时按路径推断：`bundled:` 前缀、`/plugins/` 下、会话 cwd 下（project），其余算 user。
- 上下文组成（`harl-core::composition`）：重放事件流，口径同下面的上下文视图（七类、同样的字符估算和校准），每步在第一条模型事件之前拍快照，
  按这步的实际 prompt 校准后累加到 turn 的 `composition`；会话末尾另记当前上下文（最后一次请求的 prompt + 之后新增的估算）。
  和 web 的已知偏差：图片不估；快照点按 stats 的步边界（pfh 不带 assistant 角色的工具调用会晚一条）；都会在校准时被吸收（complete 时按比例摊开，否则落进 other）。
  模型过滤时 turn 的组成按所选模型的 prompt 占比等比缩小。

## 上下文视图

会话页的「上下文」视图（`web/src/lib/context.ts`、`components/ctx/`）移植自 dsh-context，但不依赖任何 harness 的运行时，只重放归档的 canonical 事件，四家 provider 通用：

- 上下文 = system prompt + 工具 schema + 消息面。消息面按事件顺序追加，压缩时移走（dsh 按 `shadowedSeqs` 逐条，其他整面换掉）；某次请求的快照拍在它第一条回复之前。
- 七类组成：system、工具 schema、用户消息、注入的上下文、skill、助手回复、工具结果。各家的来源：dsh 的 `request/header`、`system/message`、注入的 `user/message`；Claude Code 的 `prompt_snapshot` 与各种 attachment；Codex 的 developer 消息；pfh 的 `systemSections`。
- token 先按字符估（ASCII 4 字符 1 个、CJK 1.5 字符 1 个、每块 +4），再按 provider 报的 prompt（新输入 + 缓存读写）校准：system 和工具 schema 都在日志里时整体缩放，否则差额单列为「未记录」，不摊进别的类别。
- 压缩腾出的 token 用前后两次请求的实际 prompt 之差；费用按每次请求发生那一刻的价格折算。
- 文件活动从工具调用解析（Read / Edit / MultiEdit / Write / Grep / Glob、apply_patch、str_replace_editor、dsh `run_code` 里嵌套的调用），增删行按参数估；shell 命令不解析。

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
index.db               chunk hash → pack 位置（可由 packs 重建）；派生缓存（会话统计、会话头）
```

- dsh 的会话文件是 zstd 压缩的（`~/.dsh/sessions/<cwd>/<session>/session[.v3].jsonl.zstd`），按**解压后**的 JSONL 存：`size`/`blob`/`cat`/`check` 都是解压后的内容，磁盘上的大小另记在 `file_size`，只给 stat 跳过用。老会话迁到 v3 后旧的 `session.jsonl.zstd` 还在，raw 层照存，会话列表和统计只算 v3。
- 每条记录都带 `machine`，最新快照按 `(machine, source)` 取，所以多台机器的数据可以汇到一个存储里而不互相覆盖。machine-id 是随机串，靠 `raw/machines/` 认出是哪台机器；它和 manifest、packs 一样在 `raw/` 下，同步时一起走。
- source = 源文件的绝对路径，统一 `/` 分隔（Windows 也是），所以从不同 root 或相对路径扫到同一个文件只算一个。
- chunk = 一行（含 `\n`；末尾没换行的半行单独一块），blake3 寻址。JSONL 是追加写的，所以一次 capture 只存新增的行；fork/resume 复制的历史、未变的文件不重复存。
- tree = 一个文件快照的行 hash 列表，分两级：每 1024 个行 hash 拼成一块（32 KiB，也是 chunk），tree 是块 hash 的列表。文件只追加，所以再次快照时除最后一块外全部去重，增量只有新行 + 最后一块 + 顶层列表。`concat(tree 展开)` 逐字节等于原文件，`harl check` 就是验这个。记录的 `v` 决定 tree 编码：v2 是旧的平铺列表，仍能读；新记录都是 v3。
- pack = 若干 zstd 帧，每帧 ≤ 4 MiB 未压缩数据，帧头列出所含 chunk 的 hash 与长度。
- 派生缓存 = index.db 的 `derived(kind, tree, v, json)`，主键 `(kind, tree, v)`：`kind` 是 `stats` / `head`，`v` 是各自的口径版本（`STATS_VERSION`、`HEAD_VERSION`），读只查当前版本。
  主键带 v，所以口径版本不同的两个进程（还开着的旧版 serve 和刚装的新版）各写各的行，不会互相 `INSERT OR REPLACE` 掉对方的、没完没了地重算。
  - 迁移：早期的库主键是 `(kind, tree)`。打开时（`Store::migrate_derived`，一个写事务）把旧表改名 `derived_legacy`、按新主键建表、只拷最新快照的行（旧主键下每个 (kind, tree) 一行，量就是会话数 × 2）；改名是瞬间的，几个 GB 的旧表不在打开时删。拿不到写锁就警告一下照旧用旧表，下次打开再迁。
    旧版二进制打开迁移后的库照常工作：它的 `CREATE TABLE IF NOT EXISTS derived` 是空操作，读按 `(kind, tree, v)` 查、写 `INSERT OR REPLACE INTO derived(kind, tree, v, json)` 都不依赖主键形状，在新主键下只替换同版本那一行；它不碰 `derived_legacy`。
  - GC（`Store::gc_derived`）：只留 kind 是本版本认识的、v 是当前版本、tree 是某个源现在的最新快照（`normalize::live`）的行，其余（旧版本、被新快照取代的 tree、不认识的 kind、`derived_legacy` 整张）都删。
    每批 512 行一个写事务，不长时间挡 capture；每批在写锁里先读行、再对 manifest 算最新快照——派生行总在它的快照进 manifest 之后才写——所以 GC 期间新 capture 的快照不会被误删。
    删的只是缓存：别的版本的进程下次要用时发现没有，重算再写一行，不会出错（两个版本长期同时开着时，对方的行每次 GC 删一遍、各重算一次）。
    serve 在启动那一轮预热（全部最新快照的 stats / head 都进了进程内缓存）之后做一次，之后每 6 小时一次；也可以手动 `harl gc`。
  - 删掉的行释放的页留在文件里给以后的写入复用，文件不变小。`harl gc --vacuum` 再 VACUUM 把空间还给文件系统：要把整个库重写一遍，慢，期间挡住写入；WAL 模式下重写的页先进 WAL，临时要再占约一份库大小的磁盘，完成后 checkpoint 并截断 WAL。所以只在显式命令里做。

实测（404 个文件，521.6 MiB）：首次 capture 3.5 s → 151 MiB（29%）；之后每次增量只存新行，一个活跃会话的变化通常是几十 KiB；`check` 2.3 s。大文件的固定开销也小：12 万行（8.6 MiB）的会话追加一行再 capture，只存 3 个 chunk、9.8 KiB（平铺 tree 时是 3.7 MiB）。升级后旧 v2 记录的文件第一次再变化时会一次性存全部块（32 B × 行数），之后才是这个量级。

