<script setup lang="ts">
import { computed, ref } from 'vue'
import EChart from '../EChart.vue'
import type { EChartsOption } from '../../lib/echarts'
import { axisBase, tooltipBase, ttRow, ttTitle, useChartTheme } from '../../lib/chartTheme'
import { fmtInt, fmtMoney, fmtMoneyExact } from '../../lib/format'
import { WEEKDAYS } from '../../lib/summaryModel'
import DashCard from './DashCard.vue'

const props = defineProps<{ hourlyTurns: number[][]; hourlyCost: number[][] }>()
const t = useChartTheme()

type Metric = 'turns' | 'cost'
const metric = ref<Metric>('turns')

const grid = computed(() => (metric.value === 'turns' ? props.hourlyTurns : props.hourlyCost))
const fmt = computed(() => (metric.value === 'turns' ? fmtInt : fmtMoney))
const fmtExact = computed(() => (metric.value === 'turns' ? fmtInt : fmtMoneyExact))

const peak = computed(() => {
  let best = { d: 0, h: 0, v: -1 }
  grid.value.forEach((row, d) => row.forEach((v, h) => v > best.v && (best = { d, h, v })))
  return best.v > 0 ? best : null
})

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
        const turns = props.hourlyTurns[d]?.[h] ?? 0
        const cost = props.hourlyCost[d]?.[h] ?? 0
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
      axisLabel: { ...ax.axisLabel, interval: 2, fontFamily: th.monoFamily, formatter: (v: string) => `${v.padStart(2, '0')}` },
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
      textStyle: { color: th.fg2, fontSize: 10, fontFamily: th.monoFamily },
    },
    series: [
      {
        type: 'heatmap',
        data,
        itemStyle: { borderColor: th.bg, borderWidth: 2 },
        emphasis: { itemStyle: { borderColor: th.fg, borderWidth: 1 } },
      },
    ],
  }
})
</script>

<template>
  <DashCard n="06" title="星期 × 小时" :caption="peak ? `峰值 ${WEEKDAYS[peak.d]} ${String(peak.h).padStart(2, '0')}:00 · ${fmt(peak.v)}` : '本地时区'">
    <template #controls>
      <button type="button" :class="{ active: metric === 'turns' }" @click="metric = 'turns'">turns</button>
      <button type="button" :class="{ active: metric === 'cost' }" @click="metric = 'cost'">费用</button>
    </template>
    <EChart :option="option" :height="230" />
  </DashCard>
</template>
