# harl cloud：登录、统计上传、个人主页与徽章

状态：草案（2026-10）。定稿前不写代码。

## 目标

- 命令行和 WebUI 都能用 GitHub 账号登录。
- 登录后把**统计汇总**定时上传到 harl 云服务。
- 云服务提供个人主页 `/u/<github login>`，以及可以放进 README 的 shields 徽章。

## 非目标

- **不上传会话内容**。原始会话仍走 `[remote]` 同步到用户自己的对象存储，与本服务无关。
- 不做排行榜和跨用户对比（以后再说，见“待定”）。
- 不保证数据真实：数据是用户自己上报的，可以伪造。徽章和主页只代表“本人声称”。

## 决策

| 决策 | 选择 | 理由 / 放弃的方案 |
| --- | --- | --- |
| 存储位置 | 自建服务 | gist 要高频改写，还做不了个人主页 |
| 上传内容 | 只传按小时汇总的数字 | 服务端不碰代码、路径、提示词，数据泄露的代价很小 |
| 身份 | 只用 GitHub | 徽章放在 GitHub README 里，login 天然是公开 ID；Google 带来账号合并问题，收益小 |
| 登录方式 | OAuth Device Flow | 不需要本地回调端口，SSH 远程机器上的 `harl agent` 也能登录 |
| 服务端 | Cloudflare Workers + D1（TypeScript） | 免运维、近乎零成本，徽章走边缘缓存；前端已在用 `llm-pricing`，服务端可以直接复用来算费用 |
| 时间粒度 | UTC 小时桶 | 主页可以按访问者时区显示，也能支持“最近 24h”；按天分桶做不到 |
| 费用 | 服务端读取时计算 | 客户端只传 token；价格目录会更新，存 token 才能事后重算 |

## 认证

### 命令行：`harl login`

1. 向服务端请求 `GET /v1/auth/config`，拿到 GitHub OAuth App 的 `client_id`。`client_id` 是公开值，放在服务端是为了以后换 App 时不用发新客户端。
2. 直接向 GitHub `POST /login/device/code` 发起 device flow，scope 留空（只需要读公开资料）。
3. 打印 `user_code` 和 `verification_uri`，尝试打开浏览器，然后按 GitHub 给的 `interval` 轮询拿 access token。
4. `POST /v1/auth/github { github_token, machine_id, machine_name }`。服务端用这个 token 调 GitHub `/user` 验证身份，upsert 用户，签发 **harl token**（32 字节随机数，服务端只存 sha256），然后返回。
5. 客户端把 harl token 写进 `~/.harl/credentials`（Unix 下权限 0600），**GitHub token 用完即丢**，服务端也不保存。

- 每台机器一个 token，可以在主页上单独吊销。
- `harl logout` 调 `POST /v1/auth/revoke` 吊销 token，再删除本地凭据。

### WebUI

浏览器始终拿不到 token。本地 `harl serve` 提供下面这些接口，由它代为执行上面的流程：

- `POST /api/cloud/login` → `{ user_code, verification_uri, expires_in }`
- `GET  /api/cloud/login/poll` → `pending | ok { login, avatar } | expired | denied`
- `POST /api/cloud/logout`
- `GET  /api/cloud/status` → 登录身份、是否开启上传、上次上传时间和错误

## 上传

### 数据模型

上传的数据分两张表，主键都包含 `machine_id`，所以多台机器的数据可以相加而不重复计数：

- **usage**：键为 `(machine_id, hour, provider, model)`，字段有 `steps`、tokens（`input / output / cache_read / cache_write / cache_write_1h / reasoning`）和 `model_ms`。
- **activity**：键为 `(machine_id, hour, provider)`，字段有：
  - `sessions_started`：按会话的 `first_ts` 归到小时，保证可以相加；不按“活跃会话数”统计，因为跨小时的活跃会话无法相加。
  - `turns`、`tool_calls`、`tool_failures`、`wall_ms`。

归属规则：一个 turn 的全部 token 和计数都记到该 turn `ts` 所在的小时。跨整点的 turn 不拆分。

**不上传**：项目名、路径、session id、提示词、工具参数、skill 名（自定义 skill 名可能暴露私有信息）。机器名默认是 `machine-<id 前 6 位>`，不用 hostname，用户可以自己改。

### 协议

`POST /v1/ingest`，请求头 `Authorization: Bearer <harl token>`：

```json
{
  "schema": 1,
  "usage":    [{ "machine": "…", "hour": "2026-10-14T09:00:00Z", "provider": "claude-code", "model": "claude-opus-5-5", "steps": 12, "tokens": { … }, "model_ms": 81234 }],
  "activity": [{ "machine": "…", "hour": "2026-10-14T09:00:00Z", "provider": "claude-code", "sessions_started": 1, "turns": 4, "tool_calls": 30, "tool_failures": 1, "wall_ms": 610000 }]
}
```

- 语义是**整桶覆盖**，不是累加。客户端每次都用本地全量数据重算一个桶，所以重复推送、断线重推都是幂等的。
- 一个小时的数据上传时，必须带上该小时下这台机器的全部行。服务端先删除 `(user, machine, hour)` 的旧行，再写入新行，这样模型被改名或消失也能正确处理。
- 单个请求最多 500 个小时桶，超过就分批。

### 客户端什么时候传

- 复用 `serve` 和 `agent` 现有的同步循环，没有配置 `[remote]` 也会运行。
- 本地维护 `cloud-state.json`，记录每个小时桶已上传内容的哈希。每轮只重算“本轮有新快照的会话”涉及的小时，哈希变了才上传。
- 首次登录时回填全部历史，按时间倒序分批上传，让主页先显示最近的数据。
- 上传本机能看到的**所有机器**的记录，包括通过 `[remote]` 拉下来的，`machine` 字段取记录本身的值。因为主键包含 machine，同一账号下两台机器都上传同一份数据也只是覆盖成相同的值。
- 服务端对每个 token 限流（例如每 10 秒最多 1 次），客户端遇到 429 就退避重试。

## 服务端

### 路由

| 路由 | 认证 | 说明 |
| --- | --- | --- |
| `GET  /v1/auth/config` | – | `client_id` |
| `POST /v1/auth/github` | – | 用 GitHub token 换 harl token |
| `POST /v1/auth/revoke` | token | 吊销当前 token |
| `POST /v1/ingest` | token | 上传 |
| `GET  /v1/me` | token / 会话 | 资料、机器列表、可见性设置 |
| `PATCH /v1/me` | token / 会话 | 修改可见性、机器名 |
| `DELETE /v1/me` | token / 会话 | 删除账号和全部数据 |
| `DELETE /v1/me/machines/{id}` | token / 会话 | 删除一台机器的数据并吊销它的 token |
| `GET  /v1/users/{login}/summary?range=` | – | 公开汇总，受可见性设置控制 |
| `GET  /badge/{login}/{metric}.json?range=` | – | shields endpoint |
| `GET  /u/{login}` | – | 个人主页 |

个人主页的“设置”页需要浏览器登录。这里走标准的 GitHub OAuth web flow（回调到云服务自己的域名），登录后种一个 HttpOnly cookie。这是唯一需要 client secret 的地方，secret 只存在 Worker 的 secrets 里。

### D1 表结构

```sql
CREATE TABLE users (
  id          INTEGER PRIMARY KEY,   -- GitHub user id，用户改名后不变
  login       TEXT NOT NULL UNIQUE,  -- 每次登录时更新
  avatar_url  TEXT,
  public      INTEGER NOT NULL DEFAULT 0,  -- 主页是否公开
  badges      TEXT NOT NULL DEFAULT '[]',  -- 允许公开的徽章指标
  created_at  INTEGER NOT NULL
);
CREATE TABLE machines (
  user_id INTEGER NOT NULL, id TEXT NOT NULL, name TEXT NOT NULL, last_seen INTEGER,
  PRIMARY KEY (user_id, id)
);
CREATE TABLE tokens (
  hash BLOB PRIMARY KEY, user_id INTEGER NOT NULL, machine_id TEXT NOT NULL,
  created_at INTEGER NOT NULL, last_used INTEGER
);
CREATE TABLE usage (
  user_id INTEGER NOT NULL, machine_id TEXT NOT NULL, hour INTEGER NOT NULL,  -- unix 秒，整点
  provider TEXT NOT NULL, model TEXT NOT NULL,
  steps INTEGER, input INTEGER, output INTEGER, cache_read INTEGER, cache_write INTEGER,
  cache_write_1h INTEGER, reasoning INTEGER, model_ms INTEGER,
  PRIMARY KEY (user_id, machine_id, hour, provider, model)
);
CREATE TABLE activity (
  user_id INTEGER NOT NULL, machine_id TEXT NOT NULL, hour INTEGER NOT NULL, provider TEXT NOT NULL,
  sessions_started INTEGER, turns INTEGER, tool_calls INTEGER, tool_failures INTEGER, wall_ms INTEGER,
  PRIMARY KEY (user_id, machine_id, hour, provider)
);
```

数据量估算：重度用户每天 24 小时 × 3 个模型，一年约 2.6 万行 usage，在 D1 的承受范围内。以后需要再加按天的物化表。

### 费用与缓存

- 费用在读取时用 `llm-pricing` 按 `(model, tokens, hour)` 折算，口径和本地 dashboard 一致，都是“等价 API 价，不是账单”。
- 汇总结果用 Cache API 缓存 5 分钟。徽章响应带 `Cache-Control: max-age=300`，JSON 里设 `cacheSeconds: 300`，shields 那边也会缓存。

## 徽章

shields endpoint 格式：`{ "schemaVersion": 1, "label": "…", "message": "…", "color": "…", "cacheSeconds": 300 }`。

| metric | 示例 |
| --- | --- |
| `tokens` | `harl tokens │ 1.2B` |
| `cost` | `harl spend │ $3.4k` |
| `sessions` | `harl sessions │ 812` |
| `hours` | `agent hours │ 420h`（取自 `wall_ms`） |
| `model` | `top model │ opus-5.5` |

`range` 可选 `7d`、`30d`、`all`，默认 `30d`。用户没有公开的指标返回灰色的 `private`，不返回 404，避免 README 里显示一张破图。

README 中的用法：

```md
![](https://img.shields.io/endpoint?url=https://<域名>/badge/jannchie/tokens.json)
```

## 隐私与同意

- **登录不等于同意上传**。是否上传由 `[cloud] upload` 控制，`harl login` 结束时询问，默认是。
- 主页默认**不公开**，徽章指标逐个开启。
- 用户可以删除整个账号、删除单台机器的数据，也可以吊销单个 token。
- 服务端不记录请求体日志。

## 客户端改动

- `config.toml` 新增：
  ```toml
  [cloud]
  url = "https://<域名>"   # 默认官方服务，自建时可以改
  upload = true
  ```
- 新增 `~/.harl/credentials` 和 `~/.harl/cloud-state.json`。
- 新增命令 `harl login`、`harl logout`，并在 `harl status` 里显示云端状态。
- 新增 `harl-core` 函数：按小时桶聚合，输入是 `SessionStats` 的 `turn_stats`，输出是 usage 行和 activity 行。
- `serve` 新增 `/api/cloud/*` 接口；WebUI 侧栏加登录入口，设置页显示上传状态。

## 分阶段

1. **骨架**：Worker 路由、D1 迁移、device flow 认证、`harl login` 和 `harl logout`。
2. **上传**：小时桶聚合、`/v1/ingest`、增量与回填。
3. **徽章**：`/badge`，以及主页设置里的可见性开关。
4. **主页**：`/u/{login}`，包括日历热力图、趋势、模型占比，复用 web 里现有的 dash 组件。
5. **WebUI 登录**。

## 待定

- 域名。
- 主页的前端放在哪里：直接用这个仓库 `web/` 的组件打包，还是单独建一个 Pages 项目。
- 排行榜：数据可以伪造，需要先想清楚怎么防作弊，或者干脆只做“好友对比”。
- 服务端代码放在哪里：放进这个仓库的 `cloud/`（pnpm workspace 成员），还是单独建仓库。我倾向放在同一个仓库，上传 schema 就能和客户端在同一个 PR 里修改。
