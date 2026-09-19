// 与 `harl serve` 的 JSON API 契约。类型对应 crates/harl-core/src/event.rs 里 Event 的 serde 形状：
// snake_case；Option 为 None 的字段不出现；inferred / links 为空不出现；ext 总是存在。

export type Provider = 'claude-code' | 'pfh' | (string & {})

export interface Status {
  data_dir: string
  machine: string
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
  /** 可选：会话标题（比如第一条 user 文本截断）。契约里没有，后端给了列表就显示 */
  title?: string
}

export type Kind =
  | 'turn_started'
  | 'turn_completed'
  | 'step_started'
  | 'step_completed'
  | 'message'
  | 'tool_call'
  | 'tool_result'
  | 'request_header'
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
  input?: unknown
  output?: unknown
  /** pfh 原样透传；Claude Code 是 completed | failed */
  status?: string
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
  usage?: Usage
  inferred?: string[]
  links?: Link[]
  ext: Record<string, unknown>
  prov: Prov
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

async function getJson<T>(url: string, signal?: AbortSignal): Promise<T> {
  let res: Response
  try {
    res = await fetch(url, { headers: { accept: 'application/json' }, signal })
  } catch (e) {
    if (e instanceof DOMException && e.name === 'AbortError') throw e
    throw new ApiError(0, url, `网络错误：${(e as Error).message}`)
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
  events: (id: string, signal?: AbortSignal) =>
    getJson<Event[]>(`/api/sessions/${encodeURIComponent(id)}/events`, signal),
  /** 原始 JSONL 的 URL（text/plain），直接开新标签用 */
  rawUrl: (id: string) => `/api/sessions/${encodeURIComponent(id)}/raw`,
}
