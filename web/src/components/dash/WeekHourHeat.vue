<script setup lang="ts">
import { computed, ref } from 'vue'
import EChart from '../EChart.vue'
import type { EChartsOption } from '../../lib/echarts'
import { axisBase, tooltipBase, ttRow, ttTitle, useChartTheme } from '../../lib/chartTheme'
import { fmtMoney, fmtMoneyExact, fmtMs, pad } from '../../lib/format'
import type { PricedHourCell } from '../../lib/pricing'
import { chartSummary, WEEKDAYS } from '../../lib/summaryModel'
import DashCard from './DashCard.vue'
import Segmented from '../Segmented.vue'

const props = defineProps<{ hourly: PricedHourCell[] }>()
const t = useChartTheme()

type Metric = 'cost' | 'wall_ms'
const metric = ref<Metric>('cost')

/** 稀疏格子铺成 [周一..周日][0..23] */
function dense(pick: (c: PricedHourCell) => number): number[][] {
  const g = Array.from({ length: 7 }, () => Array<number>(24).fill(0))
  for (const c of props.hourly) g[c.weekday]![c.hour] = pick(c)
  return g
}
const METRICS: { key: Metric; label: string; pick: (c: PricedHourCell) => number; fmt: (v: number) => string; fmtExact: (v: number) => string }[] = [
  { key: 'cost', label: '费用', pick: (c) => c.cost_usd, fmt: fmtMoney, fmtExact: fmtMoneyExact },
  { key: 'wall_ms', label: '墙钟时间', pick: (c) => c.wall_ms, fmt: fmtMs, fmtExact: fmtMs },
]
const METRIC_OPTIONS = METRICS.map((m) => ({ value: m.key, label: m.label }))
/** 当前指标和 tooltip 里顺带显示的另一个 */
const cur = computed(() => METRICS.find((m) => m.key === metric.value) ?? METRICS[0]!)
const other = computed(() => METRICS.find((m) => m.key !== metric.value)!)
const grids = computed(() => Object.fromEntries(METRICS.map((m) => [m.key, dense(m.pick)])) as Record<Metric, number[][]>)
const grid = computed(() => grids.value[metric.value])

const peak = computed(() => {
  let best = { d: 0, h: 0, v: -1 }
  grid.value.forEach((row, d) => row.forEach((v, h) => v > best.v && (best = { d, h, v })))
  return best.v > 0 ? best : null
})

/** 「周三 14:00」 */
const slotName = (d: number, h: number) => `${WEEKDAYS[d]} ${pad(h)}:00`

const ariaLabel = computed(() =>
  chartSummary(
    `星期 × 小时 · ${cur.value.label}`,
    grid.value.flatMap((row, d) => row.map((v, h) => ({ label: slotName(d, h), v }))),
    '个时段有活动',
    cur.value.fmtExact,
  ),
)

const option = computed<EChartsOption>(() => {
  const th = t.value
  const ax = axisBase(th)
  const data: [number, number, number][] = []
  let max = 0
  grid.value.forEach((row, d) =>
    row.forEach((v, h) => {
      data.push([h, d, v])
      if (v > max) max = v
    }),
  )
  return {
    tooltip: {
      ...tooltipBase(th),
      position: 'top',
      formatter: (p: unknown) => {
        const [h, d, v] = (p as { value: [number, number, number] }).value
        return (
          ttTitle(`${WEEKDAYS[d]} ${String(h).padStart(2, '0')}:00–${String(h + 1).padStart(2, '0')}:00`) +
          ttRow(null, cur.value.label, cur.value.fmtExact(v)) +
          ttRow(null, other.value.label, other.value.fmtExact(grids.value[other.value.key][d]?.[h] ?? 0), true)
        )
      },
    },
    grid: { left: 8, right: 48, top: 8, bottom: 4, containLabel: true },
    xAxis: {
      type: 'category',
      data: Array.from({ length: 24 }, (_, i) => i),
      ...ax,
      splitLine: { show: false },
      axisLine: { show: false },
      axisLabel: { ...ax.axisLabel, interval: 2, formatter: (v: string) => `${v.padStart(2, '0')}` },
    },
    yAxis: {
      type: 'category',
      data: WEEKDAYS,
      inverse: true,
      ...ax,
      splitLine: { show: false },
      axisLine: { show: false },
    },
    visualMap: {
      type: 'continuous',
      min: 0,
      max: Math.max(max, 1e-9),
      calculable: false,
      hoverLink: false,
      orient: 'vertical',
      right: 0,
      top: 'middle',
      itemWidth: 8,
      itemHeight: 110,
      inRange: { color: th.seq },
      text: [cur.value.fmt(max), '0'],
      textStyle: { color: th.fg2, fontSize: 10, fontFamily: th.fontFamily },
    },
    series: [
      {
        type: 'heatmap',
        data,
        itemStyle: { borderColor: th.surface, borderWidth: 2, borderRadius: 3 },
        emphasis: { itemStyle: { borderColor: th.fg2, borderWidth: 1 } },
      },
    ],
  }
})
</script>

<template>
  <DashCard icon="i-carbon-grid" title="星期 × 小时" :caption="peak ? `峰值 ${slotName(peak.d, peak.h)} · ${cur.fmt(peak.v)}` : undefined">
    <template #controls>
      <Segmented v-model="metric" label="热图指标" :options="METRIC_OPTIONS" />
    </template>
    <EChart :option="option" :height="230" :label="ariaLabel" />
  </DashCard>
</template>
