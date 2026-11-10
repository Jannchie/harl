// 从 style.css 的 token 里读图表颜色，跟随 <html data-scheme> 变化。
// 所有图表 option 都从 useChartTheme() 取色，不写死 hex。
import { readonly, ref, watch, type Ref } from 'vue'
import { scheme } from './scheme'
import type { TooltipComponentOption } from 'echarts/components'

export interface ChartTheme {
  dark: boolean
  bg: string
  /** 卡片（page-ground 上叠一层 surface）的实色：热图格子的缝要和卡片同色才像留白 */
  surface: string
  /** 热图里没有数据的格子，比卡片再深一档 */
  empty: string
  fg: string
  fg1: string
  fg2: string
  line: string
  grid: string
  axis: string
  accent: string
  danger: string
  ok: string
  tool: string
  user: string
  /** 8 槽分类色，固定顺序，永不循环；超出折到 other */
  cat: string[]
  /** 第 6 槽（绿）同色相浅一档：思考，和回答（第 6 槽本身）只差明度 */
  cat6Soft: string
  other: string
  /** 7 步顺序渐变，近零 → 最大 */
  seq: string[]
  fontFamily: string
}

function readTheme(): ChartTheme {
  const cs = getComputedStyle(document.documentElement)
  const v = (name: string) => cs.getPropertyValue(name).trim()
  const dark = scheme.value === 'dark'
  return {
    dark,
    bg: v('--bg'),
    // 与 uno 的 surface（black/4、white/5）同一个配比
    surface: blend(v('--bg'), dark ? 255 : 0, dark ? 0.05 : 0.04),
    // 深色只多叠一点：叠到 10% 会比 --seq-1（最小的那档）还亮，空格子反倒像有数据
    empty: blend(v('--bg'), dark ? 255 : 0, dark ? 0.06 : 0.08),
    fg: v('--fg'),
    fg1: v('--fg-1'),
    fg2: v('--fg-2'),
    line: v('--line'),
    grid: v('--chart-grid'),
    axis: v('--chart-axis'),
    accent: v('--accent'),
    danger: v('--danger'),
    ok: v('--ok'),
    tool: v('--tool'),
    user: v('--user'),
    cat: [1, 2, 3, 4, 5, 6, 7, 8].map((i) => v(`--chart-${i}`)),
    cat6Soft: v('--chart-6-soft'),
    other: v('--chart-other'),
    seq: [1, 2, 3, 4, 5, 6, 7].map((i) => v(`--seq-${i}`)),
    fontFamily: v('--font-ui') || getComputedStyle(document.body).fontFamily,
  }
}

let shared: Ref<ChartTheme> | null = null

// 模块级常驻：没有图表挂着时也跟着配色走，下次挂载拿到的不会是旧色板。
// 等浏览器把新 token 算出来再读
watch(scheme, () =>
  requestAnimationFrame(() => {
    if (shared) shared.value = readTheme()
  }),
)

/** 单例的响应式主题；首次使用时才读 token（那时样式表已经就位） */
export function useChartTheme(): Readonly<Ref<ChartTheme>> {
  if (!shared) shared = ref(readTheme())
  return readonly(shared) as Readonly<Ref<ChartTheme>>
}

/** 所有图表共用的 tooltip 外观 */
export function tooltipBase(theme: ChartTheme): TooltipComponentOption {
  return {
    backgroundColor: theme.bg,
    borderColor: theme.line,
    borderWidth: 1,
    padding: [8, 12],
    textStyle: { color: theme.fg, fontSize: 12, fontFamily: theme.fontFamily },
    extraCssText: 'box-shadow: var(--tooltip-shadow); border-radius: 12px; max-width: 360px;',
    confine: true,
  }
}

/** tooltip 里的一行：色点 + 名称 + 右对齐数值 */
export function ttRow(color: string | null, label: string, value: string, muted = false): string {
  const dot = color
    ? `<span style="display:inline-block;width:8px;height:8px;border-radius:50%;background:${color};margin-right:6px;vertical-align:middle"></span>`
    : ''
  return `<div style="display:flex;justify-content:space-between;gap:16px;line-height:1.6;${muted ? 'color:var(--fg-2)' : ''}"><span>${dot}${esc(label)}</span><span style="font-variant-numeric:tabular-nums">${esc(value)}</span></div>`
}

export function ttTitle(text: string): string {
  return `<div style="font-weight:600;margin-bottom:3px">${esc(text)}</div>`
}

export function esc(s: string): string {
  return s.replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c] ?? c)
}

/** 直角坐标系图表的轴样式：细线、退后的网格 */
export function axisBase(theme: ChartTheme) {
  return {
    axisLine: { show: false, lineStyle: { color: theme.axis } },
    axisTick: { show: false },
    axisLabel: { color: theme.fg2, fontSize: 11, fontFamily: theme.fontFamily },
    splitLine: { lineStyle: { color: theme.grid, type: 'solid' as const } },
  }
}

/** 纯色 hex 上叠一层 alpha 的黑 / 白，得到实色 */
function blend(hex: string, channel: number, alpha: number): string {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex)
  if (!m) return hex
  const n = parseInt(m[1]!, 16)
  const mix = (c: number) => Math.round(c * (1 - alpha) + channel * alpha)
  const out = [(n >> 16) & 255, (n >> 8) & 255, n & 255].map(mix)
  return `#${out.map((c) => c.toString(16).padStart(2, '0')).join('')}`
}

/** 填充色上放文字：按亮度选白或墨（墨默认是浅色主题的 --fg） */
export function inkOn(hex: string, dark = '#171717', light = '#ffffff'): string {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim())
  if (!m) return light
  const n = parseInt(m[1]!, 16)
  const lum = (0.2126 * ((n >> 16) & 255) + 0.7152 * ((n >> 8) & 255) + 0.0722 * (n & 255)) / 255
  return lum > 0.6 ? dark : light
}
