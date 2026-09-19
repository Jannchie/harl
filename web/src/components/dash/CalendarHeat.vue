<script setup lang="ts">
import { computed, ref } from 'vue'
import type { DailyAgg } from '../../api'
import EChart from '../EChart.vue'
import type { EChartsOption } from '../../lib/echarts'
import { tooltipBase, ttRow, ttTitle, useChartTheme } from '../../lib/chartTheme'
import { fmtBig, fmtInt, fmtMoney, fmtMoneyExact, fmtMs } from '../../lib/format'
import { localDate } from '../../lib/summaryModel'
import DashCard from './DashCard.vue'

const props = defineProps<{ daily: DailyAgg[] }>()
const t = useChartTheme()

type Metric = 'cost' | 'turns' | 'output' | 'model_ms'
const metric = ref<Metric>('cost')
const METRICS: { key: Metric; label: string; pick: (d: DailyAgg) => number; fmt: (v: number) => string }[] = [
  { key: 'cost', label: '费用', pick: (d) => d.cost_usd, fmt: fmtMoney },
  { key: 'turns', label: 'turns', pick: (d) => d.turns, fmt: fmtInt },
  { key: 'output', label: '输出 tokens', pick: (d) => d.tokens.output, fmt: fmtBig },
  { key: 'model_ms', label: '模型时间', pick: (d) => d.model_ms, fmt: fmtMs },
]
const cur = computed(() => METRICS.find((m) => m.key === metric.value) ?? METRICS[0]!)

const byDate = computed(() => new Map(props.daily.map((d) => [d.date, d])))

/** 过去一年：今天往前 52 周，对齐到周一开头 */
const range = computed<[string, string]>(() => {
  const end = new Date()
  end.setHours(0, 0, 0, 0)
  const start = new Date(end)
  start.setDate(start.getDate() - 364)
  // 往前推到周一，让第一列完整
  const dow = (start.getDay() + 6) % 7
  start.setDate(start.getDate() - dow)
  return [localDate(start), localDate(end)]
})

const activeDays = computed(() => props.daily.filter((d) => cur.value.pick(d) > 0).length)

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
    visualMap: {
      type: 'continuous',
      min: 0,
      max,
      show: true,
      orient: 'horizontal',
      left: 'center',
      bottom: 0,
      itemWidth: 8,
      itemHeight: 90,
      calculable: false,
      hoverLink: false,
      inRange: { color: th.seq },
      text: [m.fmt(max), '0'],
      textStyle: { color: th.fg2, fontSize: 10, fontFamily: th.monoFamily },
    },
    calendar: {
      top: 22,
      left: 34,
      right: 12,
      bottom: 26,
      cellSize: ['auto', 15],
      range: range.value,
      itemStyle: { color: th.bg1, borderColor: th.bg, borderWidth: 2 },
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
        itemStyle: { borderColor: th.bg, borderWidth: 2 },
        emphasis: { itemStyle: { borderColor: th.fg, borderWidth: 1 } },
      },
    ],
  }
})
</script>

<template>
  <DashCard n="02" title="过去一年" :caption="`按本地日期分桶 · ${activeDays} 天有活动`">
    <template #controls>
      <button
        v-for="m in METRICS"
        :key="m.key"
        type="button"
        :class="{ active: metric === m.key }"
        @click="metric = m.key"
      >
        {{ m.label }}
      </button>
    </template>
    <EChart :option="option" :height="186" />
  </DashCard>
</template>
