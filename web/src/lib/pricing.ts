// 费用在这里算：后端只给 (model, tokens, 时间)，用 llm-pricing 折成 USD。
// 目录联网时取 models.dev（浏览器 Cache API 存 24h），拉不到用库自带的快照；模型不在目录里才是"未定价"。
import { PricingCatalog, type PricingCache, type TimeInput } from 'llm-pricing'
import type { DailyAgg, HourCell, ModelAgg, ProviderAgg, Summary, SummaryTotals, Tokens } from '../api'

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
export type PricedHourCell = HourCell & { cost_usd: number }
export type PricedTotals = SummaryTotals & {
  cost_usd: number
  /** 目录里没有价格的模型跑的步数 */
  unpriced_steps: number
}

/** 价格目录来源：loading = 还没拉完，先用内置快照出的图；snapshot = 拉失败，最终用的是快照 */
export type PricingSource = 'loading' | 'live' | 'snapshot'

export type PricedSummary = Omit<Summary, 'totals' | 'by_provider' | 'by_model' | 'daily' | 'hourly'> & {
  totals: PricedTotals
  by_provider: Record<string, PricedProviderAgg>
  by_model: Record<string, PricedModelAgg>
  daily: PricedDailyAgg[]
  hourly: PricedHourCell[]
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
    outputTokens: tk.output,
    window,
  })
  return { cost_usd: e.cost, unpriced: e.pricing == null }
}

function priceModels(byModel: Record<string, ModelAgg>, window?: Window): Record<string, PricedModelAgg> {
  const out: Record<string, PricedModelAgg> = {}
  for (const [m, a] of Object.entries(byModel)) out[m] = { ...a, ...estimate(m, a.tokens, window) }
  return out
}

function sumCost(byModel: Record<string, Cost>): number {
  return Object.values(byModel).reduce((a, m) => a + m.cost_usd, 0)
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
  const daily: PricedDailyAgg[] = s.daily.map((d) => {
    const by_model = priceModels(d.by_model, dayWindow(d.date, s.tz_offset_min))
    return { ...d, by_model, cost_usd: sumCost(by_model) }
  })
  const by_model: Record<string, PricedModelAgg> = {}
  for (const [m, a] of Object.entries(s.by_model)) by_model[m] = { ...a, cost_usd: 0, unpriced: false }
  for (const d of daily) {
    for (const [m, a] of Object.entries(d.by_model)) {
      const t = by_model[m]
      if (!t) continue
      t.cost_usd += a.cost_usd
      t.unpriced ||= a.unpriced
    }
  }
  const unpriced_steps = Object.values(by_model).reduce((a, m) => a + (m.unpriced ? m.steps : 0), 0)
  const totals: PricedTotals = { ...s.totals, cost_usd: sumCost(by_model), unpriced_steps }

  // provider 与小时格没有按天的拆分，按整段窗口混价
  const by_provider: Record<string, PricedProviderAgg> = {}
  for (const [p, a] of Object.entries(s.by_provider)) {
    const bm = priceModels(a.by_model, span)
    by_provider[p] = { ...a, by_model: bm, cost_usd: sumCost(bm) }
  }
  const hourly: PricedHourCell[] = s.hourly.map((c) => ({
    ...c,
    cost_usd: Object.entries(c.by_model).reduce((a, [m, tk]) => a + estimate(m, tk, span).cost_usd, 0),
  }))

  return { ...s, totals, by_provider, by_model, daily, hourly, pricing: pricingSource() }
}
