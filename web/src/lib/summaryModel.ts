// 从 Summary 派生出各图共用的东西：模型的固定顺序与配色、provider 配色、缓存命中率。
// 颜色跟着实体走，不跟着排名走：模型顺序在这里定一次，所有图表复用。
import type { Summary, Tokens, ToolCategory } from '../api'
import { t } from '../i18n'
import { tn } from './format'
import type { ChartTheme } from './chartTheme'
import type { PricedModelAgg } from './pricing'

export const MAX_SERIES = 8
export const OTHER = 'other'

/** 模型固定顺序：费用 → 输出 token → 步数 → 名字。前 8 个各占一槽，其余折到 other */
export function modelOrder(byModel: Record<string, PricedModelAgg>): string[] {
  return Object.entries(byModel)
    .sort(([na, a], [nb, b]) => b.cost_usd - a.cost_usd || b.tokens.output - a.tokens.output || b.steps - a.steps || na.localeCompare(nb))
    .map(([n]) => n)
}

export interface ModelPalette {
  /** 有槽位的模型，按固定顺序 */
  slots: string[]
  /** 折进 other 的模型 */
  folded: string[]
  color: (model: string) => string
}

export function modelPalette(byModel: Record<string, PricedModelAgg>, theme: ChartTheme): ModelPalette {
  const order = modelOrder(byModel)
  // 超过 8 个才折叠；刚好 8 个全给槽位
  const slots = order.length > MAX_SERIES ? order.slice(0, MAX_SERIES - 1) : order
  const folded = order.slice(slots.length)
  const map = new Map(slots.map((m, i) => [m, theme.cat[i] ?? theme.other]))
  return {
    slots,
    folded,
    color: (m) => map.get(m) ?? theme.other,
  }
}

/** provider 沿用列表页 badge 的语义色：claude-code = tool 琥珀，pfh = accent 蓝，dsh = user 紫，codex = ok 绿 */
export function providerColor(provider: string, theme: ChartTheme): string {
  if (provider === 'claude-code') return theme.tool
  if (provider === 'pfh') return theme.accent
  if (provider === 'dsh') return theme.user
  if (provider === 'codex') return theme.ok
  return theme.other
}

/** 工具类别固定顺序 + 固定槽位；meta / other 是"剩下的"，用灰 */
export const CATEGORY_ORDER: ToolCategory[] = ['shell', 'read', 'edit', 'search', 'web', 'agent', 'user', 'mcp', 'meta', 'other']

export function categoryColor(c: string, theme: ChartTheme): string {
  const i = CATEGORY_ORDER.indexOf(c as ToolCategory)
  if (i < 0 || i >= MAX_SERIES) return theme.other
  return theme.cat[i] ?? theme.other
}

export function categoryLabel(c: string): string {
  return CATEGORY_ORDER.includes(c as ToolCategory) ? t(`dash.category.${c as ToolCategory}`) : c
}

/** 模型实际读到的输入：新输入 + 缓存读 + 缓存写 */
export function promptTokens(tk: Tokens): number {
  return tk.input + tk.cache_read + tk.cache_write
}

/** 输入（含缓存读写）+ 输出 */
export function totalTokens(tk: Tokens): number {
  return promptTokens(tk) + tk.output
}

export function zeroTokens(): Tokens {
  return { input: 0, output: 0, cache_read: 0, cache_write: 0, cache_write_1h: 0, reasoning: 0 }
}

export function addTokens(a: Tokens, b: Tokens) {
  a.input += b.input
  a.output += b.output
  a.cache_read += b.cache_read
  a.cache_write += b.cache_write
  a.cache_write_1h += b.cache_write_1h
  a.reasoning += b.reasoning
}

export function cacheHitRate(tk: Tokens): number | null {
  const denom = promptTokens(tk)
  return denom > 0 ? tk.cache_read / denom : null
}

/** 整个归档都没东西（和"选中范围内没活动"区分开） */
export function isEmpty(s: Pick<Summary, 'available' | 'totals' | 'daily'>): boolean {
  return s.available[0] == null && s.totals.sessions === 0 && s.daily.length === 0
}

/** 选中的范围里没有 turn */
export function isRangeEmpty(s: Pick<Summary, 'totals' | 'daily'>): boolean {
  return s.totals.turns === 0 && s.daily.length === 0
}

/** 选中的时间范围，已经落到本地日期上（两端都含），图表按它铺日期轴 */
export interface DateRange {
  from: string | null
  to: string | null
  /** 滚动窗口（"最近 N"）的精确起点，ISO；自然日期范围是 null */
  since?: string | null
  /** 来自哪个预设；自定义日期是 null */
  preset?: RangePreset | null
}

const DATE_RE = /^\d{4}-\d{2}-\d{2}$/
export function isDateStr(s: unknown): s is string {
  return typeof s === 'string' && DATE_RE.test(s) && !Number.isNaN(new Date(`${s}T00:00:00`).getTime())
}

export function monthStart(date: string): string {
  return `${date.slice(0, 7)}-01`
}
export function monthEnd(date: string): string {
  const d = new Date(`${date}T00:00:00`)
  return localDate(new Date(d.getFullYear(), d.getMonth() + 1, 0))
}

export function addDays(date: string, n: number): string {
  const d = new Date(`${date}T00:00:00`)
  d.setDate(d.getDate() + n)
  return localDate(d)
}

/** 预设：自然周期（今天、本周……，跟着日历走）和滚动窗口（最近 N 小时 / 天，跟着此刻走）。
 * URL 里存预设名而不是算出来的日期，这样明天打开"本月"还是本月 */
export type CalendarPreset = 'today' | 'yesterday' | 'week' | 'lastWeek' | 'month' | 'lastMonth' | 'year' | 'lastYear'
export type RollingPreset = '24h' | '7d' | '30d' | '90d' | '365d'
export type RangePreset = 'all' | CalendarPreset | RollingPreset

/** 自然周期，按 日 / 周 / 月 / 年 两两成对（"这个"和"上一个"）；名字用 presetLabel 取 */
export const CALENDAR_PRESETS: CalendarPreset[] = ['today', 'yesterday', 'week', 'lastWeek', 'month', 'lastMonth', 'year', 'lastYear']
export const ROLLING: { key: RollingPreset; hours: number }[] = [
  { key: '24h', hours: 24 },
  { key: '7d', hours: 7 * 24 },
  { key: '30d', hours: 30 * 24 },
  { key: '90d', hours: 90 * 24 },
  { key: '365d', hours: 365 * 24 },
]

const PRESETS: RangePreset[] = ['all', ...CALENDAR_PRESETS, ...ROLLING.map((p) => p.key)]

const isRolling = (p: RangePreset): p is RollingPreset => ROLLING.some((r) => r.key === p)

export function isPreset(s: unknown): s is RangePreset {
  return typeof s === 'string' && PRESETS.includes(s as RangePreset)
}
/** 完整名字：本周、最近 24 小时 */
export function presetLabel(p: RangePreset): string {
  return isRolling(p) ? t('dash.preset.recent', { span: t(`dash.rolling.${p}`) }) : t(`dash.preset.${p}`)
}
/** 选择器按钮上的短名：滚动窗口省掉"最近" */
export function presetShortLabel(p: RangePreset): string {
  return isRolling(p) ? t(`dash.rolling.${p}`) : presetLabel(p)
}

/** 预设落到具体范围，都以本地"此刻"为准；进行中的周期截到今天 */
export function presetRange(p: RangePreset, now = new Date()): DateRange {
  const today = localDate(now)
  const y = now.getFullYear()
  const m = now.getMonth()
  const cal = (from: string, to: string): DateRange => ({ from, to, since: null, preset: p })
  const roll = ROLLING.find((r) => r.key === p)
  if (roll) {
    const since = new Date(now.getTime() - roll.hours * 3_600_000)
    return { from: localDate(since), to: today, since: since.toISOString(), preset: p }
  }
  switch (p as Exclude<RangePreset, RollingPreset>) {
    case 'all':
      return { from: null, to: null, since: null, preset: p }
    case 'today':
      return cal(today, today)
    case 'yesterday': {
      const d = addDays(today, -1)
      return cal(d, d)
    }
    case 'week':
      return cal(bucketOf(today, 'week'), today)
    case 'lastWeek': {
      const mon = addDays(bucketOf(today, 'week'), -7)
      return cal(mon, addDays(mon, 6))
    }
    case 'month':
      return cal(localDate(new Date(y, m, 1)), today)
    case 'lastMonth':
      return cal(localDate(new Date(y, m - 1, 1)), localDate(new Date(y, m, 0)))
    case 'year':
      return cal(`${y}-01-01`, today)
    case 'lastYear':
      return cal(`${y - 1}-01-01`, `${y - 1}-12-31`)
  }
}

/** 同一个选择：预设比名字，自定义比日期 */
export function sameRange(a: DateRange, b: DateRange): boolean {
  if (a.preset || b.preset) return a.preset === b.preset
  return (a.from ?? null) === (b.from ?? null) && (a.to ?? null) === (b.to ?? null)
}

/** 本地日期串 YYYY-MM-DD */
export function localDate(d: Date): string {
  const p = (n: number) => (n < 10 ? `0${n}` : String(n))
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`
}

/** 选中范围的首尾两天：单边时另一边用数据边界补，再没有就用有数据的首尾天 */
export function spanOf(
  range: DateRange,
  bounds: { min: string | null; max: string | null },
  daily: { date: string }[],
): [string, string] | null {
  const first = range.from ?? bounds.min ?? daily[0]?.date
  const last = range.to ?? bounds.max ?? daily[daily.length - 1]?.date
  return first && last && first <= last ? [first, last] : null
}

/** from 到 to（含）有几天 */
export function dayCount(from: string, to: string): number {
  return Math.round((Date.parse(`${to}T00:00:00Z`) - Date.parse(`${from}T00:00:00Z`)) / 86_400_000) + 1
}

export type Gran = 'hour' | 'day' | 'week' | 'month'

/** 跨度决定粒度：柱子多到几百根就只剩噪声，只有一根又什么都看不出。
 * 一天按小时，62 天内按日，约 7 个月内按周，再长按月 */
export function granOf(days: number): Gran {
  return days === 1 ? 'hour' : days <= 62 ? 'day' : days <= 210 ? 'week' : 'month'
}

/** 一天归到哪个桶：日（和小时）就是自己，周是那周的周一，月是 YYYY-MM */
export function bucketOf(date: string, g: Gran): string {
  if (g === 'month') return date.slice(0, 7)
  if (g === 'week') {
    const d = new Date(`${date}T00:00:00`)
    d.setDate(d.getDate() - ((d.getDay() + 6) % 7))
    return localDate(d)
  }
  return date
}

/** 从 from 到 to（含）每天一个日期串 */
export function dateRange(from: string, to: string): string[] {
  const out: string[] = []
  const d = new Date(`${from}T00:00:00`)
  const end = new Date(`${to}T00:00:00`)
  while (d <= end) {
    out.push(localDate(d))
    d.setDate(d.getDate() + 1)
  }
  return out
}

/** 给读屏的图表概括（canvas 里的东西读屏读不到）：几个点有值、合计、最高的那个。countKey 是把有值的点数写成一句的消息（"12 天有活动"） */
export function chartSummary(head: string, points: { label: string; v: number }[], countKey: string, fmt: (v: number) => string): string {
  const hit = points.filter((p) => p.v > 0)
  if (!hit.length) return t('dash.common.chartEmpty', { head })
  const top = hit.reduce((a, p) => (p.v > a.v ? p : a))
  const total = hit.reduce((a, p) => a + p.v, 0)
  return t('dash.common.chartSummary', { head, count: tn(countKey, hit.length), total: fmt(total), top: top.label, topValue: fmt(top.v) })
}
