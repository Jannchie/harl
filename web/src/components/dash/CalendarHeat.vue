<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import type { PricedDailyAgg as DailyAgg } from '../../lib/pricing'
import EChart from '../EChart.vue'
import type { EChartsOption } from '../../lib/echarts'
import { tooltipBase, ttRow, ttTitle, useChartTheme } from '../../lib/chartTheme'
import { fmtBig, fmtInt, fmtMoney, fmtMoneyExact, fmtMs } from '../../lib/format'
import { chartSummary, type DateRange, localDate, totalTokens } from '../../lib/summaryModel'
import DashCard from './DashCard.vue'
import Segmented from '../Segmented.vue'

const props = defineProps<{
  daily: DailyAgg[]
  /** 选中的范围（本地日期，可单边） */
  range: DateRange
  /** 全部数据的本地日期边界 */
  bounds: { min: string | null; max: string | null }
}>()
const t = useChartTheme()

// 格子要是正方形：ECharts 的 cellSize 'auto' 是宽高各自铺满，比例跟着容器走，所以自己按容器宽度算边长
const CAL_LEFT = 34
const CAL_RIGHT = 12
const CAL_TOP = 22
const CAL_BOTTOM = 4
const CELL_MAX = 24
const wrap = ref<HTMLDivElement | null>(null)
const wrapWidth = ref(0)
let ro: ResizeObserver | null = null
onMounted(() => {
  if (!wrap.value) return
  wrapWidth.value = wrap.value.clientWidth
  ro = new ResizeObserver(([e]) => (wrapWidth.value = e?.contentRect.width ?? 0))
  ro.observe(wrap.value)
})
onBeforeUnmount(() => ro?.disconnect())

type Metric = 'cost' | 'wall_ms' | 'tokens'
const metric = ref<Metric>('cost')
const METRICS: { key: Metric; label: string; pick: (d: DailyAgg) => number; fmt: (v: number) => string }[] = [
  { key: 'cost', label: '费用', pick: (d) => d.cost_usd, fmt: fmtMoney },
  { key: 'wall_ms', label: '墙钟时间', pick: (d) => d.wall_ms, fmt: fmtMs },
  { key: 'tokens', label: 'tokens', pick: (d) => totalTokens(d.tokens), fmt: fmtBig },
]
const METRIC_OPTIONS = METRICS.map((m) => ({ value: m.key, label: m.label }))
const cur = computed(() => METRICS.find((m) => m.key === metric.value) ?? METRICS[0]!)

const byDate = computed(() => new Map(props.daily.map((d) => [d.date, d])))

const ariaLabel = computed(() => {
  const m = cur.value
  return chartSummary(`日历 · ${m.label}`, props.daily.map((d) => ({ label: d.date, v: m.pick(d) })), '天有活动', m.fmt)
})

function monthStart(date: string): string {
  return `${date.slice(0, 7)}-01`
}
function monthEnd(date: string): string {
  const d = new Date(`${date}T00:00:00`)
  return localDate(new Date(d.getFullYear(), d.getMonth() + 1, 0))
}
function daysBetween(a: string, b: string): number {
  return Math.round((new Date(`${b}T00:00:00`).getTime() - new Date(`${a}T00:00:00`).getTime()) / 86400e3)
}

/**
 * 日历跨度：选中范围 ≤ 1 年就按范围显示（补成整月）；
 * 全部 / 超过一年时显示最近 12 个月（到今天，起点对齐周一）
 */
const calRange = computed<[string, string]>(() => {
  const today = localDate(new Date())
  const from = props.range.from ?? props.bounds.min
  const to = props.range.to ?? props.bounds.max ?? today
  const isAll = !props.range.from && !props.range.to
  if (from && to && !isAll && daysBetween(from, to) <= 366) {
    return [monthStart(from), monthEnd(to)]
  }
  const end = new Date(`${today}T00:00:00`)
  const start = new Date(end)
  start.setDate(start.getDate() - 364)
  const dow = (start.getDay() + 6) % 7
  start.setDate(start.getDate() - dow)
  return [localDate(start), today]
})

const calWeeks = computed(() => Math.ceil(daysBetween(...calRange.value) / 7) + 1)
/** 正方形格子的边长：铺满可用宽度，但不超过 CELL_MAX；容器还没量到时先按最大值 */
const cell = computed(() => {
  const avail = wrapWidth.value - CAL_LEFT - CAL_RIGHT
  return avail > 0 ? Math.max(8, Math.min(CELL_MAX, Math.floor(avail / calWeeks.value))) : CELL_MAX
})

const option = computed<EChartsOption>(() => {
  const th = t.value
  const m = cur.value
  const data = props.daily.map((d) => [d.date, m.pick(d)] as [string, number])
  const max = Math.max(1e-9, ...data.map((x) => x[1]))
  return {
    tooltip: {
      ...tooltipBase(th),
      formatter: (p: unknown) => {
        const v = (p as { value: [string, number] }).value
        const d = byDate.value.get(v[0])
        if (!d) return ttTitle(v[0])
        return (
          ttTitle(v[0]) +
          ttRow(null, m.label, m.fmt(v[1])) +
          `<hr style="border:0;border-top:1px solid ${th.line};margin:4px 0">` +
          ttRow(null, 'turns / steps', `${fmtInt(d.turns)} / ${fmtInt(d.steps)}`, true) +
          ttRow(null, '工具调用', fmtInt(d.tool_calls), true) +
          ttRow(null, '费用', fmtMoneyExact(d.cost_usd), true) +
          ttRow(null, 'tokens in / out', `${fmtBig(d.tokens.input)} / ${fmtBig(d.tokens.output)}`, true) +
          ttRow(null, '缓存读取', fmtBig(d.tokens.cache_read), true) +
          ttRow(null, '模型 / 工具 / 墙钟', `${fmtMs(d.model_ms)} / ${fmtMs(d.tool_ms)} / ${fmtMs(d.wall_ms)}`, true)
        )
      },
    },
    // 只负责上色，不显示图例：深浅本身就够看，具体数值看 tooltip
    visualMap: { type: 'continuous', min: 0, max, show: false, inRange: { color: th.seq } },
    calendar: {
      top: CAL_TOP,
      left: CAL_LEFT,
      bottom: CAL_BOTTOM,
      cellSize: [cell.value, cell.value],
      range: calRange.value,
      itemStyle: { color: th.empty, borderColor: th.surface, borderWidth: 3, borderRadius: 3 },
      splitLine: { show: false },
      yearLabel: { show: false },
      monthLabel: { color: th.fg2, fontSize: 10, fontFamily: th.fontFamily, nameMap: 'ZH', margin: 6 },
      dayLabel: {
        color: th.fg2,
        fontSize: 10,
        fontFamily: th.fontFamily,
        firstDay: 1,
        nameMap: ['日', '一', '二', '三', '四', '五', '六'],
        margin: 6,
      },
    },
    series: [
      {
        type: 'heatmap',
        coordinateSystem: 'calendar',
        data,
        itemStyle: { borderColor: th.surface, borderWidth: 3, borderRadius: 3 },
        emphasis: { itemStyle: { borderColor: th.fg2, borderWidth: 1 } },
      },
    ],
  }
})
</script>

<template>
  <DashCard icon="i-jannchie-calendar" title="日历">
    <template #controls>
      <Segmented v-model="metric" label="日历指标" :options="METRIC_OPTIONS" />
    </template>
    <div ref="wrap"><EChart :option="option" :height="CAL_TOP + CAL_BOTTOM + cell * 7" :label="ariaLabel" /></div>
  </DashCard>
</template>
