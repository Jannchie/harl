<script setup lang="ts">
import { computed, ref } from 'vue'
import type { DailyAgg, ModelAgg } from '../../api'
import EChart from '../EChart.vue'
import type { EChartsOption } from '../../lib/echarts'
import { axisBase, tooltipBase, ttRow, ttTitle, useChartTheme } from '../../lib/chartTheme'
import { fmtBig, fmtMoney, fmtMoneyExact, fmtMs } from '../../lib/format'
import { type DateRange, dateRange, modelPalette, OTHER, shortModel } from '../../lib/summaryModel'
import DashCard from './DashCard.vue'

const props = defineProps<{
  daily: DailyAgg[]
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

/** x 轴：选中范围内每一天（单边时另一边用数据边界补），空白天也占位 */
const dates = computed(() => {
  const first = props.range.from ?? props.bounds.min ?? props.daily[0]?.date
  const last = props.range.to ?? props.bounds.max ?? props.daily[props.daily.length - 1]?.date
  if (!first || !last || first > last) return []
  return dateRange(first, last)
})
const byDate = computed(() => new Map(props.daily.map((d) => [d.date, d])))
const palette = computed(() => modelPalette(props.byModel, t.value))

interface Series {
  name: string
  color: string
  values: number[]
}

const series = computed<Series[]>(() => {
  const ds = dates.value.map((d) => byDate.value.get(d))
  if (mode.value === 'cost') {
    const p = palette.value
    // 未定价 / 费用为 0 的模型在费用模式下永远是 0，不进图例；颜色槽位不变
    const out: Series[] = p.slots.filter((m) => (props.byModel[m]?.cost_usd ?? 0) > 0).map((m) => ({
      name: m,
      color: p.color(m),
      values: ds.map((d) => d?.by_model[m]?.cost_usd ?? 0),
    }))
    if (p.folded.length) {
      out.push({
        name: OTHER,
        color: t.value.other,
        values: ds.map((d) => p.folded.reduce((a, m) => a + (d?.by_model[m]?.cost_usd ?? 0), 0)),
      })
    }
    return out
  }
  if (mode.value === 'tokens') {
    return [
      { name: '输入', color: t.value.cat[0]!, values: ds.map((d) => d?.tokens.input ?? 0) },
      { name: '输出', color: t.value.cat[1]!, values: ds.map((d) => d?.tokens.output ?? 0) },
      { name: '缓存读取', color: t.value.cat[2]!, values: ds.map((d) => d?.tokens.cache_read ?? 0) },
      { name: '缓存写入', color: t.value.cat[3]!, values: ds.map((d) => d?.tokens.cache_write ?? 0) },
    ]
  }
  return [
    { name: '模型', color: t.value.cat[0]!, values: ds.map((d) => d?.model_ms ?? 0) },
    { name: '工具', color: t.value.cat[1]!, values: ds.map((d) => d?.tool_ms ?? 0) },
  ]
})

const fmt = computed(() => (mode.value === 'cost' ? fmtMoney : mode.value === 'tokens' ? fmtBig : fmtMs))

/** 时间模式下刻度按整小时走，不然 ECharts 会给出 8h 20m 这种刻度 */
const hourInterval = computed<number | undefined>(() => {
  if (mode.value !== 'time') return undefined
  const n = dates.value.length
  let max = 0
  for (let i = 0; i < n; i++) max = Math.max(max, series.value.reduce((a, s) => a + (s.values[i] ?? 0), 0))
  const hours = max / 3.6e6
  const step = [0.25, 0.5, 1, 2, 5, 10, 20, 50, 100, 200, 500].find((x) => hours / x <= 6) ?? 1000
  return step * 3.6e6
})
const fmtExact = computed(() => (mode.value === 'cost' ? fmtMoneyExact : mode.value === 'tokens' ? fmtBig : fmtMs))

const option = computed<EChartsOption>(() => {
  const th = t.value
  const ss = series.value
  const ax = axisBase(th)
  const many = ss.length > 4
  return {
    tooltip: {
      ...tooltipBase(th),
      trigger: 'axis',
      axisPointer: { type: 'shadow', shadowStyle: { color: th.fg, opacity: 0.06 } },
      formatter: (ps: unknown) => {
        const arr = ps as { seriesName: string; value: number; color: string; axisValue: string }[]
        if (!arr.length) return ''
        const rows = arr.filter((p) => p.value > 0).sort((a, b) => b.value - a.value)
        const total = arr.reduce((a, p) => a + p.value, 0)
        return (
          ttTitle(arr[0]!.axisValue) +
          rows.map((p) => ttRow(p.color, shortModel(p.seriesName, 30), fmtExact.value(p.value))).join('') +
          (rows.length > 1 ? `<hr style="border:0;border-top:1px solid ${th.line};margin:4px 0">` + ttRow(null, '合计', fmtExact.value(total)) : '')
        )
      },
    },
    legend: {
      type: 'plain',
      top: 0,
      left: 0,
      icon: 'rect',
      itemWidth: 10,
      itemHeight: 10,
      itemGap: 12,
      textStyle: { color: th.fg1, fontSize: 11, fontFamily: th.fontFamily },
      formatter: (n: string) => shortModel(n, many ? 18 : 28),
    },
    grid: { left: 8, right: 8, top: 34, bottom: 4, containLabel: true },
    xAxis: {
      type: 'category',
      data: dates.value,
      ...ax,
      splitLine: { show: false },
      axisLabel: { ...ax.axisLabel, formatter: (v: string) => v.slice(5), hideOverlap: true },
    },
    yAxis: {
      type: 'value',
      ...ax,
      axisLine: { show: false },
      interval: hourInterval.value,
      axisLabel: {
        ...ax.axisLabel,
        formatter: (v: number) => (mode.value === 'time' ? (v === 0 ? '0' : `${+(v / 3.6e6).toFixed(2)}h`) : fmt.value(v)),
        fontFamily: th.monoFamily,
      },
    },
    series: ss.map((s) => ({
      type: 'bar',
      name: s.name,
      stack: 'total',
      data: s.values,
      barMaxWidth: 22,
      barCategoryGap: '35%',
      itemStyle: { color: s.color, borderColor: th.bg, borderWidth: 1 },
    })),
  }
})
</script>

<template>
  <DashCard n="03" title="每日趋势" :caption="mode === 'cost' ? '前 7 个模型各占一色，其余折入 other' : mode === 'tokens' ? '四类 token 堆叠' : '模型时间 + 工具时间'">
    <template #controls>
      <button v-for="m in MODES" :key="m.key" type="button" :class="{ active: mode === m.key }" @click="mode = m.key">
        {{ m.label }}
      </button>
    </template>
    <EChart :option="option" :height="260" />
  </DashCard>
</template>
