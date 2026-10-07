<script setup lang="ts">
import { computed, ref } from 'vue'
import type { Tokens } from '../../api'
import { modelLabel, type PricedDailyAgg as DailyAgg, type PricedHourCell as HourCell, type PricedModelAgg as ModelAgg } from '../../lib/pricing'
import EChart from '../EChart.vue'
import type { EChartsOption } from '../../lib/echarts'
import { axisBase, tooltipBase, ttRow, ttTitle, useChartTheme } from '../../lib/chartTheme'
import { fmtBig, fmtMoney, fmtMoneyExact, fmtMs, pad } from '../../lib/format'
import { addTokens, bucketOf, chartSummary, type DateRange, dateRange, type Gran, granOf, localDate, modelPalette, OTHER, spanOf, zeroTokens } from '../../lib/summaryModel'
import DashCard from './DashCard.vue'
import Segmented from '../Segmented.vue'

const props = defineProps<{
  daily: DailyAgg[]
  /** 星期 × 小时格；范围只有一天时它就是这一天的 24 小时，最近 24 小时里每个（星期, 小时）也只出现一次 */
  hourly: HourCell[]
  byModel: Record<string, ModelAgg>
  range: DateRange
  bounds: { min: string | null; max: string | null }
}>()
const t = useChartTheme()

type Mode = 'cost' | 'tokens' | 'time'
const mode = ref<Mode>('cost')
const MODES: { key: Mode; label: string }[] = [
  { key: 'cost', label: '费用 · 按模型' },
  { key: 'tokens', label: 'tokens' },
  { key: 'time', label: '时间' },
]
const MODE_OPTIONS = MODES.map((m) => ({ value: m.key, label: m.label }))

/** 选中范围内每一天（单边时另一边用数据边界补），空白天也占位 */
const dates = computed(() => {
  const span = spanOf(props.range, props.bounds, props.daily)
  return span ? dateRange(...span) : []
})
const byDate = computed(() => new Map(props.daily.map((d) => [d.date, d])))
const palette = computed(() => modelPalette(props.byModel, t.value))

/** 最近 24 小时跨两个日期，但按日只有两根柱子：改按小时铺这段窗口 */
const rolling24 = computed(() => props.range.preset === '24h')
const TITLE: Record<Gran, string> = { hour: '当日分时', day: '每日趋势', week: '每周趋势', month: '每月趋势' }

const gran = computed(() => (rolling24.value ? 'hour' : granOf(dates.value.length)))
const title = computed(() => (rolling24.value ? '最近 24 小时分时' : TITLE[gran.value]))

/** 一根柱子的数据：日 / 周 / 月由若干天加总，分时由小时格来 */
interface Point {
  /** 模型 → 费用 */
  cost: Record<string, number>
  tokens: Tokens
  model_ms: number
  tool_ms: number
}
interface Bucket {
  /** x 轴类目 */
  key: string
  label: string
  /** tooltip 标题 */
  title: string
  point: Point
}

function emptyPoint(): Point {
  return { cost: {}, tokens: zeroTokens(), model_ms: 0, tool_ms: 0 }
}

function addDay(p: Point, d: DailyAgg) {
  for (const [m, a] of Object.entries(d.by_model)) p.cost[m] = (p.cost[m] ?? 0) + a.cost_usd
  addTokens(p.tokens, d.tokens)
  p.model_ms += d.model_ms
  p.tool_ms += d.tool_ms
}

function addHour(p: Point, c: HourCell) {
  for (const [m, x] of Object.entries(c.model_cost)) p.cost[m] = (p.cost[m] ?? 0) + x
  for (const tk of Object.values(c.by_model)) addTokens(p.tokens, tk)
  p.model_ms += c.model_ms
  p.tool_ms += c.tool_ms
}

const buckets = computed<Bucket[]>(() => {
  const g = gran.value
  if (g === 'hour' && rolling24.value) {
    // 从 since 所在的小时铺到此刻所在的小时（首尾两格只含窗口里的部分），共 25 格
    const start = new Date(props.range.since!)
    start.setMinutes(0, 0, 0)
    const out: (Bucket & { wd: number; h: number })[] = Array.from({ length: 25 }, (_, k) => {
      const d = new Date(start.getTime() + k * 3_600_000)
      const h = d.getHours()
      return {
        key: d.toISOString(),
        label: pad(h),
        title: `${localDate(d)} ${pad(h)}:00–${pad((h + 1) % 24)}:00`,
        point: emptyPoint(),
        wd: (d.getDay() + 6) % 7,
        h,
      }
    })
    const at = new Map(out.map((b) => [`${b.wd},${b.h}`, b]))
    for (const c of props.hourly) {
      const b = at.get(`${c.weekday},${c.hour}`)
      if (b) addHour(b.point, c)
    }
    return out
  }
  if (g === 'hour') {
    const date = dates.value[0]!
    const weekday = (new Date(`${date}T00:00:00`).getDay() + 6) % 7
    const out: Bucket[] = Array.from({ length: 24 }, (_, h) => ({
      key: String(h),
      label: pad(h),
      title: `${date} ${pad(h)}:00–${pad(h + 1)}:00`,
      point: emptyPoint(),
    }))
    for (const c of props.hourly) if (c.weekday === weekday) addHour(out[c.hour]!.point, c)
    return out
  }
  // 首尾的周 / 月可能被范围截断：只加范围内的天，标题也只写这几天
  const out: (Bucket & { first: string; last: string })[] = []
  for (const day of dates.value) {
    const key = bucketOf(day, g)
    let b = out[out.length - 1]
    if (b?.key !== key) {
      b = { key, label: g === 'month' ? key : key.slice(5), title: key, first: day, last: day, point: emptyPoint() }
      out.push(b)
    }
    b.last = day
    const d = byDate.value.get(day)
    if (d) addDay(b.point, d)
  }
  if (g === 'week') for (const b of out) b.title = b.first === b.last ? b.first : `${b.first} – ${b.last.slice(5)}`
  return out
})

interface Series {
  name: string
  color: string
  values: number[]
}

const series = computed<Series[]>(() => {
  const ps = buckets.value.map((b) => b.point)
  if (mode.value === 'cost') {
    const p = palette.value
    // 未定价 / 费用为 0 的模型在费用模式下永远是 0，不进图例；颜色槽位不变
    const out: Series[] = p.slots.filter((m) => (props.byModel[m]?.cost_usd ?? 0) > 0).map((m) => ({
      name: m,
      color: p.color(m),
      values: ps.map((x) => x.cost[m] ?? 0),
    }))
    if (p.folded.length) {
      out.push({
        name: OTHER,
        color: t.value.other,
        values: ps.map((x) => p.folded.reduce((a, m) => a + (x.cost[m] ?? 0), 0)),
      })
    }
    return out
  }
  if (mode.value === 'tokens') {
    return [
      { name: '输入', color: t.value.cat[0]!, values: ps.map((x) => x.tokens.input) },
      { name: '输出', color: t.value.cat[1]!, values: ps.map((x) => x.tokens.output) },
      { name: '缓存读取', color: t.value.cat[2]!, values: ps.map((x) => x.tokens.cache_read) },
      { name: '缓存写入', color: t.value.cat[3]!, values: ps.map((x) => x.tokens.cache_write) },
    ]
  }
  return [
    { name: '模型', color: t.value.cat[0]!, values: ps.map((x) => x.model_ms) },
    { name: '工具', color: t.value.cat[1]!, values: ps.map((x) => x.tool_ms) },
  ]
})

const ariaLabel = computed(() =>
  chartSummary(
    `${title.value} · ${MODES.find((m) => m.key === mode.value)?.label}`,
    buckets.value.map((b, i) => ({ label: b.title, v: series.value.reduce((a, s) => a + (s.values[i] ?? 0), 0) })),
    '个区间有数据',
    fmtExact.value,
  ),
)

const fmt = computed(() => (mode.value === 'cost' ? fmtMoney : mode.value === 'tokens' ? fmtBig : fmtMs))

/** 时间模式下刻度按整分钟 / 整小时走，不然 ECharts 会给出 8h 20m 这种刻度 */
const hourInterval = computed<number | undefined>(() => {
  if (mode.value !== 'time') return undefined
  const n = buckets.value.length
  let max = 0
  for (let i = 0; i < n; i++) max = Math.max(max, series.value.reduce((a, s) => a + (s.values[i] ?? 0), 0))
  const hours = max / 3.6e6
  const step = [1 / 12, 1 / 6, 0.25, 0.5, 1, 2, 5, 10, 20, 50, 100, 200, 500].find((x) => hours / x <= 6) ?? 1000
  return step * 3.6e6
})
/** 时间轴刻度：刻度间隔不到一小时就按分钟写 */
function fmtAxisMs(v: number): string {
  if (v === 0) return '0'
  if ((hourInterval.value ?? 3.6e6) < 3.6e6) return `${Math.round(v / 6e4)}m`
  return `${+(v / 3.6e6).toFixed(2)}h`
}
const fmtExact = computed(() => (mode.value === 'cost' ? fmtMoneyExact : mode.value === 'tokens' ? fmtBig : fmtMs))

const option = computed<EChartsOption>(() => {
  const th = t.value
  const ss = series.value
  const ax = axisBase(th)
  const topOf = buckets.value.map((_, di) => {
    for (let si = ss.length - 1; si >= 0; si--) if ((ss[si]!.values[di] ?? 0) > 0) return si
    return -1
  })
  return {
    tooltip: {
      ...tooltipBase(th),
      trigger: 'axis',
      axisPointer: { type: 'shadow', shadowStyle: { color: th.fg, opacity: 0.06 } },
      formatter: (ps: unknown) => {
        const arr = ps as { seriesName: string; value: number; color: string; dataIndex: number }[]
        if (!arr.length) return ''
        const rows = arr.filter((p) => p.value > 0).sort((a, b) => b.value - a.value)
        const total = arr.reduce((a, p) => a + p.value, 0)
        return (
          ttTitle(buckets.value[arr[0]!.dataIndex]?.title ?? '') +
          rows.map((p) => ttRow(p.color, modelLabel(p.seriesName), fmtExact.value(p.value))).join('') +
          (rows.length > 1 ? `<hr style="border:0;border-top:1px solid ${th.line};margin:4px 0">` + ttRow(null, '合计', fmtExact.value(total)) : '')
        )
      },
    },
    grid: { left: 4, right: 4, top: 8, bottom: 4, containLabel: true },
    xAxis: {
      type: 'category',
      data: buckets.value.map((b) => b.key),
      ...ax,
      splitLine: { show: false },
      // 分时是 00–23，日 / 周是 MM-DD（周是周一），月是 YYYY-MM
      axisLabel: { ...ax.axisLabel, formatter: (_: string, i: number) => buckets.value[i]?.label ?? '', hideOverlap: true },
    },
    yAxis: {
      type: 'value',
      ...ax,
      axisLine: { show: false },
      interval: hourInterval.value,
      axisLabel: {
        ...ax.axisLabel,
        formatter: (v: number) => (mode.value === 'time' ? fmtAxisMs(v) : fmt.value(v)),
      },
    },
    // 圆角只给最上面一段（每天第一个非零的从上往下数），不然堆叠的每段都各自圆一下
    series: ss.map((s, si) => ({
      type: 'bar',
      name: s.name,
      stack: 'total',
      data: s.values.map((v, di) => ({ value: v, itemStyle: topOf[di] === si ? { borderRadius: [3, 3, 0, 0] } : undefined })),
      barMaxWidth: 22,
      barCategoryGap: '35%',
      itemStyle: { color: s.color, borderColor: th.surface, borderWidth: 1 },
    })),
  }
})
</script>

<template>
  <DashCard icon="i-jannchie-chart-column" :title="title">
    <template #controls>
      <Segmented v-model="mode" label="趋势指标" :options="MODE_OPTIONS" />
    </template>
    <ul class="m-0 mb-2 flex list-none flex-wrap gap-x-4 gap-y-1 p-0 text-xs ink-muted">
      <li v-for="s in series" :key="s.name" class="flex items-center gap-1.5" :title="s.name">
        <span class="h-2 w-2 shrink-0 rounded-full" :style="{ background: s.color }"></span>
        {{ modelLabel(s.name) }}
      </li>
    </ul>
    <EChart :option="option" :height="240" :label="ariaLabel" />
  </DashCard>
</template>
