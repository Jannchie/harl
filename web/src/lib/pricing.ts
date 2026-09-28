// 费用在这里算：后端只给 (model, tokens, 时间)，用 llm-pricing 折成 USD。
// 目录联网时取 models.dev（浏览器 Cache API 存 24h），拉不到用库自带的快照；模型不在目录里才是"未定价"。
import { PricingCatalog, type PricingCache, type TimeInput } from 'llm-pricing'
import type { ByTag, DailyAgg, HourCell, ModelAgg, ProviderAgg, SessionRow, Summary, SummaryTotals, Tokens } from '../api'
import { addTokens, promptTokens, zeroTokens } from './summaryModel'

/** 折算出来的费用；unpriced 时 cost_usd 是 0，不代表免费 */
export interface Cost {
  /** 等价费用（一方 API 价），不是账单 */
  cost_usd: number
  unpriced: boolean
}

export type PricedModelAgg = ModelAgg & Cost
/** 带 by_model 的聚合节点：自己有总费用，每个模型有各自的 */
type Priced<T extends { by_model: unknown }> = Omit<T, 'by_model'> & { cost_usd: number; by_model: Record<string, PricedModelAgg> }
export type PricedProviderAgg = Priced<ProviderAgg>
export type PricedDailyAgg = Priced<DailyAgg>
/** model_cost：每个模型的费用，单日趋势按小时画时用 */
export type PricedHourCell = HourCell & { cost_usd: number; model_cost: Record<string, number> }
export type PricedSessionRow = Omit<SessionRow, 'by_model'> & { cost_usd: number; by_model: Record<string, PricedModelAgg> }
export type PricedTotals = SummaryTotals & {
  cost_usd: number
  /** 假如没有提示缓存（缓存读写都按新输入计价）要花多少；和 cost_usd 的差就是缓存省下的钱 */
  nocache_usd: number
  /** 目录里没有价格的模型跑的步数 */
  unpriced_steps: number
}

/** 一个标签（推理强度、skill）下的合计：步数 / tokens / 时间把各模型加起来，费用按模型分别折算再相加 */
export type PricedTag = ModelAgg & Cost & { by_model: Record<string, PricedModelAgg> }

/** 价格目录来源：loading = 还没拉完，先用内置快照出的图；snapshot = 拉失败，最终用的是快照 */
export type PricingSource = 'loading' | 'live' | 'snapshot'

export type PricedSummary = Omit<
  Summary,
  'totals' | 'by_provider' | 'by_model' | 'daily' | 'hourly' | 'sessions' | 'by_effort' | 'by_skill'
> & {
  totals: PricedTotals
  by_provider: Record<string, PricedProviderAgg>
  by_model: Record<string, PricedModelAgg>
  daily: PricedDailyAgg[]
  hourly: PricedHourCell[]
  sessions: PricedSessionRow[]
  by_effort: Record<string, PricedTag>
  by_skill: Record<string, PricedTag>
  pricing: PricingSource
}

/** models.dev 有 4 MB，用 Cache API 跨页面刷新留着；不可用（非安全上下文）就退回内存 */
function browserCache(): PricingCache | undefined {
  if (typeof caches === 'undefined') return undefined
  const store = caches.open('llm-pricing')
  return {
    get: async (key) => {
      const res = await (await store).match(key)
      return res ? res.text() : null
    },
    set: async (key, value) => {
      await (await store).put(key, new Response(value, { headers: { 'content-type': 'text/plain' } }))
    },
  }
}

const catalog = new PricingCatalog({
  cache: browserCache(),
  // 库把 fetch 存成属性再调用，浏览器里 window.fetch 脱离 this 会抛 Illegal invocation，包一层
  fetch: (input, init) => globalThis.fetch(input, init),
  onWarn: (msg, err) => console.warn('[pricing]', msg, err),
})

let loaded = false

/** 拉一次目录（24h 内直接返回）。失败不抛：库会退回快照 */
export async function ensurePricing(): Promise<void> {
  await catalog.ensureLoaded()
  loaded = true
}

/** 模型的展示名取价格目录里的 displayName（如 Claude Opus 4.5）；目录里没有就用原 id */
export function modelLabel(model: string): string {
  return catalog.getSchedule(model)?.displayName ?? model
}

function pricingSource(): PricingSource {
  if (!loaded) return 'loading'
  return catalog.state().status === 'ready' ? 'live' : 'snapshot'
}

type Window = readonly [TimeInput, TimeInput]

/**
 * harl 的 Tokens 是 Anthropic 形状：input 是 fresh 输入，cache 读/写在旁边（inputIncludesCache: false）。
 * pfh 的 inputTokens / cacheReadTokens 也按这个口径解释。
 */
function estimate(model: string, tk: Tokens, window?: Window): Cost {
  const e = catalog.estimate({
    model,
    inputTokens: tk.input,
    inputIncludesCache: false,
    cachedInputTokens: 0,
    cacheReadInputTokens: tk.cache_read,
    cacheCreationInputTokens: tk.cache_write,
    // 1 小时 TTL 的缓存写按 2× 输入价，其余按目录里的（5 分钟）缓存写价
    cacheCreation1hInputTokens: tk.cache_write_1h,
    outputTokens: tk.output,
    window,
  })
  return { cost_usd: e.cost, unpriced: e.pricing == null }
}

/** 同样的 tokens 假如没有提示缓存（缓存读写都按新输入计价）要花多少；只有总计要用 */
function nocacheCost(model: string, tk: Tokens, window?: Window): number {
  return catalog.estimate({
    model,
    inputTokens: promptTokens(tk),
    inputIncludesCache: false,
    cachedInputTokens: 0,
    cacheReadInputTokens: 0,
    cacheCreationInputTokens: 0,
    outputTokens: tk.output,
    window,
  }).cost
}

function priceModels(byModel: Record<string, ModelAgg>, window?: Window): Record<string, PricedModelAgg> {
  const out: Record<string, PricedModelAgg> = {}
  for (const [m, a] of Object.entries(byModel)) out[m] = { ...a, ...estimate(m, a.tokens, window) }
  return out
}

function sumCost(byModel: Record<string, Cost>): number {
  return Object.values(byModel).reduce((a, m) => a + m.cost_usd, 0)
}

/** 累加用的零值：每个模型的 tokens 原样带着，费用从 0 开始 */
function zeroPriced(byModel: Record<string, ModelAgg>): Record<string, PricedModelAgg> {
  const out: Record<string, PricedModelAgg> = {}
  for (const [m, a] of Object.entries(byModel)) out[m] = { ...a, cost_usd: 0, unpriced: false }
  return out
}

function addCost(into: Record<string, PricedModelAgg>, from: Record<string, Cost>) {
  for (const [m, x] of Object.entries(from)) {
    const t = into[m]
    if (!t) continue
    t.cost_usd += x.cost_usd
    t.unpriced ||= x.unpriced
  }
}

function priceTags(tags: ByTag | undefined, window?: Window): Record<string, PricedTag> {
  const out: Record<string, PricedTag> = {}
  for (const [tag, models] of Object.entries(tags ?? {})) {
    const by_model = priceModels(models, window)
    const agg: PricedTag = { steps: 0, tokens: zeroTokens(), model_ms: 0, cost_usd: 0, unpriced: false, by_model }
    for (const m of Object.values(by_model)) {
      agg.steps += m.steps
      agg.model_ms += m.model_ms
      addTokens(agg.tokens, m.tokens)
      agg.cost_usd += m.cost_usd
      agg.unpriced ||= m.unpriced
    }
    out[tag] = agg
  }
  return out
}

/** 本地日期在请求时区下的 [当天 0 点, 次日 0 点) */
function dayWindow(date: string, tzOffsetMin: number): Window {
  const start = Date.parse(`${date}T00:00:00Z`) - tzOffsetMin * 60_000
  return [start, start + 86_400_000]
}

/** 给整个 Summary 加上费用。同步、纯计算；ensurePricing() 之前调用用的是内置快照 */
export function priceSummary(s: Summary): PricedSummary {
  const span: Window | undefined = s.totals.first_ts && s.totals.last_ts ? [s.totals.first_ts, s.totals.last_ts] : undefined

  // 日桶用当天的窗口（峰谷价 / 历史价按天算）；by_model 与 totals 从日桶累加，保证 KPI 与趋势图一致。
  // 后端的 by_model 和每天的 by_model 来自同一批 turn，所以每个模型必然出现在某一天里。
  let nocache_usd = 0
  const daily: PricedDailyAgg[] = s.daily.map((d) => {
    const window = dayWindow(d.date, s.tz_offset_min)
    const by_model = priceModels(d.by_model, window)
    for (const [m, a] of Object.entries(d.by_model)) nocache_usd += nocacheCost(m, a.tokens, window)
    return { ...d, by_model, cost_usd: sumCost(by_model) }
  })
  const by_model = zeroPriced(s.by_model)
  for (const d of daily) addCost(by_model, d.by_model)
  const unpriced_steps = Object.values(by_model).reduce((a, m) => a + (m.unpriced ? m.steps : 0), 0)
  const totals: PricedTotals = { ...s.totals, cost_usd: sumCost(by_model), nocache_usd, unpriced_steps }

  // provider 也按天定价：从每天的 provider × 模型累加，和 totals 同一口径，两处数字对得上
  const by_provider: Record<string, PricedProviderAgg> = {}
  for (const [p, a] of Object.entries(s.by_provider)) by_provider[p] = { ...a, by_model: zeroPriced(a.by_model), cost_usd: 0 }
  for (const d of s.daily) {
    const window = dayWindow(d.date, s.tz_offset_min)
    for (const [p, models] of Object.entries(d.by_provider ?? {})) {
      const agg = by_provider[p]
      if (agg) addCost(agg.by_model, priceModels(models, window))
    }
  }
  for (const agg of Object.values(by_provider)) agg.cost_usd = sumCost(agg.by_model)

  // 小时格没有按天的拆分，只能按整段窗口混价（热图里的费用是近似）
  const hourly: PricedHourCell[] = s.hourly.map((c) => {
    const model_cost: Record<string, number> = {}
    for (const [m, tk] of Object.entries(c.by_model)) model_cost[m] = estimate(m, tk, span).cost_usd
    return { ...c, model_cost, cost_usd: Object.values(model_cost).reduce((a, x) => a + x, 0) }
  })

  // 会话按自己的首尾时间定价（没有按天拆分）；项目费用由会话累加，和总费用在分时价上可能略有出入
  const sessions: PricedSessionRow[] = s.sessions.map((r) => {
    const w: Window | undefined = r.first_ts && r.last_ts ? [r.first_ts, r.last_ts] : span
    const by_model = priceModels(r.by_model, w)
    return { ...r, by_model, cost_usd: sumCost(by_model) }
  })

  // 推理强度 / skill 没有按天的拆分，按整段窗口定价（和总费用在分时价上可能略有出入）
  const by_effort = priceTags(s.by_effort, span)
  const by_skill = priceTags(s.by_skill, span)

  return { ...s, totals, by_provider, by_model, daily, hourly, sessions, by_effort, by_skill, pricing: pricingSource() }
}
