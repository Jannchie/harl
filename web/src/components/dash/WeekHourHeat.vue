<script setup lang="ts">
import { computed, ref } from 'vue'
import EChart from '../EChart.vue'
import type { EChartsOption } from '../../lib/echarts'
import { axisBase, tooltipBase, ttRow, ttTitle, useChartTheme } from '../../lib/chartTheme'
import { fmtInt, fmtMoney, fmtMoneyExact, pad } from '../../lib/format'
import type { PricedHourCell } from '../../lib/pricing'
import { chartSummary, WEEKDAYS } from '../../lib/summaryModel'
import DashCard from './DashCard.vue'
import Segmented from '../Segmented.vue'

const props = defineProps<{ hourly: PricedHourCell[] }>()
const t = useChartTheme()

type Metric = 'turns' | 'cost'
const metric = ref<Metric>('turns')

/** 稀疏格子铺成 [周一..周日][0..23] */
function dense(pick: (c: PricedHourCell) => number): number[][] {
  const g = Array.from({ length: 7 }, () => Array<number>(24).fill(0))
  for (const c of props.hourly) g[c.weekday]![c.hour] = pick(c)
  return g
}
const hourlyTurns = computed(() => dense((c) => c.turns))
const hourlyCost = computed(() => dense((c) => c.cost_usd))
const grid = computed(() => (metric.value === 'turns' ? hourlyTurns.value : hourlyCost.value))
const fmt = computed(() => (metric.value === 'turns' ? fmtInt : fmtMoney))
const fmtExact = computed(() => (metric.value === 'turns' ? fmtInt : fmtMoneyExact))

const METRIC_OPTIONS = [
  { value: 'turns', label: 'turns' },
  { value: 'cost', label: '费用' },
] as const

const peak = computed(() => {
  let best = { d: 0, h: 0, v: -1 }
  grid.value.forEach((row, d) => row.forEach((v, h) => v > best.v && (best = { d, h, v })))
  return best.v > 0 ? best : null
})

/** 「周三 14:00」 */
const slotName = (d: number, h: number) => `${WEEKDAYS[d]} ${pad(h)}:00`

const ariaLabel = computed(() =>
  chartSummary(
    `星期 × 小时 · ${metric.value === 'cost' ? '费用' : 'turns'}`,
    grid.value.flatMap((row, d) => row.map((v, h) => ({ label: slotName(d, h), v }))),
    '个时段有活动',
    fmtExact.value,
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
        const turns = hourlyTurns.value[d]?.[h] ?? 0
        const cost = hourlyCost.value[d]?.[h] ?? 0
        return (
          ttTitle(`${WEEKDAYS[d]} ${String(h).padStart(2, '0')}:00–${String(h + 1).padStart(2, '0')}:00`) +
          ttRow(null, metric.value === 'turns' ? 'turns' : '费用', fmtExact.value(v)) +
          (metric.value === 'turns' ? ttRow(null, '费用', fmtMoneyExact(cost), true) : ttRow(null, 'turns', fmtInt(turns), true))
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
      text: [fmt.value(max), '0'],
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
  <DashCard icon="i-carbon-grid" title="星期 × 小时" :caption="peak ? `峰值 ${slotName(peak.d, peak.h)} · ${fmt(peak.v)}` : undefined">
    <template #controls>
      <Segmented v-model="metric" label="热图指标" :options="METRIC_OPTIONS" />
    </template>
    <EChart :option="option" :height="230" :label="ariaLabel" />
  </DashCard>
</template>
