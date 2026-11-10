// 与 `harl serve` 的 JSON API 契约。类型对应 crates/harl-core/src/event.rs 里 Event 的 serde 形状：
// snake_case；Option 为 None 的字段不出现；inferred / links 为空不出现；ext 总是存在。

import { t } from './i18n'

export type Provider = 'claude-code' | 'pfh' | 'dsh' | 'codex' | (string & {})

export interface Status {
  data_dir: string
  /** 本机的 machine id（随机串） */
  machine: string
  /** 存储里所有机器：id → 主机名 */
  machines: Record<string, string>
  sources: number
  records: number
  logical_bytes: number
  packs: number
  pack_bytes: number
  chunks: number
  index_bytes: number
}

export interface SessionInfo {
  /** 快照 tree hash，路由参数 */
  id: string
  provider: Provider
  machine: string
  /** 源文件绝对路径（统一 `/` 分隔） */
  source: string
  /** 会话 id，一般是文件名去掉 .jsonl */
  session: string
  /** 子 agent 时是父会话 id */
  parent_session: string | null
  size: number
  lines: number
  /** RFC3339 */
  mtime: string
  /** RFC3339 */
  captured_at: string
  /** 第一条 user 文本的第一行，没有就 null */
  title?: string | null
  /** 会话的工作目录（`/` 分隔），侧栏按它分组；日志开头没写就 null */
  cwd?: string | null
  /** 作为 agent 的类别 */
  kind: AgentKind
  /** agent 的短名字（Codex 的 agent 路径 / 昵称、workflow 脚本给的 label），不知道就 null */
  label?: string | null
  /** 同一次编排（一次 workflow 运行）的分组键，只在同一个根会话下面有意义 */
  run?: string | null
  /** 这次编排的由来：workflow 转述的用户请求的第一行 */
  run_title?: string | null
}

/** `review` 等是 provider 自己的子任务类别（workflow 的运行日志 journal 不是对话，后端不放进会话列表） */
export type AgentKind = 'main' | 'subagent' | 'fork' | 'workflow' | 'review' | (string & {})

export type Kind =
  | 'turn_started'
  | 'turn_completed'
  | 'step_started'
  | 'step_completed'
  | 'message'
  | 'tool_call'
  | 'tool_result'
  | 'request_header'
  /** turn 边界上 harness 跑的 hook；时长在 duration_ms */
  | 'hook'
  | 'meta'

export type Role = 'user' | 'assistant' | 'tool'

/** turn / step 的身份：id 用来分组，n 是从 1 数的序号（有些 harness 不给） */
export interface Scope {
  id: string
  n: number | null
}

export interface Tool {
  call_id: string
  /** Claude Code 的 tool_result 行没有工具名，这里是空串，UI 用配对的 tool_call 补 */
  name: string
  /** 后端按工具名归的类；Claude Code 的 tool_result 行没有名字，也就没有类别 */
  category?: ToolCategory
  input?: unknown
  output?: unknown
  /** 各家写法不一：Claude Code completed | failed，Codex / dsh ok | error，pfh 原样透传。判失败用 {@link toolFailed} */
  status?: string
  /** auto 模式分类器的判定；没送审就没有 */
  review?: Review
}

/** 工具状态里表示失败的值；和后端 stats.rs 的判断一致 */
const FAILED_STATUS = new Set(['failed', 'error'])

/** 这次工具调用失败了没有。各 provider 的 status 写法不同，统一在这里判断，别处不要再比字符串 */
export function toolFailed(status: string | null | undefined): boolean {
  return status != null && FAILED_STATUS.has(status)
}

export interface Review {
  verdict: 'allowed' | 'blocked' | 'unavailable'
  reason?: string
}

export interface Usage {
  input?: number
  output?: number
  cache_read?: number
  cache_write?: number
  /** 没映射的 usage 键（cache_creation、service_tier……）flatten 在这一层 */
  [k: string]: unknown
}

export type LinkRel = 'spawned' | 'notifies' | 'delivered_from' | (string & {})

export interface Link {
  rel: LinkRel
  session?: string
  call_id?: string
  task?: string
  inferred?: boolean
}

export interface Prov {
  /** raw 行的 chunk hash（blake3 hex） */
  chunk: string
}

export interface Event {
  provider: Provider
  session: string
  parent_session?: string
  /** 在源文件里的行号，从 0 数 */
  seq: number
  /** RFC3339 */
  ts?: string
  kind: Kind
  turn?: Scope
  step?: Scope
  role?: Role
  text?: string
  reasoning?: string
  tool?: Tool
  model?: string
  /** 推理强度：low / medium / high / xhigh / max */
  effort?: string
  /** 这次调用归属的 skill（Claude Code 的 attributionSkill） */
  skill?: string
  usage?: Usage
  /** 事件自己报的时长：turn_completed 是整个 turn，hook 是 hook 的执行时间 */
  duration_ms?: number
  /** 协作节点：插话、打断、上下文压缩 */
  signal?: 'steer' | 'interrupt' | 'compaction'
  inferred?: string[]
  links?: Link[]
  ext: Record<string, unknown>
  prov: Prov
}

// ---- GET /api/summary?tz=<分钟> ----

export interface Tokens {
  input: number
  output: number
  cache_read: number
  cache_write: number
  /** cache_write 里按 1 小时 TTL 写的部分（价格是 5 分钟的 1.6 倍） */
  cache_write_1h: number
  /** output 里的推理部分；只有报这个数的 harness（Codex）才有 */
  reasoning: number
}

/** 标签（推理强度、skill）→ 模型 → 聚合；带着模型是因为费用要按模型折算 */
export type ByTag = Record<string, Record<string, ModelAgg>>

/** 后端只给 token 数；费用由 web 端用 llm-pricing 折算，见 lib/pricing.ts 的 Priced* 类型 */
export interface ModelAgg {
  steps: number
  tokens: Tokens
  model_ms: number
}

/** 工具类别（后端按工具名归类；不同 harness 的 Bash / bash / PowerShell 都落到 shell） */
export type ToolCategory = 'shell' | 'read' | 'edit' | 'search' | 'web' | 'agent' | 'user' | 'mcp' | 'meta' | 'other'

export interface ToolAgg {
  category: ToolCategory
  calls: number
  failures: number
  /** user 类别的 ms 是在等人回答，不是执行时间 */
  ms: number
  /** 最后一次调用结果的时间（RFC3339）；STATS_VERSION 14 起才有 */
  last_ts?: string
  /** 用过它的会话数：只有 summary 的范围聚合（Summary.by_tool）带，没有就是 0 */
  sessions?: number
}

/** skill 加载 / shell 程序的使用表项 */
export interface UseAgg {
  /** skill：加载次数；程序：用到它的 shell 调用次数（一次调用里出现多次只算一次） */
  n: number
  /** 程序：所在调用失败的次数；没有就是 0 */
  failures?: number
  last_ts?: string
  /** 范围内用过它的会话数；没有就是 0 */
  sessions?: number
  /** skill 的目录或 SKILL.md 路径（Claude Code 的 Base directory、Codex 的 `<path>`、dsh 的 source.path；`bundled:x` 是内置的） */
  path?: string
  /** skill 的来源，按 path 和会话 cwd 推断 */
  source?: 'bundled' | 'plugin' | 'project' | 'user'
}

/**
 * 上下文组成（tokens）：七类同 lib/context.ts 的 Cat，other 是 provider 报的 prompt 里日志没记到的部分。
 * 后端按 composition.rs 估算再按实际 prompt 校准，和会话页的上下文视图口径一致但不逐 token 相同（图片不估）
 */
export interface Mix {
  system: number
  tools: number
  user: number
  inject: number
  skill: number
  assistant: number
  tool: number
  other: number
}

/** auto 模式分类器的审查：reviews 含放行、拦截和没给结论的 */
export interface ReviewAgg {
  reviews: number
  blocked: number
  unavailable: number
  /** 拦截原因 → 次数；没写原因的是空串 */
  by_reason: Record<string, number>
}

/** 协作节点：插话、打断、上下文压缩 */
export interface Signals {
  steers: number
  interrupts: number
  compactions: number
}

export interface SummaryTotals {
  sessions: number
  turns: number
  steps: number
  tool_calls: number
  tool_failures: number
  tokens: Tokens
  model_ms: number
  tool_ms: number
  wall_ms: number
  first_ts: string | null
  last_ts: string | null
  review: ReviewAgg
  signals: Signals
  /** turn 边界上 hook 的执行时间（Claude Code 的 stop hook） */
  hook_ms: number
}

export interface ProviderAgg {
  sessions: number
  turns: number
  tokens: Tokens
  model_ms: number
  tool_ms: number
  wall_ms: number
  by_model: Record<string, ModelAgg>
}

export interface DailyAgg {
  /** 本地日期 YYYY-MM-DD（按请求的 tz 分桶） */
  date: string
  turns: number
  steps: number
  tool_calls: number
  tokens: Tokens
  model_ms: number
  tool_ms: number
  wall_ms: number
  by_model: Record<string, ModelAgg>
  /** provider → 模型 → 聚合；按天定价时 provider 的费用从这里累加 */
  by_provider: Record<string, Record<string, ModelAgg>>
  /** 推理强度 → 步数 */
  by_effort: Record<string, number>
  /** 工具名 → 调用次数 */
  tools: Record<string, number>
  /** skill → 加载次数 */
  skills: Record<string, number>
  /** shell 程序 → 用到它的调用次数 */
  programs: Record<string, number>
  /** 这天各步 prompt 按上下文组成拆开之和 */
  composition: Mix
}

/** 本地时区的 (星期, 小时) 格子，只有有 turn 的格子 */
export interface HourCell {
  /** 0 = 周一 */
  weekday: number
  hour: number
  turns: number
  /** turn 的时间整块记在 turn 开始的那个小时 */
  model_ms: number
  tool_ms: number
  wall_ms: number
  by_model: Record<string, Tokens>
}

/** 仪表盘的过滤：provider、项目按会话，模型按 turn（见后端 summary.rs） */
export interface SummaryFilter {
  providers: string[]
  models: string[]
  /** 项目键（lib/project.ts 的 key：cwd 小写、`/` 分隔、去掉结尾斜杠）；空串是"拿不到 cwd 的会话"。
   * options 里给的是原始 cwd，由前端归成项目 */
  projects: string[]
}

/** 一个会话落在范围里的 turn 之和 */
export interface SessionRow {
  /** 快照 tree hash，会话页的路由参数 */
  id: string
  provider: Provider
  title: string | null
  cwd: string | null
  parent_session: string | null
  turns: number
  steps: number
  tool_calls: number
  tool_failures: number
  tokens: Tokens
  model_ms: number
  tool_ms: number
  wall_ms: number
  first_ts: string | null
  last_ts: string | null
  by_model: Record<string, ModelAgg>
  /** 单步最大的 prompt（新输入 + 缓存读写），即上下文最满的时候 */
  peak_ctx: number
  compactions: number
  /** 最后一条人打的消息（单行、最多 200 字符，截断时以 … 结尾）；整个会话的，不受范围影响 */
  last_user: string | null
  /** 会话末尾的上下文 tokens（最后一次请求的实际 prompt + 之后新增的估算），按它排序「当前上下文」；不受范围影响 */
  ctx: number
  /** 会话末尾上下文的组成，合计等于 ctx */
  ctx_parts: Mix
  /** ctx 全是估算：会话里没有一步报过 prompt */
  ctx_estimated: boolean
  /** 范围内各步 prompt 按上下文组成拆开之和（billed 输入的组成） */
  composition: Mix
  /** 范围内：工具名 → 调用次数 */
  tools: Record<string, number>
  /** 范围内：skill → 加载次数 */
  skills: Record<string, number>
  /** 范围内：shell 程序 → 用到它的调用次数 */
  programs: Record<string, number>
}

export interface Summary {
  tz_offset_min: number
  /** 请求的范围回显（本地日期，两端都含；since 是滚动窗口的 UTC 起点）；没限就是 null */
  range: { from: string | null; to: string | null; since: string | null }
  /** 请求的过滤回显；空数组表示不限 */
  filter: SummaryFilter
  /** 能选的 provider / 模型：全部数据里出现过的，不受过滤和范围影响 */
  options: SummaryFilter
  /** 全部数据的 UTC 时间跨度，不受 range 影响；没数据时两个都是 null */
  available: [string | null, string | null]
  totals: SummaryTotals
  by_provider: Record<string, ProviderAgg>
  by_model: Record<string, ModelAgg>
  by_tool: Record<string, ToolAgg>
  /** 按类别汇总；只出现有数据的类别 */
  by_category: Partial<Record<ToolCategory, ToolAgg>>
  /** 推理强度（low / medium / high / xhigh / max）→ 模型 → 聚合；只含带强度的步 */
  by_effort: ByTag
  /** skill → 模型 → 聚合；只含归属了 skill 的步（Claude Code 的 attributionSkill） */
  by_skill: ByTag
  /** skill 加载（不是 by_skill 的按步归属）：次数、最后一次、会话数、路径与来源 */
  skills: Record<string, UseAgg>
  /** shell 命令用到的程序（`cd x && npm test | tee log` → npm、tee）：次数、失败、最后一次、会话数 */
  programs: Record<string, UseAgg>
  /** 范围内全部 prompt 按上下文组成拆开 */
  composition: Mix
  /** 只包含有数据的日期，中间可能有空洞 */
  daily: DailyAgg[]
  hourly: HourCell[]
  /** 范围内有 turn 的会话，按最后活动时间倒序 */
  sessions: SessionRow[]
  turn_durations: TurnDurations
}

/** 范围内 turn 的墙钟时间分布 */
export interface TurnDurations {
  /** 分桶上界（ms，不含）；counts 比它多一桶，最后一桶没有上界 */
  edges_ms: number[]
  counts: number[]
  p50_ms: number
  p90_ms: number
  max_ms: number
  /** 最慢的几个 turn，从慢到快 */
  slowest: SlowTurn[]
}

export interface SlowTurn {
  /** 会话的快照 id，会话页路由参数 */
  session_id: string
  provider: Provider
  title: string | null
  /** turn id；会话页的锚点是 `turn-t:<id>` */
  turn: string
  ts: string
  wall_ms: number
  /** 模型时间 + 工具时间；墙钟里剩下的是等人、闲置 */
  active_ms: number
  steps: number
  tool_calls: number
  by_model: Record<string, ModelAgg>
}

/** `/api/version`：当前版本和 GitHub Releases 上的最新版本 */
export interface VersionInfo {
  current: string
  /** 没查（关了检查）或查不到（离线、还没发布过）时是 null */
  latest: { version: string; url: string; notes: string; published_at: string | null } | null
  update_available: boolean
  /** 这个进程能不能自己更新；不能时 reason 说明为什么（开发构建、目录不可写、没有本平台的包） */
  can_update: boolean
  reason: string | null
  /** 这个进程的自更新走到哪：installing 正在下载替换，installed 已换上、等重启。老后端没有这个字段，当 idle */
  phase?: 'idle' | 'installing' | 'installed'
}

export class ApiError extends Error {
  constructor(
    public readonly status: number,
    public readonly url: string,
    message: string,
  ) {
    super(message)
    this.name = 'ApiError'
  }
}

/** slim 事件里省略掉的 base64 留下的占位：`harl:elided:<seq>:<序号>:<原长度>`（后端 serve.rs 的 elided_marker） */
export const ELIDED = /harl:elided:(\d+):(\d+):(\d+)/g

export interface Elided {
  marker: string
  seq: number
  ord: number
  len: number
}

/** 一段文本里的占位，去重 */
export function elidedIn(text: string): Elided[] {
  const out = new Map<string, Elided>()
  for (const m of text.matchAll(ELIDED)) out.set(m[0], { marker: m[0], seq: +m[1]!, ord: +m[2]!, len: +m[3]! })
  return [...out.values()]
}

async function getJson<T>(url: string, signal?: AbortSignal): Promise<T> {
  let res: Response
  try {
    res = await fetch(url, { headers: { accept: 'application/json' }, signal })
  } catch (e) {
    if (e instanceof DOMException && e.name === 'AbortError') throw e
    throw new ApiError(0, url, t('common.api.network', { message: (e as Error).message }))
  }
  if (!res.ok) {
    const body = (await res.text().catch(() => '')).slice(0, 300)
    throw new ApiError(res.status, url, `${res.status} ${res.statusText}${body ? ` — ${body}` : ''}`)
  }
  return (await res.json()) as T
}

export const api = {
  status: (signal?: AbortSignal) => getJson<Status>('/api/status', signal),
  sessions: (signal?: AbortSignal) => getJson<SessionInfo[]>('/api/sessions', signal),
  /**
   * 会话的事件。slim：大块 base64（图片、文件）换成 {@link ELIDED} 占位，大会话的体积常常少一大半；
   * token 估算不读 base64（图片按尺寸算），折出来的上下文数字和完整版一样。要看原文用 {@link api.blob}
   */
  events: (id: string, signal?: AbortSignal) =>
    getJson<Event[]>(`/api/sessions/${encodeURIComponent(id)}/events?slim=1`, signal),
  /** slim 事件里一处占位的原文（不含 data URL 的前缀） */
  blob: async (id: string, seq: number, ord: number, signal?: AbortSignal): Promise<string> => {
    const url = `/api/sessions/${encodeURIComponent(id)}/blobs/${seq}/${ord}`
    let res: Response
    try {
      res = await fetch(url, { signal })
    } catch (e) {
      if (e instanceof DOMException && e.name === 'AbortError') throw e
      throw new ApiError(0, url, t('common.api.network', { message: (e as Error).message }))
    }
    if (!res.ok) throw new ApiError(res.status, url, `${res.status} ${res.statusText}`)
    return res.text()
  },
  /** tz 是本地时区相对 UTC 的分钟数（东为正），后端按它切本地日期；from / to 是本地日期 YYYY-MM-DD，两端都含 */
  summary: (
    tzOffsetMin: number,
    from?: string | null,
    to?: string | null,
    filter?: SummaryFilter,
    signal?: AbortSignal,
    /** 滚动窗口的起点（ISO 时刻，含）；和 from 同时给时后端取更晚的那个 */
    since?: string | null,
  ) => {
    const q = new URLSearchParams({ tz: String(tzOffsetMin) })
    if (since) q.set('since', since)
    if (from) q.set('from', from)
    if (to) q.set('to', to)
    if (filter?.providers.length) q.set('provider', filter.providers.join(','))
    if (filter?.models.length) q.set('model', filter.models.join(','))
    // 路径可能含逗号，项目用换行分隔
    if (filter?.projects.length) q.set('project', filter.projects.join('\n'))
    return getJson<Summary>(`/api/summary?${q}`, signal)
  },
  /** force：不用后端缓存，马上去 GitHub 查一次 */
  version: (force = false, signal?: AbortSignal) => getJson<VersionInfo>(`/api/version${force ? '?force=true' : ''}`, signal),
  /** 下载并换上最新版本；成功后后端随即重启，调用方轮询 version 等它回来 */
  update: async (): Promise<{ version: string }> => {
    const res = await fetch('/api/update', { method: 'POST', headers: { accept: 'application/json' } })
    if (!res.ok) throw new ApiError(res.status, '/api/update', (await res.text().catch(() => '')) || `${res.status}`)
    return res.json()
  },
  /** 原始 JSONL 的 URL（text/plain），直接开新标签用 */
  rawUrl: (id: string) => `/api/sessions/${encodeURIComponent(id)}/raw`,
}
