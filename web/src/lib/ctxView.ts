// 上下文视图各卡片共用的：类别配色与名字、偏好（localStorage）、会话级汇总（token / 费用 / 时间）。
import { reactive, ref, watch, type Ref } from 'vue'
import type { Tokens } from '../api'
import { t } from '../i18n'
import type { ChartTheme } from './chartTheme'
import { ALL_CATS, lastWithPrompt, sumParts, zeroParts, type Cat, type CtxFold, type Parts, type ToolTime } from './context'
import { priceTokens } from './pricing'
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
    if (s.model) {
      const c = priceTokens(s.model, u, s.ts)
      cost += c.cost_usd
      unpriced ||= c.unpriced
    }
    // 模型时间是从时间戳推的；太短的（Codex 调用和结果几乎同时落盘）推不准，不拿来算速度
    if (u.output > 0 && s.modelMs >= 1000) {
      speedTokens += u.output
      speedMs += Math.max(1, s.modelMs - (s.ttftMs ?? 0))
    }
  }
  const tools = Object.values(fold.byTool)
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
