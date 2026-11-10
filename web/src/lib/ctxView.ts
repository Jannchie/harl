// Portions derived from dsh-context (Apache-2.0, © 2025 bowenliang123), modified for harl. See THIRD_PARTY_NOTICES.md.
// 上下文视图各卡片共用的：类别配色与名字、偏好（localStorage）、会话级汇总（token / 费用 / 时间）、卡片之间的跳转意图。
import { reactive, ref, watch, type Ref } from 'vue'
import type { Tokens } from '../api'
import { t } from '../i18n'
import type { ChartTheme } from './chartTheme'
import {
  ALL_CATS,
  lastWithPrompt,
  sumParts,
  zeroParts,
  type Asst3,
  type BlockKind,
  type Cat,
  type CtxEventKind,
  type CtxFold,
  type Parts,
  type SpanKind,
  type ToolTime,
} from './context'
import type { OpKind } from './fileOps'
import { priceCard, priceTokens, type PriceCard } from './pricing'
import { addTokens, cacheHitRate, promptTokens, totalTokens, zeroTokens } from './summaryModel'

/** 类别固定配色：七类各占一个分类色槽，未记录用灰 */
export function catColor(c: Cat, theme: ChartTheme): string {
  if (c === 'other') return theme.other
  return theme.cat[ALL_CATS.indexOf(c)] ?? theme.other
}

export function catLabel(c: Cat): string {
  return t(`ctx.cat.${c}`)
}

/** 偏好记在 localStorage：读写都可能抛（隐私模式），抛了就只在内存里 */
export function usePref<T extends string | boolean>(key: string, initial: T): Ref<T> {
  const full = `harl:ctx:${key}`
  let v = initial
  try {
    const raw = localStorage.getItem(full)
    if (raw != null) v = JSON.parse(raw) as T
  } catch {
    // 用默认值
  }
  const r = ref(v) as Ref<T>
  watch(r, (x) => {
    try {
      localStorage.setItem(full, JSON.stringify(x))
    } catch {
      // 存不了就算了
    }
  })
  return r
}

/**
 * 卡片之间的跳转意图：概况卡里的计数、药丸点了发出来，ContextView 交给对应的卡。
 * - browser：上下文浏览器展开某一类（cat），可带过滤词（filter，按名字 / 摘要匹配）或要展开的节点（nodeId = CtxNode.key）
 * - files：文件活动按操作过滤（image = 图片文件）
 * - events：上下文事件按种类过滤
 * - commands：命令执行卡按程序名（program）或过滤条件（filter）
 * - fleet：Agent 网络
 */
export type CtxNav =
  | { kind: 'browser'; cat?: Cat; filter?: string; nodeId?: string }
  | { kind: 'files'; op?: OpKind | 'image' }
  | { kind: 'events'; eventKind?: CtxEventKind }
  | { kind: 'commands'; program?: string; filter?: string }
  | { kind: 'fleet' }

/** 当前 agent 的所有子孙 agent 的合计（Agent 网络算好交给概况卡；不含当前 agent 自己） */
export interface FamilyStats {
  agents: number
  billed: number
  cost: number
  /** 没拉到（出错）的子 agent 个数：billed / cost 只算了拉到的；0 或不给是全的 */
  partial?: number
}

/** Token 统计的九类：上下文七类里 assistant 拆成思考 / 回答，工具结果和工具调用合成一类；other 是未记录 */
export type TokCat = 'system' | 'tools' | 'user' | 'inject' | 'skill' | 'reasoning' | 'answer' | 'toolCalls' | 'other'
export const TOK_CATS: TokCat[] = ['system', 'tools', 'user', 'inject', 'skill', 'reasoning', 'answer', 'toolCalls', 'other']

/** Token 统计的配色：沿用上下文类别色，思考用第 8 个分类色槽 */
export function tokColor(c: TokCat, theme: ChartTheme): string {
  if (c === 'reasoning') return theme.cat[7] ?? theme.other
  if (c === 'answer') return catColor('assistant', theme)
  if (c === 'toolCalls') return catColor('tool', theme)
  return catColor(c, theme)
}

/** 时间段的配色 */
export function spanColor(k: SpanKind, theme: ChartTheme): string {
  switch (k) {
    case 'ttft':
      return theme.cat[3] ?? theme.other
    case 'reasoning':
      return theme.cat[7] ?? theme.other
    case 'text':
    case 'model':
      return theme.accent
    case 'toolArgs':
      return theme.cat[1] ?? theme.other
    case 'tools':
      return theme.tool
    default:
      return theme.other
  }
}

/** 会话的时间拆分：各次请求窗口里的段加起来 */
export interface CtxTiming {
  /** 各次请求窗口之和 */
  wallMs: number
  /** 窗口减去等人、减去空档 */
  activeMs: number
  /** 等人（问用户、审批） */
  waitMs: number
  /** 开销里 ≥ 2 分钟没有事件的空档 */
  idleMs: number
  byKind: Record<SpanKind, number>
  /** dsh 流式块的耗时与块数；没有块时间时 null */
  blocks: Record<BlockKind, { ms: number; n: number }> | null
  /** 至少一个工具在跑的时长 */
  toolBusyMs: number
  /** 每次调用的时长之和（并行会重叠） */
  toolSumMs: number
  toolRuns: number
  /** 时间轴：按发生顺序排的段（不含等人和空档），相邻同类合并，太碎的并进前一段 */
  strip: { kind: SpanKind; ms: number }[]
}

/** 一个模型的费用与它的价格卡（费用悬停说明用） */
export interface ModelCost {
  model: string
  steps: number
  tokens: Tokens
  cost: number
  unpriced: boolean
  /** 最后一次请求那一刻的价格卡；null 是目录里没有 */
  card: PriceCard | null
}

export interface CtxSummary {
  turns: number
  steps: number
  humanInputs: number
  answers: number
  toolCalls: number
  toolFailures: number
  compactions: number
  injects: number
  skills: number
  tokens: Tokens
  /** 新输入 + 缓存读写 + 输出 */
  billed: number
  /** 缓存读 ÷ prompt */
  cacheHit: number | null
  /** 计费 token 按组成拆：每次请求的 prompt 按当时的组成分摊；output 单列 */
  split: Parts & { output: number }
  cost: number
  unpriced: boolean
  /** 墙钟 = 每一轮的时长之和 */
  wallMs: number
  modelMs: number
  ttftMs: number | null
  toolMs: number
  outputPerSec: number | null
  slowTools: [string, ToolTime][]
  /** Token 统计：计费合计拆成九类，之和等于 billed */
  tokenStats: Record<TokCat, number>
  /** 各次请求生成的三分之和 */
  generated: Asst3
  timing: CtxTiming
  /** 按调用次数排的工具 */
  topTools: { name: string; calls: number; failures: number; ms: number }[]
  /** 每种上下文事件的条数 */
  eventCounts: Record<CtxEventKind, number>
  /** skill 加载的次数（同一个 skill 加载多次算多次） */
  skillLoads: number
  /** 按费用排的模型 */
  models: ModelCost[]
}

export function summarize(fold: CtxFold): CtxSummary {
  const tokens = zeroTokens()
  const split = { ...zeroParts(), output: 0 }
  let cost = 0
  let unpriced = false
  let modelMs = 0
  let ttft = 0
  let ttftN = 0
  let speedTokens = 0
  let speedMs = 0
  const tok = Object.fromEntries(TOK_CATS.map((c) => [c, 0])) as Record<TokCat, number>
  const byModel = new Map<string, ModelCost & { ts?: string }>()
  for (const s of fold.steps) {
    modelMs += s.modelMs
    if (s.ttftMs != null) {
      ttft += s.ttftMs
      ttftN++
    }
    const u = s.usage
    if (!u) continue
    addTokens(tokens, u)
    split.output += u.output
    const prompt = promptTokens(u)
    const total = sumParts(s.anchored)
    if (total > 0) for (const c of ALL_CATS) split[c] += (prompt * s.anchored[c]) / total
    else split.other += prompt
    splitStep(tok, s.anchored, s.asst, s.gen, prompt, u.output)
    if (s.model) {
      const c = priceTokens(s.model, u, s.ts)
      cost += c.cost_usd
      unpriced ||= c.unpriced
      let m = byModel.get(s.model)
      if (!m) byModel.set(s.model, (m = { model: s.model, steps: 0, tokens: zeroTokens(), cost: 0, unpriced: false, card: null }))
      m.steps++
      addTokens(m.tokens, u)
      m.cost += c.cost_usd
      m.unpriced ||= c.unpriced
      m.ts = s.ts
    }
    // 模型时间是从时间戳推的；太短的（Codex 调用和结果几乎同时落盘）推不准，不拿来算速度
    if (u.output > 0 && s.modelMs >= 1000) {
      speedTokens += u.output
      speedMs += Math.max(1, s.modelMs - (s.ttftMs ?? 0))
    }
  }
  const tools = Object.values(fold.byTool)
  const eventCounts: Record<CtxEventKind, number> = { inject: 0, compaction: 0, prune: 0, model: 0, mode: 0 }
  for (const e of fold.events) eventCounts[e.kind]++
  const models = [...byModel.values()]
    .map(({ ts, ...m }) => ({ ...m, card: priceCard(m.model, ts) }))
    .sort((a, b) => b.cost - a.cost || b.steps - a.steps)
  return {
    turns: fold.turns.length,
    steps: fold.steps.length,
    humanInputs: fold.humanInputs,
    answers: fold.answers,
    toolCalls: tools.reduce((a, x) => a + x.calls, 0),
    toolFailures: tools.reduce((a, x) => a + x.failures, 0),
    compactions: fold.events.filter((e) => e.kind === 'compaction' || e.kind === 'prune').length,
    injects: fold.events.filter((e) => e.kind === 'inject').length,
    skills: new Set(fold.events.filter((e) => e.form === 'skill').map((e) => e.name)).size,
    tokens,
    billed: totalTokens(tokens),
    cacheHit: cacheHitRate(tokens),
    split,
    cost,
    unpriced,
    wallMs: fold.turns.reduce((a, x) => a + x.wallMs, 0),
    modelMs,
    ttftMs: ttftN ? ttft : null,
    toolMs: tools.reduce((a, x) => a + x.ms, 0),
    outputPerSec: speedMs > 0 ? speedTokens / (speedMs / 1000) : null,
    slowTools: Object.entries(fold.byTool)
      .filter(([, x]) => x.ms > 0)
      .sort(([, a], [, b]) => b.ms - a.ms)
      .slice(0, 5),
    tokenStats: tok,
    generated: fold.generated,
    timing: timingOf(fold),
    topTools: Object.entries(fold.byTool)
      .map(([name, x]) => ({ name, ...x }))
      .sort((a, b) => b.calls - a.calls || b.ms - a.ms),
    eventCounts,
    skillLoads: fold.events.filter((e) => e.form === 'skill').length,
    models,
  }
}

/**
 * 一次请求的计费 token 拆进九类：prompt 按校准后的组成分，其中 assistant 那份再按快照时消息面上的三分比例拆；
 * output 里思考取生成的思考（不超过 output），其余按生成的回答 : 工具调用分。工具调用 = 工具结果 + 重放的调用 + 生成的调用
 */
function splitStep(into: Record<TokCat, number>, anchored: Parts, surface: Asst3, gen: Asst3, prompt: number, output: number) {
  const total = sumParts(anchored)
  if (total > 0) {
    const k = prompt / total
    into.system += anchored.system * k
    into.tools += anchored.tools * k
    into.user += anchored.user * k
    into.inject += anchored.inject * k
    into.skill += anchored.skill * k
    into.toolCalls += anchored.tool * k
    into.other += anchored.other * k
    const asst = anchored.assistant * k
    const three = surface.reasoning + surface.answer + surface.call
    if (three > 0) {
      into.reasoning += (asst * surface.reasoning) / three
      into.answer += (asst * surface.answer) / three
      into.toolCalls += (asst * surface.call) / three
    } else into.answer += asst
  } else into.other += prompt
  const reasoning = Math.min(gen.reasoning, output)
  const rest = output - reasoning
  into.reasoning += reasoning
  const ac = gen.answer + gen.call
  if (ac > 0) {
    into.answer += (rest * gen.answer) / ac
    into.toolCalls += (rest * gen.call) / ac
  } else into.answer += rest
}

/** 各次请求的时间段汇总成会话的时间拆分和时间轴 */
function timingOf(fold: CtxFold): CtxTiming {
  const byKind: Record<SpanKind, number> = { ttft: 0, reasoning: 0, text: 0, toolArgs: 0, model: 0, tools: 0, overhead: 0, wait: 0, idle: 0 }
  let wallMs = 0
  let blocks: Record<BlockKind, { ms: number; n: number }> | null = null
  const strip: { kind: SpanKind; ms: number }[] = []
  for (const s of fold.steps) {
    wallMs += s.wallMs
    for (const x of s.spans) {
      const ms = x.end - x.start
      byKind[x.kind] += ms
      if (x.kind === 'wait' || x.kind === 'idle') continue
      const last = strip[strip.length - 1]
      if (last && last.kind === x.kind) last.ms += ms
      else strip.push({ kind: x.kind, ms })
    }
    if (s.blocks) {
      blocks ??= { reasoning: { ms: 0, n: 0 }, text: { ms: 0, n: 0 }, toolArgs: { ms: 0, n: 0 } }
      for (const k of ['reasoning', 'text', 'toolArgs'] as const) {
        blocks[k].ms += s.blocks[k].ms
        blocks[k].n += s.blocks[k].n
      }
    }
  }
  const activeMs = wallMs - byKind.wait - byKind.idle
  // 几千段画不出来：短于全程 1/1500 的并进前一段
  const min = activeMs / 1500
  const packed: { kind: SpanKind; ms: number }[] = []
  for (const x of strip) {
    const last = packed[packed.length - 1]
    if (last && (last.kind === x.kind || x.ms < min)) last.ms += x.ms
    else packed.push({ ...x })
  }
  const tools = Object.values(fold.byTool)
  return {
    wallMs,
    activeMs,
    waitMs: byKind.wait,
    idleMs: byKind.idle,
    byKind,
    blocks: fold.streamTiming ? blocks : null,
    toolBusyMs: fold.toolBusyMs,
    toolSumMs: tools.reduce((a, x) => a + x.ms, 0),
    toolRuns: tools.reduce((a, x) => a + x.calls, 0),
    strip: packed,
  }
}

/** Agent 网络里一张卡的数字 */
export interface FleetStats {
  steps: number
  /** 最后一次请求的 prompt */
  last: number
  peak: number
  billed: number
  cost: number
  wallMs: number
  parts: Parts
}
export type FleetSlot = FleetStats | 'loading' | 'error'

export function fleetStats(f: CtxFold, s: CtxSummary): FleetStats {
  const last = lastWithPrompt(f.steps)
  return {
    steps: s.steps,
    last: last?.prompt ?? 0,
    peak: Math.max(0, ...f.steps.map((x) => x.prompt ?? 0)),
    billed: s.billed,
    cost: s.cost,
    wallMs: s.wallMs,
    parts: last ? last.anchored : zeroParts(),
  }
}

/** 别的会话的卡片数字，跨组件实例留着（切视图不重拉）；键是 id@mtime，会话还在写就会重算 */
export const fleetCache = reactive(new Map<string, FleetSlot>())
