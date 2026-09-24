// 从 Summary 派生出各图共用的东西：模型的固定顺序与配色、provider 配色、缓存命中率。
// 颜色跟着实体走，不跟着排名走：模型顺序在这里定一次，所有图表复用。
import type { Summary, Tokens, ToolCategory } from '../api'
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

export function modelPalette(byModel: Record<string, PricedModelAgg>, t: ChartTheme): ModelPalette {
  const order = modelOrder(byModel)
  // 超过 8 个才折叠；刚好 8 个全给槽位
  const slots = order.length > MAX_SERIES ? order.slice(0, MAX_SERIES - 1) : order
  const folded = order.slice(slots.length)
  const map = new Map(slots.map((m, i) => [m, t.cat[i] ?? t.other]))
  return {
    slots,
    folded,
    color: (m) => map.get(m) ?? t.other,
  }
}

/** provider 沿用列表页 badge 的语义色：claude-code = tool 琥珀，pfh = accent 蓝，dsh = user 紫，codex = ok 绿 */
export function providerColor(provider: string, t: ChartTheme): string {
  if (provider === 'claude-code') return t.tool
  if (provider === 'pfh') return t.accent
  if (provider === 'dsh') return t.user
  if (provider === 'codex') return t.ok
  return t.other
}

/** 工具类别固定顺序 + 固定槽位；meta / other 是"剩下的"，用灰 */
export const CATEGORY_ORDER: ToolCategory[] = ['shell', 'read', 'edit', 'search', 'web', 'agent', 'user', 'mcp', 'meta', 'other']

export const CATEGORY_LABEL: Record<ToolCategory, string> = {
  shell: '命令行',
  read: '读文件',
  edit: '编辑',
  search: '搜索',
  web: '网络',
  agent: '子 agent',
  user: '等用户',
  mcp: 'MCP',
  meta: 'harness',
  other: '其他',
}

export function categoryColor(c: string, t: ChartTheme): string {
  const i = CATEGORY_ORDER.indexOf(c as ToolCategory)
  if (i < 0 || i >= MAX_SERIES) return t.other
  return t.cat[i] ?? t.other
}

export function categoryLabel(c: string): string {
  return CATEGORY_LABEL[c as ToolCategory] ?? c
}

/** 模型实际读到的输入：新输入 + 缓存读 + 缓存写 */
export function promptTokens(tk: Tokens): number {
  return tk.input + tk.cache_read + tk.cache_write
}

/** 输入（含缓存读写）+ 输出 */
export function totalTokens(tk: Tokens): number {
  return promptTokens(tk) + tk.output
}

export function cacheHitRate(tk: Tokens): number | null {
  const denom = promptTokens(tk)
  return denom > 0 ? tk.cache_read / denom : null
}

/** 整个归档都没东西（和"选中范围内没活动"区分开） */
export function isEmpty(s: Summary): boolean {
  return s.available[0] == null && s.totals.sessions === 0 && s.daily.length === 0
}

/** 选中的范围里没有 turn */
export function isRangeEmpty(s: Summary): boolean {
  return s.totals.turns === 0 && s.daily.length === 0
}

export interface DateRange {
  from: string | null
  to: string | null
}

const DATE_RE = /^\d{4}-\d{2}-\d{2}$/
export function isDateStr(s: unknown): s is string {
  return typeof s === 'string' && DATE_RE.test(s) && !Number.isNaN(new Date(`${s}T00:00:00`).getTime())
}

export function addDays(date: string, n: number): string {
  const d = new Date(`${date}T00:00:00`)
  d.setDate(d.getDate() + n)
  return localDate(d)
}

/** 预设范围，都以本地"今天"为准 */
export type RangePreset = 'all' | 'today' | '7d' | '30d' | '90d' | 'month' | 'lastMonth'
export const RANGE_PRESETS: { key: RangePreset; label: string }[] = [
  { key: 'all', label: '全部' },
  { key: 'today', label: '今天' },
  { key: '7d', label: '7 天' },
  { key: '30d', label: '30 天' },
  { key: '90d', label: '90 天' },
  { key: 'month', label: '本月' },
  { key: 'lastMonth', label: '上月' },
]

export function presetRange(p: RangePreset, now = new Date()): DateRange {
  const today = localDate(now)
  const y = now.getFullYear()
  const m = now.getMonth()
  switch (p) {
    case 'all':
      return { from: null, to: null }
    case 'today':
      return { from: today, to: today }
    case '7d':
      return { from: addDays(today, -6), to: today }
    case '30d':
      return { from: addDays(today, -29), to: today }
    case '90d':
      return { from: addDays(today, -89), to: today }
    case 'month':
      return { from: localDate(new Date(y, m, 1)), to: today }
    case 'lastMonth':
      return { from: localDate(new Date(y, m - 1, 1)), to: localDate(new Date(y, m, 0)) }
  }
}

export function sameRange(a: DateRange, b: DateRange): boolean {
  return (a.from ?? null) === (b.from ?? null) && (a.to ?? null) === (b.to ?? null)
}

/** 本地日期串 YYYY-MM-DD */
export function localDate(d: Date): string {
  const p = (n: number) => (n < 10 ? `0${n}` : String(n))
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`
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

/** 给读屏的图表概括（canvas 里的东西读屏读不到）：几个点有值、合计、最高的那个 */
export function chartSummary(head: string, points: { label: string; v: number }[], unit: string, fmt: (v: number) => string): string {
  const hit = points.filter((p) => p.v > 0)
  if (!hit.length) return `${head}：没有数据`
  const top = hit.reduce((a, p) => (p.v > a.v ? p : a))
  const total = hit.reduce((a, p) => a + p.v, 0)
  return `${head}：${hit.length} ${unit}，合计 ${fmt(total)}，最高 ${top.label} ${fmt(top.v)}`
}

export const WEEKDAYS = ['周一', '周二', '周三', '周四', '周五', '周六', '周日']

/** 名字太长时截中间 */
export function shortModel(name: string, max = 22): string {
  if (name.length <= max) return name
  return `${name.slice(0, max - 8)}…${name.slice(-6)}`
}
