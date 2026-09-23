<script setup lang="ts">
import { computed, ref } from 'vue'
import type { PricedDailyAgg as DailyAgg, PricedModelAgg as ModelAgg } from '../../lib/pricing'
import EChart from '../EChart.vue'
import type { EChartsOption } from '../../lib/echarts'
import { axisBase, tooltipBase, ttRow, ttTitle, useChartTheme } from '../../lib/chartTheme'
import { fmtBig, fmtMoney, fmtMoneyExact, fmtMs } from '../../lib/format'
import { type DateRange, dateRange, modelPalette, OTHER, shortModel } from '../../lib/summaryModel'
import DashCard from './DashCard.vue'
import SegButton from './SegButton.vue'

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
  const topOf = dates.value.map((_, di) => {
    for (let si = ss.length - 1; si >= 0; si--) if ((ss[si]!.values[di] ?? 0) > 0) return si
    return -1
  })
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
    grid: { left: 4, right: 4, top: 8, bottom: 4, containLabel: true },
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
  <DashCard icon="i-carbon-chart-column" title="每日趋势" :caption="mode === 'cost' ? '前 7 个模型各占一色，其余折入 other' : mode === 'tokens' ? '四类 token 堆叠' : '模型时间 + 工具时间'">
    <template #controls>
      <SegButton v-for="m in MODES" :key="m.key" :active="mode === m.key" @click="mode = m.key">{{ m.label }}</SegButton>
    </template>
    <ul class="m-0 mb-2 flex list-none flex-wrap gap-x-4 gap-y-1 p-0 text-xs ink-muted">
      <li v-for="s in series" :key="s.name" class="flex items-center gap-1.5" :title="s.name">
        <span class="h-2 w-2 shrink-0 rounded-full" :style="{ background: s.color }"></span>
        {{ shortModel(s.name, 28) }}
      </li>
    </ul>
    <EChart :option="option" :height="240" />
  </DashCard>
</template>
