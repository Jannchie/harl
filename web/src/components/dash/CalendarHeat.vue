<script setup lang="ts">
import { computed, ref } from 'vue'
import type { DailyAgg } from '../../api'
import EChart from '../EChart.vue'
import type { EChartsOption } from '../../lib/echarts'
import { tooltipBase, ttRow, ttTitle, useChartTheme } from '../../lib/chartTheme'
import { fmtBig, fmtInt, fmtMoney, fmtMoneyExact, fmtMs } from '../../lib/format'
import { type DateRange, localDate } from '../../lib/summaryModel'
import DashCard from './DashCard.vue'

const props = defineProps<{
  daily: DailyAgg[]
  /** 选中的范围（本地日期，可单边） */
  range: DateRange
  /** 全部数据的本地日期边界 */
  bounds: { min: string | null; max: string | null }
}>()
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
const calRange = computed<{ range: [string, string]; label: string }>(() => {
  const today = localDate(new Date())
  const from = props.range.from ?? props.bounds.min
  const to = props.range.to ?? props.bounds.max ?? today
  const isAll = !props.range.from && !props.range.to
  if (from && to && !isAll && daysBetween(from, to) <= 366) {
    return { range: [monthStart(from), monthEnd(to)], label: `${monthStart(from).slice(0, 7)} → ${monthEnd(to).slice(0, 7)}` }
  }
  const end = new Date(`${today}T00:00:00`)
  const start = new Date(end)
  start.setDate(start.getDate() - 364)
  const dow = (start.getDay() + 6) % 7
  start.setDate(start.getDate() - dow)
  return { range: [localDate(start), today], label: '最近 12 个月' }
})

const calWeeks = computed(() => Math.ceil(daysBetween(calRange.value.range[0], calRange.value.range[1]) / 7) + 1)

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
      left: calWeeks.value <= 26 ? 40 : 'center',
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
      // left + right 同时给的话 ECharts 会无视 cellSize 的宽度，短范围只给 left
      right: calWeeks.value <= 26 ? undefined : 12,
      bottom: 26,
      // 短范围不要把格子横向拉成长条：半年以内固定宽度，更长才自适应铺满
      cellSize: [calWeeks.value <= 26 ? 24 : 'auto', 15],
      range: calRange.value.range,
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
  <DashCard n="02" title="日历" :caption="`${calRange.label} · 按本地日期分桶 · ${activeDays} 天有活动`">
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
