// Portions derived from dsh-context (Apache-2.0, © 2025 bowenliang123), modified for harl. See THIRD_PARTY_NOTICES.md.
// 上下文视图的折叠：把 canonical 事件流重放一遍，还原每次模型请求时上下文里装了什么。
// 思路取自 dsh-context（DeepSeek Harness 的上下文插件），但不依赖任何 harness 的运行时：
// 只用归档下来的日志，所以四家 provider 都能看。
//
// - 上下文 = system prompt + 工具 schema + 消息面（surface）。消息面按事件顺序追加，压缩时整段移走（记 gone）。
// - 一次请求（step）在它的第一条回复之前拍快照；某个 step 的上下文就是 seq 在它之前、还没被移走的节点。
// - 节点的 token 是估算（ASCII 4 字符 ≈ 1 token、CJK 约 1.5 字符 ≈ 1 token，每块再加 4），
//   有 provider 报的 prompt tokens 时按它校准：system 和工具都记在日志里就按比例缩放，
//   记不全（老版 Claude Code、Codex 不记工具 schema）时差额单列成「未记录」，不把它摊进别的类别。

import { toolFailed, type Event, type Tokens, type Usage } from '../api'
import { prettyJson, truncate } from './format'
import { outputText, safeJson } from './group'
import { promptTokens } from './summaryModel'

/** 七类和 dsh-context 一致；other 是 provider 报的 prompt 里日志没记到的部分 */
export type Cat = 'system' | 'tools' | 'user' | 'inject' | 'skill' | 'assistant' | 'tool' | 'other'
const CATS: Cat[] = ['system', 'tools', 'user', 'inject', 'skill', 'assistant', 'tool']
export const ALL_CATS: Cat[] = [...CATS, 'other']

export type Parts = Record<Cat, number>
export function zeroParts(): Parts {
  return { system: 0, tools: 0, user: 0, inject: 0, skill: 0, assistant: 0, tool: 0, other: 0 }
}
export function sumParts(p: Parts): number {
  let s = 0
  for (const c of ALL_CATS) s += p[c]
  return s
}

// ---- token 估算 ----

/** 一段文本的 token 估算：ASCII 4 字符 1 个，其余（CJK 等）1.5 字符 1 个 */
function estimateText(s: string | null | undefined): number {
  if (!s) return 0
  let ascii = 0
  let wide = 0
  for (let i = 0; i < s.length; i++) {
    if (s.charCodeAt(i) < 128) ascii++
    else wide++
  }
  return Math.ceil(ascii / 4 + wide / 1.5)
}

/** 一块内容（text / reasoning / 工具参数……）：正文 + 4 的块开销 */
function block(s: string | null | undefined): number {
  return s ? estimateText(s) + 4 : 0
}

function json(v: unknown): string {
  return v == null ? '' : typeof v === 'string' ? v : safeJson(v)
}

/** 展开时显示的正文：字符串原样，其余 pretty JSON */
function pretty(v: unknown): string {
  return typeof v === 'string' ? v : prettyJson(v)
}

/**
 * 图片的 token：DeepSeek 按 14px patch、3×3 合并成网格，行数 ×（列数 + 1）+ 2；面积不到 544² 先放大，
 * 超过 1024 按宽高比解出放得下的网格，缩放到不动点为止（dsh-context v0.83 的口径）。
 * 再加每张图约 213 的固定开销：归档里 DeepSeek 相邻两次请求的实际 prompt 差减去新增文字的估算，
 * 640² 到 2423×1731 的图都稳定比网格多出这么多。其他模型按 Anthropic 的 w×h/750（≤1600）。没有尺寸给个保守值。
 */
function estimateImage(w: number | undefined, h: number | undefined, deepseek: boolean): number {
  if (!w || !h) return deepseek ? 1024 + DEEPSEEK_IMAGE_OVERHEAD : 1600
  if (!deepseek) return Math.min(1600, Math.ceil((w * h) / 750))
  return (deepseekGrid(w, h) ?? 1024) + DEEPSEEK_IMAGE_OVERHEAD
}

const DEEPSEEK_IMAGE_OVERHEAD = 213
const DEEPSEEK_IMAGE_MIN_AREA = 544 * 544
const DEEPSEEK_IMAGE_CAP = 1024

interface ImageFit {
  w: number
  h: number
  tokens: number
}

function ceilDiv(a: number, b: number): number {
  return Math.floor((a + b - 1) / b)
}

/** 缩放后尺寸（14 的倍数）的网格 token：每 3 个 patch 合并成一格，每行多一个换行位，首尾各一个 */
function gridTokens(w: number, h: number): number {
  const cols = ceilDiv(Math.floor(w / 14), 3)
  const rows = ceilDiv(Math.floor(h / 14), 3)
  return rows * (cols + 1) + 2
}

/** 一轮缩放：小图放大到最小面积，边长补到 14 的倍数；超了上限就按宽高比解一个放得下的网格 */
function fitImage(w: number, h: number): ImageFit {
  let ww = w
  let hh = h
  if (ww * hh < DEEPSEEK_IMAGE_MIN_AREA) {
    const k = Math.sqrt(DEEPSEEK_IMAGE_MIN_AREA / (ww * hh))
    ww = Math.trunc(ww * k)
    hh = Math.trunc(hh * k)
  }
  const bw = ceilDiv(ww, 14) * 14
  const bh = ceilDiv(hh, 14) * 14
  const tokens = gridTokens(bw, bh)
  if (tokens <= DEEPSEEK_IMAGE_CAP) return { w: bw, h: bh, tokens }
  // rows ×（cols + 1）≈ cap − 2，rows = cols × h/w：解出列数
  const ratio = h / w
  const cols = Math.sqrt((DEEPSEEK_IMAGE_CAP - 2) / ratio + 0.25) - 0.5
  const rows = cols * ratio
  let fw: number
  let fh: number
  if (cols < 1) {
    fw = 42
    fh = Math.floor((DEEPSEEK_IMAGE_CAP - 2) / 2) * 42
  } else if (rows < 1) {
    fw = (DEEPSEEK_IMAGE_CAP - 3) * 42
    fh = 42
  } else {
    const k = Math.min((Math.trunc(cols) * 42) / w, (Math.trunc(rows) * 42) / h)
    fw = Math.trunc((w * k) / 14) * 14
    fh = Math.trunc((h * k) / 14) * 14
  }
  return { w: fw, h: fh, tokens: gridTokens(fw, fh) }
}

/** 反复缩放到尺寸不再变；10 轮还不稳定就算不出来 */
function deepseekGrid(w: number, h: number): number | null {
  let f = fitImage(w, h)
  for (let i = 1; i < 10; i++) {
    const g = fitImage(f.w, f.h)
    if (g.w === f.w && g.h === f.h && g.tokens === f.tokens) return f.tokens <= DEEPSEEK_IMAGE_CAP ? f.tokens : null
    f = g
  }
  return null
}

interface ImageInfo {
  width?: number
  height?: number
  name?: string
  bytes?: number
  /** 估算的 token（折叠时按当时的模型填） */
  tokens?: number
}

/** 内容里的图片块（content part 数组、tool 结果数组，递归找 type=image） */
function imagesIn(v: unknown, out: ImageInfo[] = [], depth = 0): ImageInfo[] {
  if (depth > 6 || v == null || typeof v !== 'object') return out
  if (Array.isArray(v)) {
    for (const x of v) imagesIn(x, out, depth + 1)
    return out
  }
  const o = v as Record<string, unknown>
  if (o.type === 'image' || o.type === 'input_image') {
    const a = (o.attachment ?? o.source ?? o) as Record<string, unknown>
    const num = (x: unknown) => (typeof x === 'number' ? x : undefined)
    out.push({
      width: num(a.width),
      height: num(a.height),
      name: typeof a.name === 'string' ? a.name : undefined,
      bytes: num(a.bytes),
    })
    return out
  }
  for (const x of Object.values(o)) if (x && typeof x === 'object') imagesIn(x, out, depth + 1)
  return out
}

// ---- 节点 ----

export type AsstKind = 'thinking' | 'tools' | 'answer'

/** 助手内容按三种拆开的 token：思考、回答正文、工具调用（名字 + 参数）。三者之和等于节点的估算 */
export interface Asst3 {
  reasoning: number
  answer: number
  call: number
}
export function zeroAsst(): Asst3 {
  return { reasoning: 0, answer: 0, call: 0 }
}
function addAsst(into: Asst3, from: Asst3, k = 1) {
  into.reasoning += from.reasoning * k
  into.answer += from.answer * k
  into.call += from.call * k
}

/** 上下文里的一项：一条消息、一份 system prompt、一个工具 schema */
export interface CtxNode {
  /** 稳定键：sys / tool:<name> / n<seq> */
  key: string
  seq: number
  cat: Cat
  /** 估算的 token */
  est: number
  /** 一行摘要 */
  label: string
  /** 来源：工具名、注入方（插件 / 文件 / skill 名）、附件类型 */
  name?: string
  /** 工具调用失败 */
  err?: boolean
  /** assistant 节点含的内容种类 */
  kinds?: AsstKind[]
  /** assistant 节点的三分估算（思考 / 回答 / 工具调用） */
  asst?: Asst3
  /** 消息面上的排序位置（seq 口径）：压缩摘要插在第一条被移走的节点那里，不排在末尾 */
  pos?: number
  images?: ImageInfo[]
  /** 正文（展开时显示）；大的 JSON 在展开时才序列化 */
  body: () => string
  /** 正文是 JSON（工具参数、schema）还是文本 */
  bodyKind: 'text' | 'json' | 'markdown'
  /** 原事件；system / 工具 schema 没有单一事件时是带出它的那个 */
  ev: Event
  /** 节点所在的 step（第几次请求的输出 / 输入），从 0 数；system / 工具是带出它们时的下一次请求 */
  step: number
  /** 被哪个事件（seq）移出上下文：压缩、修剪 */
  gone?: number
  /** dsh 自己的行号（surfaceOp / shadowedSeqs 用它） */
  rawSeq?: number
  /** tool 结果配对的调用 */
  call?: Event
  /** 工具 schema 上显式写的来源插件 */
  plugin?: string
}

export type CtxEventKind = 'inject' | 'compaction' | 'prune' | 'model' | 'mode'

/** 上下文事件：注入、压缩、修剪、模型切换、模式切换 */
export interface CtxEvent {
  kind: CtxEventKind
  seq: number
  ts?: string
  /** 发生时下一次请求（从 0 数）；之后没有请求了就是 steps.length */
  step: number
  /** 注入的 token；压缩是腾出来的 */
  tokens: number
  /** 注入方、skill 名、模型 from → to、模式 */
  name: string
  /** 注入的形式（instructions / skill / attachment 类型）、压缩的触发方式 */
  form?: string
  /** 压缩移走的条数 */
  count?: number
  /** provider 报的压缩前 tokens（Claude Code 的 compactMetadata.preTokens） */
  preTokens?: number
  /** 压缩 / 修剪的跨度：前一次请求 → 后一次请求（「T1 S3→4」）；两头没有请求时是 null */
  span?: { from: StepPos | null; to: StepPos | null }
  node?: CtxNode
}

/** 请求在会话里的位置：第几轮（turn.n）的第几次请求（从 1 数） */
export interface StepPos {
  idx: number
  turn: number | null
  step: number
}

/**
 * 一次请求里的一段时间（epoch ms）。ttft / reasoning / text / toolArgs 来自 dsh 的流式块时间；
 * 没有块时间的 provider 生成段记成 model。tools = 工具调用 → 结果；overhead = 没被覆盖的；
 * wait = 等人（问用户、审批）；idle = 开销里 ≥ 2 分钟没有任何事件的空档
 */
export type SpanKind = 'ttft' | 'reasoning' | 'text' | 'toolArgs' | 'model' | 'tools' | 'overhead' | 'wait' | 'idle'
export type BlockKind = 'reasoning' | 'text' | 'toolArgs'

export interface Span {
  kind: SpanKind
  start: number
  end: number
}

/** 一次模型请求 */
export interface StepRec {
  idx: number
  /** 第一条回复的 seq：快照拍在它之前 */
  seq: number
  /** 最后一条回复的 seq */
  endSeq: number
  ts?: string
  endTs?: string
  /** step 开始（step_started 或上一个事件）的时间：模型时间从这里算 */
  startTs?: string
  turnId: string | null
  turnN: number | null
  model: string | null
  /** 估算的组成 */
  parts: Parts
  /** 每类的条数 */
  counts: Record<Cat, number>
  usage: Tokens | null
  /** input + 缓存读写 = 这次请求的上下文；没有 usage 时 null */
  prompt: number | null
  /** 按 usage 校准后的组成；没有 usage 时等于 parts */
  anchored: Parts
  /** 这一步的上下文里有没有被移走的节点（压缩前的步，快照是重建的） */
  approximate: boolean
  /** 模型时间（ms）：开始 → 最后一条回复 */
  modelMs: number
  /** 首 token 延迟（只有 dsh 记流式时间） */
  ttftMs?: number
  /** 这一步发起的工具跑了多久 */
  toolMs: number
  /** 快照时消息面上 assistant 的三分合计（之和等于 parts.assistant） */
  asst: Asst3
  /** 这次请求生成的三分：思考优先用 provider 报的 reasoning tokens（不超过 output），其余按估算 */
  gen: Asst3
  /** 窗口：开始 → step 结束（step_completed，或它发起的工具最后一个结果、最后一条回复） */
  wallMs: number
  /** 窗口减去等人的时间 */
  activeMs: number
  /** 窗口里按时间排好的各段；没有时间戳时是空的 */
  spans: Span[]
  /** dsh 流式块的耗时与块数（没有块时间时不给） */
  blocks?: Record<BlockKind, { ms: number; n: number }>
}

export interface Turn {
  id: string
  n: number | null
  /** 这一轮的请求 [first, last] */
  first: number
  last: number
  ts?: string
  endTs?: string
  /** 显式时长或首尾之差 */
  wallMs: number
}

export interface ToolTime {
  calls: number
  failures: number
  ms: number
}

export interface CtxFold {
  nodes: CtxNode[]
  /** 每份 system prompt（换过就有多份），按出现顺序 */
  systems: CtxNode[]
  /** 工具 schema 的每一版（epoch）：从 seq 起生效 */
  toolEpochs: { seq: number; tools: CtxNode[] }[]
  steps: StepRec[]
  turns: Turn[]
  events: CtxEvent[]
  /** 模型的上下文窗口（dsh 的 request/context 会报） */
  window: number | null
  /** 单次请求的最大输出（dsh header 的 maxTokens），用来画保留区 */
  maxOutput: number | null
  /** system 和工具 schema 都在日志里（校准时按比例缩放，否则差额单列） */
  complete: boolean
  /** 人发的消息（不含注入） */
  humanInputs: number
  /** 带正文的回复 */
  answers: number
  byTool: Record<string, ToolTime>
  /** call_id → 调用 / 结果 */
  calls: Map<string, { call?: Event; result?: Event }>
  /** 各次请求生成的三分之和 */
  generated: Asst3
  /** 至少一个工具在跑的时长（各请求窗口里工具段的并集，不含等人） */
  toolBusyMs: number
  /** 等人的区间（问用户的工具调用 → 结果；dsh 的审批 asked → decided），合并过 */
  waits: { start: number; end: number }[]
  /** 请求窗口的开销里 ≥ 2 分钟没有任何事件的空档 */
  idleGaps: { start: number; end: number }[]
  /** 有 dsh 的流式块时间（能拆思考 / 回答 / 工具参数） */
  streamTiming: boolean
}

// ---- 分类 ----

const SKILL_RE = /<skill_content\s+name="([^"]+)"/
const SKILL_TOOLS = new Set(['skill', 'Skill'])
/** 向用户提问的工具：调用 → 结果之间是在等人 */
const ASK_TOOLS = new Set(['AskUserQuestion', 'ask_user_question', 'request_user_input'])

function obj(v: unknown): Record<string, unknown> | null {
  return v && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, unknown>) : null
}
function str(v: unknown): string | undefined {
  return typeof v === 'string' && v ? v : undefined
}

/** 只压缩开头一段：工具输出可能有几 MB，整段 replace 再截掉太浪费 */
function oneLine(s: string, max = 120): string {
  const head = s.length > max * 4 ? s.slice(0, max * 4) : s
  const line = head.replace(/\s+/g, ' ').trim()
  return head === s || line.length > max ? truncate(line, max) : `${line}…`
}

/** dsh 注入消息的来源：指令文件路径 → 插件 → kind */
function dshSourceName(src: Record<string, unknown> | null): string | undefined {
  if (!src) return undefined
  const changes = Array.isArray(src.changes) ? src.changes : []
  const paths = changes.map((c) => str(obj(c)?.path)).filter(Boolean)
  if (paths.length) return paths.join(', ')
  return str(src.name) ?? str(src.plugin) ?? str(src.kind)
}

/** Claude Code 的 attachment：类型 → 进上下文的文本 */
function claudeAttachmentText(a: Record<string, unknown>, ext: Record<string, unknown>): string {
  const rendered = ext.rendered
  if (Array.isArray(rendered)) {
    const t = rendered.map((r) => str(obj(r)?.content) ?? '').join('\n')
    if (t) return t
  }
  if (typeof a.text === 'string') return a.text
  if (typeof a.content === 'string') return a.content
  if (Array.isArray(a.files)) return a.files.map((f) => str(obj(f)?.content) ?? '').join('\n')
  if (Array.isArray(a.addedLines)) return a.addedLines.join('\n')
  if (Array.isArray(a.addedBlocks)) return a.addedBlocks.join('\n')
  return json(a)
}

/** 不进上下文的 Claude Code attachment（只是给 UI / 记账的） */
const CLAUDE_SILENT = new Set([
  'prompt_snapshot',
  'deferred_tools_record',
  'queued_command',
  'command_permissions',
  'credential_org',
  'auto_mode',
  'edited_text_file',
  'hook_success',
  'hook_non_blocking_error',
  'async_hook_response',
])

/** 文件名在 Claude Code instructions attachment 里 */
function claudeAttachmentName(a: Record<string, unknown>): string | undefined {
  if (Array.isArray(a.files)) {
    const paths = a.files.map((f) => str(obj(f)?.path)).filter(Boolean) as string[]
    if (paths.length) return paths.map((p) => p.split(/[\\/]/).slice(-2).join('/')).join(', ')
  }
  return str(a.type)
}

function isDeepseek(model: string | null | undefined): boolean {
  return !!model && /deepseek/i.test(model)
}

/** 图片的 token 合计，顺手记到每张图上（浏览器里显示） */
function imageTokens(images: ImageInfo[], model: string | null): number {
  let sum = 0
  for (const im of images) sum += im.tokens = estimateImage(im.width, im.height, isDeepseek(model))
  return sum
}

// ---- 折叠 ----

export function foldContext(events: Event[]): CtxFold {
  const nodes: CtxNode[] = []
  const live = new Set<CtxNode>()
  /** dsh 行号 → 节点：一行 assistant/message 会拆成回复和几个工具调用，同一个行号有多个节点 */
  const byRaw = new Map<number, CtxNode[]>()
  const systems: CtxNode[] = []
  const toolEpochs: { seq: number; tools: CtxNode[] }[] = []
  const steps: StepRec[] = []
  const turns: Turn[] = []
  const evs: CtxEvent[] = []
  const byTool: Record<string, ToolTime> = {}
  const calls = new Map<string, { call?: Event; result?: Event }>()
  const stepOfCall = new Map<string, StepRec>()
  /** step_completed 的时间（按 step 序号） */
  const stepDoneTs: (string | undefined)[] = []
  /** provider 报的 reasoning tokens（按 step 序号）；没报是 undefined */
  const reportedReasoning: (number | undefined)[] = []
  /** 每一步发起的工具：调用 → 结果（epoch ms） */
  const toolRuns: { start: number; end: number }[][] = []
  /** 所有事件的时间：请求窗口里的空档按它找 */
  const beats: number[] = []
  /** 等人的区间：问用户的工具、dsh 的审批 */
  const waitList: { start: number; end: number }[] = []
  /** dsh 审批：id → asked 的时间；到最后都没 decided 的按所在请求的结束算 */
  const approvals = new Map<string, number>()
  /** 每一步的第一条回复（dsh 的流式时间在它上面） */
  const firstEv: Event[] = []

  const sums = zeroParts()
  const counts: Record<Cat, number> = zeroParts()
  /** 消息面上 assistant 的三分合计 */
  const asum = zeroAsst()
  let system: CtxNode | null = null
  let tools: CtxNode[] = []
  let window: number | null = null
  let maxOutput: number | null = null
  let model: string | null = null
  let humanInputs = 0
  let answers = 0
  let sawSystem = false
  let sawTools = false
  /** 压缩事件在等它后面那条替换上来的摘要，好算净腾出多少 */
  let pendingCompaction: { ev: CtxEvent; removed: number } | null = null
  /**
   * dsh 的压缩先报被遮蔽的行号，等带 surfaceOp replace 的摘要到了才真正移走，摘要插在第一条被移走的位置。
   * 下一次请求前（或者下一条不是替换的消息进来时）还没等到就直接移走
   */
  let pendingShadow: { seqs: number[]; by: number } | null = null
  /** dsh 这一步的 assistant/message 行号：后面的 tool/call 节点跟它一起被遮蔽 */
  let asstRaw: { key: string | null; raw: number | undefined } | null = null
  /** 下一个进上下文的节点（seq 是这个）排在 pos 上 */
  let placeNext: { seq: number; pos: number } | null = null

  for (const ev of events) {
    if (!ev.tool) continue
    const slot = calls.get(ev.tool.call_id) ?? {}
    if (ev.kind === 'tool_call' && !slot.call) slot.call = ev
    else if (ev.kind === 'tool_result' && !slot.result) slot.result = ev
    calls.set(ev.tool.call_id, slot)
  }

  const stepIdx = () => steps.length

  const add = (n: CtxNode) => {
    if (placeNext && placeNext.seq === n.seq) {
      n.pos = placeNext.pos
      placeNext = null
    } else if (pendingShadow) flushShadow()
    nodes.push(n)
    live.add(n)
    sums[n.cat] += n.est
    counts[n.cat]++
    if (n.asst) addAsst(asum, n.asst)
    if (n.rawSeq != null) {
      const list = byRaw.get(n.rawSeq)
      if (list) list.push(n)
      else byRaw.set(n.rawSeq, [n])
    }
    if (pendingCompaction && n.cat !== 'assistant') {
      pendingCompaction.ev.tokens = Math.max(0, pendingCompaction.removed - n.est)
      pendingCompaction = null
    }
  }
  const remove = (n: CtxNode, by: number): number => {
    if (!live.has(n)) return 0
    live.delete(n)
    n.gone = by
    sums[n.cat] -= n.est
    counts[n.cat]--
    if (n.asst) addAsst(asum, n.asst, -1)
    return n.est
  }
  /** 按 dsh 行号移走；返回移走的 token、条数和消息面上最靠前的那条的位置 */
  const removeRaw = (raws: Iterable<number>, by: number): { removed: number; count: number; first: number | null } => {
    let removed = 0
    let count = 0
    let first: number | null = null
    for (const rs of raws)
      for (const n of byRaw.get(rs) ?? []) {
        if (!live.has(n)) continue
        removed += remove(n, by)
        count++
        const at = n.pos ?? n.seq
        if (first == null || at < first) first = at
      }
    return { removed, count, first }
  }
  /** 没等到替换的摘要：被遮蔽的直接移走 */
  const flushShadow = () => {
    if (!pendingShadow) return
    const p = pendingShadow
    pendingShadow = null
    removeRaw(p.seqs, p.by)
  }
  /** 整面换掉的压缩（不是 dsh 那种逐条遮蔽）：日志里没记压缩后保留了哪些，之后的快照是近似的 */
  let rebuilt = false
  const clearSurface = (by: number): { removed: number; count: number } => {
    rebuilt = true
    let removed = 0
    let count = 0
    for (const n of [...live]) {
      removed += remove(n, by)
      count++
    }
    return { removed, count }
  }

  const setSystem = (ev: Event, text: string, name?: string) => {
    sawSystem = true
    if (!text.trim()) return
    if (system && system.body() === text) return
    if (system) sums.system -= system.est
    system = {
      key: 'sys',
      seq: ev.seq,
      cat: 'system',
      est: estimateText(text) + 4,
      label: oneLine(text, 80),
      name,
      body: () => text,
      bodyKind: 'markdown',
      ev,
      step: stepIdx(),
    }
    systems.push(system)
    sums.system += system.est
    counts.system = 1
  }

  /** 上一版工具列表的原文：dsh / pfh / Claude Code 每次请求都重报一遍，没变就不重建 */
  let toolRaws: string[] = []
  const setTools = (ev: Event, list: unknown[]) => {
    // 只有名字的列表（pfh 的 toolNames）没有 schema，不算「工具记全了」
    if (list.some((t) => obj(t))) sawTools = true
    const raws = list.map(json)
    if (raws.length === toolRaws.length && raws.every((r, i) => r === toolRaws[i])) return
    toolRaws = raws
    tools = list.map((t, i): CtxNode => {
      const o = obj(t)
      const name = str(o?.name) ?? (typeof t === 'string' ? t : '?')
      return {
        key: `tool:${name}`,
        seq: ev.seq,
        cat: 'tools',
        est: estimateText(raws[i]) + 4,
        label: oneLine(str(o?.description) ?? '', 100),
        name,
        plugin: str(o?.plugin),
        body: () => pretty(t),
        bodyKind: 'json',
        ev,
        step: stepIdx(),
      }
    })
    toolEpochs.push({ seq: ev.seq, tools })
    sums.tools = tools.reduce((a, n) => a + n.est, 0)
    counts.tools = tools.length
  }

  /** 这一轮已经从 turn_started 记下的提问 */
  let turnPrompt: { turn: string | undefined; text: string } | null = null
  let lastMode: string | null = null

  const addUser = (ev: Event, text: string) => {
    const images = imagesIn(ev.ext)
    if (!text && !images.length) return
    humanInputs++
    add({
      key: `n${ev.seq}`,
      seq: ev.seq,
      cat: 'user',
      est: block(text) + imageTokens(images, model) + 4,
      label: oneLine(text) || `[${images.length} image]`,
      images: images.length ? images : undefined,
      body: () => text,
      bodyKind: 'markdown',
      ev,
      step: stepIdx(),
      rawSeq: rawSeqOf(ev),
    })
    breakLoose()
  }

  const pushEvent = (e: Omit<CtxEvent, 'step'>) => evs.push({ ...e, step: stepIdx() })

  const inject = (ev: Event, text: string, name: string | undefined, form: string, cat: Cat = 'inject') => {
    if (!text.trim()) return
    const n: CtxNode = {
      key: `n${ev.seq}`,
      seq: ev.seq,
      cat,
      est: block(text) + 4,
      label: oneLine(text),
      name,
      body: () => text,
      bodyKind: 'markdown',
      ev,
      step: stepIdx(),
      rawSeq: rawSeqOf(ev),
    }
    add(n)
    pushEvent({ kind: 'inject', seq: ev.seq, ts: ev.ts, tokens: n.est, name: name ?? form, form: cat === 'skill' ? 'skill' : form, node: n })
  }

  // ---- step 记录 ----
  let cur: StepRec | null = null
  let curKey: string | null = null
  /** 没有 step 编号的 harness：回复连着的算一步，中间来了别的消息就是下一步 */
  let looseRun = 0
  let lastTs: string | undefined
  let stepStartTs: string | undefined

  const tokensOf = (u: Usage): Tokens => {
    const n = (x: unknown) => (typeof x === 'number' ? x : 0)
    return {
      input: n(u.input),
      output: n(u.output),
      cache_read: n(u.cache_read),
      cache_write: n(u.cache_write),
      cache_write_1h: n(u.cache_write_1h),
      reasoning: n(u.reasoning),
    }
  }

  const openStep = (ev: Event) => {
    const key = ev.step?.id ?? `~${ev.turn?.id ?? ''}:${looseRun}`
    if (cur && curKey === key) return cur
    flushShadow()
    const rec: StepRec = {
      idx: steps.length,
      seq: ev.seq,
      endSeq: ev.seq,
      ts: ev.ts,
      endTs: ev.ts,
      startTs: stepStartTs ?? lastTs,
      turnId: ev.turn?.id ?? null,
      turnN: ev.turn?.n ?? null,
      model: ev.model ?? model,
      parts: { ...sums },
      counts: { ...counts },
      usage: null,
      prompt: null,
      anchored: zeroParts(),
      approximate: rebuilt,
      modelMs: 0,
      toolMs: 0,
      asst: { ...asum },
      gen: zeroAsst(),
      wallMs: 0,
      activeMs: 0,
      spans: [],
    }
    stepStartTs = undefined
    steps.push(rec)
    firstEv.push(ev)
    cur = rec
    curKey = key
    const tid = rec.turnId
    if (tid != null) {
      const t = turns[turns.length - 1]
      if (t && t.id === tid) t.last = rec.idx
      else turns.push({ id: tid, n: rec.turnN, first: rec.idx, last: rec.idx, ts: rec.ts, wallMs: 0 })
    }
    return rec
  }

  /** 回复事件落到当前 step 上：更新结束时间、模型 */
  const touchStep = (rec: StepRec, ev: Event) => {
    rec.endSeq = ev.seq
    if (ev.ts) rec.endTs = ev.ts
    if (ev.model && !rec.model) rec.model = ev.model
  }

  /** 一条非回复的消息进了上下文：下一条回复就是新的一步（没有 step 编号时） */
  const breakLoose = () => {
    if (cur && curKey?.startsWith('~')) {
      looseRun++
      cur = null
      curKey = null
    }
  }

  /** 模型换了记一条事件 */
  const switchModel = (ev: Event, m: string | undefined) => {
    if (!m) return
    if (model && m !== model) pushEvent({ kind: 'model', seq: ev.seq, ts: ev.ts, tokens: 0, name: `${model} → ${m}` })
    model = m
  }

  for (const ev of events) {
    const ext = ev.ext as Record<string, unknown>
    const ty = str(ext.type) ?? ''
    const data = obj(ext.data)

    // dsh 的 surfaceOp replace：压缩预告过被遮蔽的行号就移走那些，否则按区间换掉；
    // 这条消息本身（摘要）排在第一条被移走的位置
    const op = obj(ext.surfaceOp)
    if (op && op.op === 'replace') {
      let first: number | null = null
      if (pendingShadow) {
        const p = pendingShadow
        pendingShadow = null
        first = removeRaw(p.seqs, p.by).first
      } else if (!pendingCompaction) {
        const a = Number(op.startSeq)
        const b = Number(op.endSeq)
        if (Number.isFinite(a) && Number.isFinite(b)) {
          const range: number[] = []
          for (let rs = a; rs <= b; rs++) range.push(rs)
          first = removeRaw(range, ev.seq).first
        }
      }
      if (first != null) placeNext = { seq: ev.seq, pos: first }
    }

    switch (ev.kind) {
      case 'step_started':
        stepStartTs = ev.ts
        break
      case 'step_completed': {
        // dsh / pfh 的 step 结束在工具跑完之后：请求窗口到这里
        const rec = steps[steps.length - 1]
        if (rec && ev.ts && (ev.step?.id == null || ev.step.id === curKey)) stepDoneTs[rec.idx] = ev.ts
        break
      }
      case 'turn_started':
        // Claude Code 的提问在 turn_started 上；pfh / dsh 另记一条同文的 user 消息，到时跳过
        if (ev.text) {
          addUser(ev, ev.text)
          turnPrompt = { turn: ev.turn?.id, text: ev.text }
        }
        break
      case 'turn_completed': {
        const t = turns[turns.length - 1]
        if (t && (ev.turn?.id == null || t.id === ev.turn.id) && ev.duration_ms) t.wallMs = ev.duration_ms
        break
      }
      case 'tool_call': {
        const tool = ev.tool!
        const rec = openStep(ev)
        touchStep(rec, ev)
        stepOfCall.set(tool.call_id, rec)
        const args = json(tool.input)
        const est = estimateText(tool.name) + block(args)
        add({
          key: `n${ev.seq}`,
          seq: ev.seq,
          cat: 'assistant',
          est,
          label: tool.name,
          name: tool.name,
          kinds: ['tools'],
          asst: { reasoning: 0, answer: 0, call: est },
          body: () => pretty(tool.input),
          bodyKind: typeof tool.input === 'string' ? 'text' : 'json',
          ev,
          step: rec.idx,
          // dsh 的工具调用在 assistant/message 里，tool/call 只是另记一行：遮蔽、替换按那条回复的行号算
          rawSeq: (asstRaw && asstRaw.key === curKey ? asstRaw.raw : undefined) ?? rawSeqOf(ev),
        })
        break
      }
      case 'tool_result': {
        const tool = ev.tool!
        const slot = calls.get(tool.call_id)
        const call = slot?.call
        const name = tool.name || call?.tool?.name || ''
        const output = tool.output ?? dshResultContent(ev)
        const text = contentText(output)
        const images = imagesIn(output)
        const err = toolFailed(tool.status) || tailFailed(text)
        const m = SKILL_TOOLS.has(name) || !call ? SKILL_RE.exec(text) : null
        // dsh 的 skill 工具参数是 {name}，Claude Code 的 Skill 是 {skill}（口径同 harl-core 的 composition.rs）
        const skillArgs = obj(call?.tool?.input)
        const skill = SKILL_TOOLS.has(name) || m ? (m?.[1] ?? str(skillArgs?.name) ?? str(skillArgs?.skill) ?? name) : null
        const n: CtxNode = {
          key: `n${ev.seq}`,
          seq: ev.seq,
          cat: skill ? 'skill' : 'tool',
          est: block(text) + imageTokens(images, model) + 4,
          label: oneLine(text, 100),
          name: skill ?? name,
          err,
          images: images.length ? images : undefined,
          body: () => text,
          bodyKind: 'text',
          ev,
          step: stepIdx(),
          rawSeq: rawSeqOf(ev),
          call,
        }
        add(n)
        if (skill) pushEvent({ kind: 'inject', seq: ev.seq, ts: ev.ts, tokens: n.est, name: skill, form: 'skill', node: n })
        const tt = (byTool[name || '?'] ??= { calls: 0, failures: 0, ms: 0 })
        tt.calls++
        if (err) tt.failures++
        if (call?.ts && ev.ts) {
          const ms = Math.max(0, Date.parse(ev.ts) - Date.parse(call.ts))
          tt.ms += ms
          const owner = stepOfCall.get(tool.call_id)
          if (owner) {
            owner.toolMs += ms
            ;(toolRuns[owner.idx] ??= []).push({ start: Date.parse(call.ts), end: Date.parse(ev.ts) })
          }
        }
        if (ASK_TOOLS.has(name)) {
          humanInputs++
          if (call?.ts && ev.ts) waitList.push({ start: Date.parse(call.ts), end: Date.parse(ev.ts) })
        }
        breakLoose()
        break
      }
      case 'message': {
        if (ev.role === 'user') {
          if (ext.isCompactSummary) {
            inject(ev, ev.text ?? '', 'compaction', 'summary')
            break
          }
          if (turnPrompt && turnPrompt.turn === ev.turn?.id && turnPrompt.text === (ev.text ?? '')) {
            turnPrompt = null
            break
          }
          addUser(ev, ev.text ?? '')
        } else if (ev.role === 'assistant') {
          if (ev.provider === 'dsh') asstRaw = { key: ev.step?.id ?? null, raw: rawSeqOf(ev) }
          if (!ev.text && !ev.reasoning) {
            // 只带用量 / 签名的行也算这一步的回复
            if (ev.usage || ev.model) touchStep(openStep(ev), ev)
            break
          }
          const rec = openStep(ev)
          touchStep(rec, ev)
          if (ev.model) model = ev.model
          const kinds: AsstKind[] = []
          if (ev.reasoning) kinds.push('thinking')
          if (ev.text) {
            kinds.push('answer')
            answers++
          }
          const text = ev.text ?? ''
          const reasoning = ev.reasoning ?? ''
          // 每条 +4 的消息开销记在正文上；只有思考时记在思考上
          const r = block(reasoning) + (text ? 0 : 4)
          const a = block(text) + (text ? 4 : 0)
          add({
            key: `n${ev.seq}`,
            seq: ev.seq,
            cat: 'assistant',
            est: block(text) + block(reasoning) + 4,
            label: oneLine(text || reasoning),
            kinds,
            asst: { reasoning: r, answer: a, call: 0 },
            body: () => (reasoning ? `> ${reasoning.replace(/\n/g, '\n> ')}\n\n${text}` : text),
            bodyKind: 'markdown',
            ev,
            step: rec.idx,
            rawSeq: rawSeqOf(ev),
          })
        }
        break
      }
      case 'request_header':
      case 'meta': {
        handleHeader(ev, ty, ext, data)
        break
      }
    }

    // 压缩：signal 是跨 provider 的；dsh 带被遮蔽的行号，其他整面换掉
    if (ev.signal === 'compaction') {
      const shadowed = Array.isArray(data?.shadowedSeqs) ? (data!.shadowedSeqs as unknown[]).map(Number) : null
      let removed = 0
      let count = 0
      if (shadowed) {
        // 先只记下来，等替换上来的摘要（见上面的 surfaceOp）再移走
        flushShadow()
        for (const rs of shadowed)
          for (const n of byRaw.get(rs) ?? [])
            if (live.has(n)) {
              removed += n.est
              count++
            }
        pendingShadow = { seqs: shadowed, by: ev.seq }
      } else ({ removed, count } = clearSurface(ev.seq))
      const meta = obj(ext.compactMetadata)
      const e: CtxEvent = {
        kind: ty === 'compaction/prune' ? 'prune' : 'compaction',
        seq: ev.seq,
        ts: ev.ts,
        step: stepIdx(),
        tokens: typeof data?.shadowedTokenCount === 'number' ? (data.shadowedTokenCount as number) : removed,
        name: ty === 'compaction/prune' ? 'prune' : 'compaction',
        form: str(meta?.trigger),
        count,
        preTokens: typeof meta?.preTokens === 'number' ? (meta.preTokens as number) : undefined,
      }
      evs.push(e)
      pendingCompaction = { ev: e, removed: e.tokens }
      // Codex 的 compacted 行自带替换后的历史
      const payload = obj(ext.payload)
      const hist = payload?.replacement_history
      if (Array.isArray(hist) && hist.length) inject(ev, json(hist), 'compaction', 'summary')
      else if (str(payload?.message)) inject(ev, payload!.message as string, 'compaction', 'summary')
      breakLoose()
    }

    // usage 落到它所在的 step（Claude Code 同一请求的每行都带同一份，取最后一份；pfh 在 step_completed 上）
    if (ev.usage) {
      const rec: StepRec | null = cur ?? steps[steps.length - 1] ?? null
      if (rec && (ev.kind === 'step_completed' || ev.role === 'assistant' || !rec.usage)) {
        rec.usage = tokensOf(ev.usage)
        rec.prompt = promptTokens(rec.usage)
        const u = ev.usage
        const r = u.reasoning ?? u.reasoningTokens ?? u.reasoning_tokens
        reportedReasoning[rec.idx] = typeof r === 'number' ? r : undefined
      }
    }

    if (ev.ts) {
      lastTs = ev.ts
      const at = Date.parse(ev.ts)
      if (Number.isFinite(at)) beats.push(at)
    }
  }

  function handleHeader(ev: Event, ty: string, ext: Record<string, unknown>, data: Record<string, unknown> | null) {
    // ---- dsh ----
    if (ev.provider === 'dsh') {
      if (ty === 'request/header') {
        const header = obj(data?.header)
        const cfg = obj(header?.config)
        if (typeof header?.system === 'string') setSystem(ev, header.system)
        if (Array.isArray(header?.tools)) setTools(ev, header.tools)
        if (typeof cfg?.maxTokens === 'number') maxOutput = cfg.maxTokens
        switchModel(ev, str(cfg?.model))
        return
      }
      if (ty === 'request/context') {
        if (typeof data?.contextWindow === 'number') window = data.contextWindow
        return
      }
      if (ty === 'system/message') {
        setSystem(ev, ev.text ?? '', str(obj(obj(data?.message)?.source)?.plugin))
        return
      }
      if (ty === 'approval/asked' || ty === 'approval/decided') {
        const id = str(data?.id)
        const at = ev.ts ? Date.parse(ev.ts) : NaN
        if (!id || !Number.isFinite(at)) return
        if (ty === 'approval/asked') approvals.set(id, at)
        else {
          const from = approvals.get(id)
          approvals.delete(id)
          if (from != null) waitList.push({ start: from, end: at })
        }
        return
      }
      if (ty === 'plan/mode') {
        pushEvent({ kind: 'mode', seq: ev.seq, ts: ev.ts, tokens: 0, name: data?.active ? 'plan.on' : 'plan.off' })
        return
      }
      if (ty === 'user/message' || ty === 'developer/message') {
        const src = obj(data?.source) ?? obj(obj(data?.message)?.source)
        const kind = str(src?.kind)
        const skill = kind === 'skill-invocation' || kind === 'skill-catalog'
        inject(ev, ev.text ?? json(data?.content), dshSourceName(src), str(src?.form) ?? (skill ? 'skill' : kind ?? 'context'), skill ? 'skill' : 'inject')
        return
      }
      return
    }

    // ---- Claude Code ----
    if (ev.provider === 'claude-code') {
      if (ty === 'attachment') {
        const a = obj(ext.attachment)
        if (!a) return
        const at = str(a.type) ?? ''
        if (at === 'prompt_snapshot') {
          if (Array.isArray(a.systemPrompt)) setSystem(ev, (a.systemPrompt as unknown[]).map((x) => json(x)).join('\n\n'))
          if (Array.isArray(a.tools)) setTools(ev, a.tools)
          return
        }
        if (CLAUDE_SILENT.has(at)) return
        const text = claudeAttachmentText(a, ext)
        inject(ev, text, claudeAttachmentName(a), at, at === 'skill_listing' || at === 'invoked_skills' ? 'skill' : 'inject')
        return
      }
      if (ty === 'user' && ev.text && !ev.signal) {
        inject(ev, ev.text, str(obj(ext.origin)?.kind) ?? 'meta', 'meta')
        return
      }
      if (ty === 'permission-mode' && str(ext.permissionMode) && ext.permissionMode !== lastMode) {
        const from = lastMode
        lastMode = ext.permissionMode as string
        if (from == null) return
        pushEvent({ kind: 'mode', seq: ev.seq, ts: ev.ts, tokens: 0, name: ext.permissionMode as string })
      }
      return
    }

    // ---- Codex ----
    if (ev.provider === 'codex') {
      const p = obj(ext.payload)
      if (ty === 'turn_context') {
        switchModel(ev, str(p?.model))
        return
      }
      if (ev.kind === 'request_header' && ev.text) {
        const role = str(p?.role)
        const name = /^#\s*AGENTS\.md/.test(ev.text) ? 'AGENTS.md' : /^<(\w+)/.exec(ev.text.trim())?.[1] ?? role
        inject(ev, ev.text, name, role ?? 'context')
      }
      return
    }

    // ---- pfh ----
    if (ev.provider === 'pfh') {
      if (ty === 'request_header') {
        const sections = Array.isArray(ext.systemSections) ? (ext.systemSections as unknown[]) : []
        if (sections.length) setSystem(ev, sections.map((s) => str(obj(s)?.content) ?? '').join('\n\n'))
        if (Array.isArray(ext.toolNames)) setTools(ev, ext.toolNames as unknown[])
        switchModel(ev, str(ext.modelId))
        return
      }
      if (ty === 'context_queued' && str(ext.content)) inject(ev, ext.content as string, str(ext.producer), 'context')
      return
    }

    // 其他：带正文的 request_header 当注入
    if (ev.kind === 'request_header' && ev.text) inject(ev, ev.text, ty || undefined, 'context')
  }

  // ---- 收尾：校准、时间 ----
  const complete = sawSystem && sawTools
  for (const s of steps) {
    s.anchored = anchor(s.parts, s.prompt, complete)
    if (s.startTs && s.endTs) s.modelMs = Math.max(0, Date.parse(s.endTs) - Date.parse(s.startTs))
  }
  // 压缩腾出多少：前后两次请求都有 provider 报的 prompt 就用实际差值，比估算准
  for (const e of evs) {
    if (e.kind !== 'compaction' && e.kind !== 'prune') continue
    const before = steps[e.step - 1]?.prompt
    const after = steps[e.step]?.prompt
    if (before != null && after != null) e.tokens = Math.max(0, before - after)
  }
  for (const t of turns) {
    const a = steps[t.first]
    const b = steps[t.last]
    t.ts = a?.startTs ?? a?.ts
    t.endTs = b?.endTs
    if (!t.wallMs && t.ts && t.endTs) t.wallMs = Math.max(0, Date.parse(t.endTs) - Date.parse(t.ts))
  }
  // dsh：v3 把流式 chunk 折进 assistant/message 的 stream，首 token 时间、各块的开始时间在里面
  const firstTok: (number | undefined)[] = []
  const blockSpans: Span[][] = []
  let streamTiming = false
  for (const s of steps) {
    const ev = firstEv[s.idx]
    const stream = obj(ev?.ext.data)?.stream
    if (!Array.isArray(stream) || !s.startTs) continue
    const st = streamBlocks(stream, ev?.ts ? Date.parse(ev.ts) : NaN)
    if (st.first == null) continue
    s.ttftMs = Math.max(0, st.first - Date.parse(s.startTs))
    firstTok[s.idx] = st.first
    if (st.blocks.length) {
      streamTiming = true
      blockSpans[s.idx] = st.blocks
    }
  }

  // 生成的三分：这一步自己的输出节点估算；provider 报了 reasoning tokens 就用它（不超过 output）
  const generated = zeroAsst()
  for (const n of nodes) if (n.asst && steps[n.step]) addAsst(steps[n.step]!.gen, n.asst)
  for (const s of steps) {
    const r = reportedReasoning[s.idx]
    if (r != null) s.gen.reasoning = s.usage ? Math.min(r, s.usage.output) : r
    addAsst(generated, s.gen)
  }

  // 压缩的跨度：前一次请求 → 后一次请求
  const posOf = stepPosLookup(steps, turns)
  for (const e of evs) {
    if (e.kind !== 'compaction' && e.kind !== 'prune') continue
    e.span = { from: posOf(e.step - 1), to: posOf(e.step) }
  }

  // ---- 时间：每次请求的窗口拆成各段 ----
  beats.sort((a, b) => a - b)
  const windows = steps.map((s) => stepWindow(s, stepDoneTs[s.idx], toolRuns[s.idx]))
  // 到最后都没 decided 的审批：按所在请求的结束算
  for (const at of approvals.values()) {
    const w = windows.find((x) => x && at >= x.start && at < x.end)
    if (w) waitList.push({ start: at, end: w.end })
  }
  const waits = mergeIntervals(waitList)
  const idleGaps: { start: number; end: number }[] = []
  let toolBusyMs = 0
  for (const s of steps) {
    const w = windows[s.idx]
    if (!w) continue
    s.wallMs = w.end - w.start
    const raw: Span[] = []
    const first = firstTok[s.idx]
    const replyAt = s.endTs ? Date.parse(s.endTs) : w.start
    if (first != null && first >= w.start) {
      raw.push({ kind: 'ttft', start: w.start, end: first })
      const bl = blockSpans[s.idx]
      if (bl) raw.push(...bl)
      else raw.push({ kind: 'model', start: first, end: replyAt })
    } else raw.push({ kind: 'model', start: w.start, end: replyAt })
    for (const r of toolRuns[s.idx] ?? []) raw.push({ kind: 'tools', start: r.start, end: r.end })
    const clipped = raw
      .map((r) => ({ kind: r.kind, start: Math.max(w.start, r.start), end: Math.min(w.end, r.end) }))
      .filter((r) => r.end > r.start)
      .sort((a, b) => a.start - b.start || a.end - b.end)
    // 等人：落在窗口里的部分
    const myWaits: Span[] = clipIntervals(waits, w.start, w.end).map((x) => ({ kind: 'wait', ...x }))
    s.activeMs = s.wallMs - myWaits.reduce((a, x) => a + x.end - x.start, 0)
    // 开销里的空档：回复落盘之后 ≥ 2 分钟没有事件；和等人重叠的部分记成 wait
    const msgAt = s.ts ? Date.parse(s.ts) : w.start
    const gapCuts: Span[] = []
    for (const g of beatGaps(beats, w.start, w.end, Math.max(w.start, msgAt))) carve(gapCuts, 'idle', g.start, g.end, myWaits)
    const overheadCuts = [...gapCuts.filter((x) => x.kind === 'idle'), ...myWaits].sort((a, b) => a.start - b.start)
    const spans: Span[] = []
    let y = w.start
    for (const r of clipped) {
      const t = Math.max(y, r.start)
      if (r.end <= t) continue
      carve(spans, 'overhead', y, t, overheadCuts)
      carve(spans, r.kind, t, r.end, myWaits)
      y = r.end
    }
    carve(spans, 'overhead', y, w.end, overheadCuts)
    s.spans = spans
    for (const x of spans) {
      if (x.kind === 'tools') toolBusyMs += x.end - x.start
      else if (x.kind === 'idle') idleGaps.push({ start: x.start, end: x.end })
    }
    // 块的耗时与块数（块数按流里的块算，被等人切开也只算一块）
    const bl = blockSpans[s.idx]
    if (bl) {
      const blocks: Record<BlockKind, { ms: number; n: number }> = {
        reasoning: { ms: 0, n: 0 },
        text: { ms: 0, n: 0 },
        toolArgs: { ms: 0, n: 0 },
      }
      for (const b of bl) blocks[b.kind as BlockKind].n++
      for (const x of spans) if (x.kind === 'reasoning' || x.kind === 'text' || x.kind === 'toolArgs') blocks[x.kind].ms += x.end - x.start
      s.blocks = blocks
    }
  }

  return {
    nodes,
    systems,
    toolEpochs,
    steps,
    turns,
    events: evs,
    window,
    maxOutput,
    complete,
    humanInputs,
    answers,
    byTool,
    calls,
    generated,
    toolBusyMs,
    waits,
    idleGaps: mergeIntervals(idleGaps),
    streamTiming,
  }
}

// ---- 时间的小工具 ----

/**
 * dsh 的流：block-start 标出每块（思考 / 正文 / 工具参数）从哪一刻开始，块一直到下一块开始或回复落盘。
 * first 是第一个带时间的 chunk（首 token）；第一块从首 token 起算
 */
function streamBlocks(stream: unknown[], endAt: number): { first?: number; blocks: Span[] } {
  let first: number | undefined
  const starts: { kind: BlockKind; at: number }[] = []
  for (const c of stream) {
    const o = obj(c)
    if (!o) continue
    const t = typeof o.time === 'number' ? o.time : typeof o.time0 === 'number' ? o.time0 : null
    if (t == null) continue
    first ??= t
    const ch = obj(o.chunk)
    if (o.type !== 'chunk' || ch?.type !== 'block-start') continue
    const kind = ch.blockType === 'reasoning' ? 'reasoning' : ch.blockType === 'text' ? 'text' : ch.blockType === 'tool-call' ? 'toolArgs' : null
    if (kind) starts.push({ kind, at: t })
  }
  const blocks: Span[] = []
  if (first == null || !Number.isFinite(endAt)) return { first, blocks }
  let prevEnd: number | null = null
  for (let i = 0; i < starts.length; i++) {
    const b = starts[i]!
    const end = Math.min(endAt, starts[i + 1]?.at ?? endAt)
    if (end <= b.at || end <= first) continue
    const start = prevEnd == null ? first : Math.max(prevEnd, b.at)
    if (end <= start) continue
    blocks.push({ kind: b.kind, start, end })
    prevEnd = end
  }
  return { first, blocks }
}

/**
 * 请求的窗口：开始 → 最后一条回复、step_completed、它发起的工具最后一个结果里最晚的那个
 * （Codex 的 step 结束是按用量上报推的，工具还没跑完就结束了，所以工具结果也要算进来）
 */
function stepWindow(s: StepRec, done: string | undefined, runs: { start: number; end: number }[] | undefined): { start: number; end: number } | null {
  if (!s.startTs) return null
  const start = Date.parse(s.startTs)
  let end = s.endTs ? Date.parse(s.endTs) : start
  if (done) end = Math.max(end, Date.parse(done))
  for (const r of runs ?? []) end = Math.max(end, r.end)
  return Number.isFinite(start) && Number.isFinite(end) ? { start, end: Math.max(start, end) } : null
}

/** 排序、合并重叠的区间 */
function mergeIntervals(list: { start: number; end: number }[]): { start: number; end: number }[] {
  const sorted = list
    .filter((x) => Number.isFinite(x.start) && Number.isFinite(x.end) && x.end > x.start)
    .sort((a, b) => a.start - b.start || a.end - b.end)
  const out: { start: number; end: number }[] = []
  for (const x of sorted) {
    const last = out[out.length - 1]
    if (last && x.start <= last.end) last.end = Math.max(last.end, x.end)
    else out.push({ ...x })
  }
  return out
}

/** 合并好的区间落在 [a, b) 里的部分 */
function clipIntervals(list: { start: number; end: number }[], a: number, b: number): { start: number; end: number }[] {
  const out: { start: number; end: number }[] = []
  for (const x of list) {
    if (x.end <= a) continue
    if (x.start >= b) break
    out.push({ start: Math.max(a, x.start), end: Math.min(b, x.end) })
  }
  return out
}

/** 窗口里相邻两个事件之间 ≥ 2 分钟的空档；from 之前（模型还在生成）的不算 */
const IDLE_GAP_MS = 120_000
function beatGaps(beats: number[], start: number, end: number, from: number): { start: number; end: number }[] {
  const out: { start: number; end: number }[] = []
  const push = (a: number, b: number) => {
    const x = Math.max(a, from)
    if (b - x >= IDLE_GAP_MS) out.push({ start: x, end: b })
  }
  let lo = 0
  let hi = beats.length
  while (lo < hi) {
    const mid = (lo + hi) >> 1
    if (beats[mid]! <= start) lo = mid + 1
    else hi = mid
  }
  let prev = start
  for (let i = lo; i < beats.length && beats[i]! < end; i++) {
    push(prev, beats[i]!)
    prev = beats[i]!
  }
  push(prev, end)
  return out
}

/** [a, b) 按 cuts（已排序、不重叠）切开：落在 cut 里的记成 cut 的 kind，其余记 kind */
function carve(out: Span[], kind: SpanKind, a: number, b: number, cuts: Span[]) {
  if (!(b > a)) return
  let x = a
  for (const c of cuts) {
    if (c.end <= x) continue
    if (c.start >= b) break
    const s = Math.max(x, c.start)
    if (s > x) out.push({ kind, start: x, end: s })
    const e = Math.min(b, c.end)
    out.push({ kind: c.kind, start: s, end: e })
    x = e
    if (x >= b) return
  }
  if (b > x) out.push({ kind, start: x, end: b })
}

/** 请求序号 → 第几轮的第几次请求 */
function stepPosLookup(steps: StepRec[], turns: Turn[]): (idx: number) => StepPos | null {
  const firstOf: number[] = []
  for (const t of turns) for (let i = t.first; i <= t.last; i++) firstOf[i] = t.first
  return (idx) => {
    const s = steps[idx]
    if (!s) return null
    return { idx, turn: s.turnN, step: s.idx - (firstOf[idx] ?? s.idx) + 1 }
  }
}

/** 内容的文本：字符串原样，part 数组拼 text、图片跳过（另按尺寸计），其余 JSON */
function contentText(v: unknown): string {
  if (!Array.isArray(v)) return outputText(v)
  return v
    .map((p) => {
      const o = obj(p)
      if (o?.type === 'image' || o?.type === 'input_image') return ''
      if (typeof o?.text === 'string') return o.text
      return json(p)
    })
    .filter(Boolean)
    .join('\n')
}

/**
 * dsh 新版的 tool/result 不再包一层 tool-result part，结果的 text / image part 直接放在 message.content 里，
 * normalizer 没把它搬进 tool.output：从 ext 里取回来
 */
function dshResultContent(ev: Event): unknown {
  if (ev.provider !== 'dsh') return undefined
  const content = obj(obj((ev.ext as Record<string, unknown>).data)?.message)?.content
  return Array.isArray(content) && content.length ? content : undefined
}

/** dsh 原始行号（surfaceOp、shadowedSeqs 用的是它，不是 harl 的行号） */
function rawSeqOf(ev: Event): number | undefined {
  if (ev.provider !== 'dsh') return undefined
  const s = (ev.ext as Record<string, unknown>).seq
  return typeof s === 'number' ? s : undefined
}

/** 结果文本末尾的退出标记：`[exit code: 1]`、`[shell killed by signal …]` */
function tailFailed(text: string): boolean {
  const tail = text.slice(-200)
  const m = /\[(?:exit code|shell exited: code):?\s*(-?\d+)\]\s*$/.exec(tail)
  if (m) return m[1] !== '0'
  return /\[(?:shell )?killed by signal[^\]]*\]\s*$|\[status: (?:killed|failed)[^\]]*\]\s*$/.test(tail)
}

/**
 * 估算的组成按 provider 报的 prompt tokens 校准。
 * complete：system 与工具都在日志里，整体按比例缩放；否则多出来的单列 other（日志没记到的 system / 工具 / 框架开销）
 */
function anchor(parts: Parts, prompt: number | null, complete: boolean): Parts {
  const est = sumParts(parts) - parts.other
  if (prompt == null || prompt <= 0 || est <= 0) return { ...parts }
  const out = zeroParts()
  if (complete || prompt < est) {
    const k = prompt / est
    for (const c of CATS) out[c] = Math.round(parts[c] * k)
    // 取整的零头给最大的一类
    const diff = prompt - sumParts(out)
    const top = CATS.reduce((a, c) => (out[c] > out[a] ? c : a), CATS[0]!)
    out[top] += diff
    return out
  }
  for (const c of CATS) out[c] = parts[c]
  out.other = prompt - est
  return out
}

// ---- 某一步的上下文 ----

export interface Assembled {
  step: StepRec | null
  system: CtxNode | null
  tools: CtxNode[]
  /** 消息面，按进入顺序 */
  surface: CtxNode[]
}

/** 第 idx 次请求时的上下文；idx = steps.length 是"下一次请求"（当前上下文） */
export function assemble(fold: CtxFold, idx: number): Assembled {
  const step = fold.steps[idx] ?? null
  const at = step ? step.seq : Number.POSITIVE_INFINITY
  let system: CtxNode | null = null
  for (const s of fold.systems) if (s.seq < at) system = s
  let tools: CtxNode[] = []
  for (const e of fold.toolEpochs) if (e.seq < at) tools = e.tools
  const surface = fold.nodes.slice(0, seqBound(fold.nodes, at)).filter((n) => n.gone == null || n.gone > at)
  // 压缩摘要排在它替换掉的那段的位置（排在同一 seq 的节点之前）
  if (surface.some((n) => n.pos != null)) surface.sort((a, b) => (a.pos != null ? a.pos - 0.5 : a.seq) - (b.pos != null ? b.pos - 0.5 : b.seq))
  return { step, system, tools, surface }
}

export function itemsByCat(a: Assembled): Record<Cat, CtxNode[]> {
  const out = Object.fromEntries(ALL_CATS.map((c) => [c, [] as CtxNode[]])) as unknown as Record<Cat, CtxNode[]>
  if (a.system) out.system.push(a.system)
  out.tools.push(...a.tools)
  for (const n of a.surface) out[n.cat].push(n)
  return out
}

/** 每类的估算合计 */
export function estOf(byCat: Record<Cat, CtxNode[]>): Parts {
  const out = zeroParts()
  for (const c of ALL_CATS) for (const n of byCat[c]) out[c] += n.est
  return out
}

/** 最后一次有 provider 报 prompt 的请求 */
export function lastWithPrompt(steps: StepRec[]): StepRec | undefined {
  for (let i = steps.length - 1; i >= 0; i--) if (steps[i]!.prompt != null) return steps[i]
  return undefined
}

/** 当前上下文（最后一次请求之后又进来的也算上）：估算的组成，用最后一次请求的 prompt 校准 */
export function currentParts(fold: CtxFold): { parts: Parts; tokens: number; est: boolean } {
  const a = assemble(fold, fold.steps.length)
  const parts = estOf(itemsByCat(a))
  const last = lastWithPrompt(fold.steps)
  if (last?.prompt == null) return { parts, tokens: sumParts(parts), est: true }
  // 最后一次请求的实际 prompt + 之后新增的估算
  const grown = sumParts(parts) - sumParts(last.parts)
  const tokens = Math.max(0, last.prompt + Math.max(0, grown))
  return { parts: anchor(parts, tokens, fold.complete), tokens, est: false }
}

/** 一轮的最后一次请求代表这一轮（turn 粒度的趋势） */
export function turnSteps(fold: CtxFold): { turn: Turn; step: StepRec; count: number; toolMs: number; modelMs: number }[] {
  return fold.turns
    .map((t) => {
      const step = fold.steps[t.last]
      if (!step) return null
      let toolMs = 0
      let modelMs = 0
      for (let i = t.first; i <= t.last; i++) {
        toolMs += fold.steps[i]?.toolMs ?? 0
        modelMs += fold.steps[i]?.modelMs ?? 0
      }
      return { turn: t, step, count: t.last - t.first + 1, toolMs, modelMs }
    })
    .filter((x): x is NonNullable<typeof x> => x != null)
}

// ---- 步骤简报：这一步是谁开的头、新进来了什么、回了什么 ----

export interface Brief {
  opener: CtxNode | null
  inputs: CtxNode[]
  response: CtxNode[]
}

/** 这次请求自己的输出（快照之后才进上下文） */
/** nodes 按 seq 排好序：第一个 seq ≥ seq 的下标 */
function seqBound(nodes: CtxNode[], seq: number): number {
  let lo = 0
  let hi = nodes.length
  while (lo < hi) {
    const mid = (lo + hi) >> 1
    if (nodes[mid]!.seq < seq) lo = mid + 1
    else hi = mid
  }
  return lo
}

/** seq 落在 [from, to) 的节点 */
function nodesIn(fold: CtxFold, from: number, to: number): CtxNode[] {
  return fold.nodes.slice(seqBound(fold.nodes, from), seqBound(fold.nodes, to))
}

export function outputsOf(fold: CtxFold, s: StepRec): CtxNode[] {
  return nodesIn(fold, s.seq, s.endSeq + 1).filter((n) => n.cat === 'assistant')
}

export function briefOf(fold: CtxFold, idx: number): Brief {
  const s = fold.steps[idx]
  if (!s) return { opener: null, inputs: [], response: [] }
  const prev = fold.steps[idx - 1]
  const response = outputsOf(fold, s)
  const turnStart = !prev || prev.turnId !== s.turnId
  let opener: CtxNode | null = null
  const firstOfTurn = fold.steps.find((x) => x.turnId === s.turnId) ?? s
  const prevTurnEnd = fold.steps[firstOfTurn.idx - 1]?.endSeq ?? -1
  for (const n of nodesIn(fold, prevTurnEnd + 1, firstOfTurn.seq)) if (n.cat === 'user') opener = n
  const inputs = turnStart ? [] : nodesIn(fold, prev.endSeq + 1, s.seq)
  return { opener, inputs, response }
}

// ---- 工具来源 ----

const FIRST_PARTY: Record<string, string> = {
  read: 'dsh-tool-fs',
  write: 'dsh-tool-fs',
  edit: 'dsh-tool-fs',
  read_image: 'dsh-tool-fs',
  grep: 'dsh-tool-fs-search',
  glob: 'dsh-tool-fs-search',
  run_code: 'dsh-tools',
  bash: 'dsh-tool-shell',
  pwsh: 'dsh-tool-shell',
  shell: 'dsh-tool-shell',
  web_fetch: 'dsh-tool-web',
  web_search: 'dsh-tool-web',
  todo_write: 'dsh-tools',
  skill: 'dsh-skills',
  ask_user_question: 'dsh-tools',
  subagent: 'dsh-subagent',
}

/** 工具从哪来：mcp__server__tool → mcp:server；schema 上显式写的 plugin；dsh 内置的映射到包名；其余是 harness 内置 */
export function toolSource(node: CtxNode): string | null {
  const name = node.name ?? ''
  const m = /^mcp__(.+)__[^_].*$/.exec(name)
  if (m) return `mcp:${m[1]!.replace(/__.*$/, '')}`
  if (name.startsWith('mcp_')) return 'mcp'
  if (node.plugin) return node.plugin
  return (node.ev.provider === 'dsh' ? FIRST_PARTY[name] : undefined) ?? 'built-in'
}
