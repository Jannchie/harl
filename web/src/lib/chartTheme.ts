// 从 style.css 的 token 里读图表颜色，跟随 prefers-color-scheme 变化。
// 所有图表 option 都从 useChartTheme() 取色，不写死 hex。
import { onScopeDispose, readonly, ref, type Ref } from 'vue'
import type { TooltipComponentOption } from 'echarts/components'

export interface ChartTheme {
  dark: boolean
  bg: string
  bg1: string
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
  other: string
  /** 7 步顺序渐变，近零 → 最大 */
  seq: string[]
  fontFamily: string
  monoFamily: string
}

function readTheme(): ChartTheme {
  const cs = getComputedStyle(document.documentElement)
  const v = (name: string) => cs.getPropertyValue(name).trim()
  const dark = window.matchMedia('(prefers-color-scheme: dark)').matches
  return {
    dark,
    bg: v('--bg'),
    bg1: v('--bg-1'),
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
    other: v('--chart-other'),
    seq: [1, 2, 3, 4, 5, 6, 7].map((i) => v(`--seq-${i}`)),
    fontFamily: v('--font-ui'),
    monoFamily: v('--font-mono'),
  }
}

let shared: Ref<ChartTheme> | null = null
let listeners = 0
let mq: MediaQueryList | null = null
let onChange: (() => void) | null = null

/** 单例的响应式主题；组件卸载后没人用了就解绑 media query 监听 */
export function useChartTheme(): Readonly<Ref<ChartTheme>> {
  if (!shared) shared = ref(readTheme())
  if (listeners === 0) {
    mq = window.matchMedia('(prefers-color-scheme: dark)')
    onChange = () => {
      // 等浏览器把新 token 算出来再读
      requestAnimationFrame(() => {
        if (shared) shared.value = readTheme()
      })
    }
    mq.addEventListener('change', onChange)
  }
  listeners++
  onScopeDispose(() => {
    listeners--
    if (listeners === 0 && mq && onChange) {
      mq.removeEventListener('change', onChange)
      mq = null
      onChange = null
    }
  })
  return readonly(shared) as Readonly<Ref<ChartTheme>>
}

/** 所有图表共用的 tooltip 外观 */
export function tooltipBase(t: ChartTheme): TooltipComponentOption {
  return {
    backgroundColor: t.bg,
    borderColor: t.line,
    borderWidth: 1,
    padding: [6, 10],
    textStyle: { color: t.fg, fontSize: 12, fontFamily: t.fontFamily },
    extraCssText: 'box-shadow: 0 4px 16px rgba(0,0,0,.18); border-radius: 2px; max-width: 360px;',
    confine: true,
  }
}

/** tooltip 里的一行：色点 + 名称 + 右对齐数值 */
export function ttRow(color: string | null, label: string, value: string, muted = false): string {
  const dot = color
    ? `<span style="display:inline-block;width:8px;height:8px;border-radius:50%;background:${color};margin-right:6px;vertical-align:middle"></span>`
    : ''
  return `<div style="display:flex;justify-content:space-between;gap:16px;line-height:1.6;${muted ? 'opacity:.7' : ''}"><span>${dot}${esc(label)}</span><span style="font-family:ui-monospace,Menlo,Consolas,monospace;font-variant-numeric:tabular-nums">${esc(value)}</span></div>`
}

export function ttTitle(text: string): string {
  return `<div style="font-weight:600;margin-bottom:3px">${esc(text)}</div>`
}

export function esc(s: string): string {
  return s.replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c] ?? c)
}

/** 直角坐标系图表的轴样式：细线、退后的网格 */
export function axisBase(t: ChartTheme) {
  return {
    axisLine: { lineStyle: { color: t.axis } },
    axisTick: { show: false },
    axisLabel: { color: t.fg2, fontSize: 11, fontFamily: t.fontFamily },
    splitLine: { lineStyle: { color: t.grid, type: 'solid' as const } },
  }
}
