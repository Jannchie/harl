<script setup lang="ts">
// 推理强度 / skill 两种切法看同一件事：钱和时间花在了哪种调用上。
// 两个维度都只覆盖一部分步（pfh 没有强度，大多数步不归属 skill），覆盖率写在标题下面。
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import type { DailyAgg } from '../../api'
import type { EChartsOption } from '../../lib/echarts'
import { modelLabel, type PricedTag } from '../../lib/pricing'
import { fmtBig, fmtInt, fmtMoney, fmtMoneyExact, fmtMs, fmtPct, tn } from '../../lib/format'
import { axisBase, tooltipBase, ttRow, ttTitle, useChartTheme } from '../../lib/chartTheme'
import { bucketOf, dateRange, type DateRange, granOf, spanOf } from '../../lib/summaryModel'
import { useSort } from '../../lib/sort'
import DashCard from './DashCard.vue'
import EChart from '../EChart.vue'
import SortTh from './SortTh.vue'
import Segmented from '../Segmented.vue'

const props = defineProps<{
  byEffort: Record<string, PricedTag>
  bySkill: Record<string, PricedTag>
  totalSteps: number
  totalCost: number
  /** 强度占比的时间走势用 */
  daily: Pick<DailyAgg, 'date' | 'by_effort'>[]
  range: DateRange
  bounds: { min: string | null; max: string | null }
}>()
const theme = useChartTheme()
const { t } = useI18n()

type View = 'effort' | 'skill'
// Dashboard 只在两者至少有一个非空时才渲染这张卡
const view = ref<View>(Object.keys(props.byEffort).length ? 'effort' : 'skill')
const VIEWS = computed(() => [
  { value: 'effort' as const, label: t('common.metric.effort') },
  { value: 'skill' as const, label: 'Skill' },
])

/** 强度是有序的：从低到高，没见过的排在后面 */
const LEVELS = ['minimal', 'low', 'medium', 'high', 'xhigh', 'max']
const level = (e: string) => {
  const i = LEVELS.indexOf(e.toLowerCase())
  return i < 0 ? LEVELS.length : i
}
/** 强度用顺序色（越高越深） */
const effortColor = (e: string) => theme.value.seq[Math.min(theme.value.seq.length - 1, 1 + level(e))] ?? theme.value.accent

interface Row {
  name: string
  a: PricedTag
  color: string
  /** 强度的次序；skill 是 0 */
  order: number
  share: number | null
  perStepMs: number | null
  perStepOut: number | null
  perStepCost: number | null
}

const tags = computed(() => (view.value === 'effort' ? props.byEffort : props.bySkill))
const rows = computed<Row[]>(() =>
  Object.entries(tags.value).map(([name, a]) => {
    // 强度用顺序色（越高越深），skill 之间没有次序，用同一个强调色
    const order = view.value === 'effort' ? level(name) : 0
    const color = view.value === 'effort' ? effortColor(name) : theme.value.accent
    const n = a.steps || null
    return {
      name,
      a,
      color,
      order,
      share: props.totalCost > 0 && !a.unpriced ? a.cost_usd / props.totalCost : null,
      perStepMs: n && a.model_ms / n,
      perStepOut: n && a.tokens.output / n,
      perStepCost: n && !a.unpriced ? a.cost_usd / n : null,
    }
  }),
)
const { sort, sorted } = useSort(
  rows,
  {
    name: (r) => (view.value === 'effort' ? r.order : r.name),
    steps: (r) => r.a.steps,
    cost: (r) => (r.a.unpriced ? null : r.a.cost_usd),
    perStepMs: (r) => r.perStepMs,
    perStepOut: (r) => r.perStepOut,
    perStepCost: (r) => r.perStepCost,
  },
  'cost',
)
const maxCost = computed(() => Math.max(1e-9, ...rows.value.map((r) => r.a.cost_usd)))

/** 说明这个维度覆盖了多少步 / 多少钱 */
const caption = computed(() => {
  if (!rows.value.length) return undefined
  const steps = rows.value.reduce((n, r) => n + r.a.steps, 0)
  const cost = rows.value.reduce((n, r) => n + r.a.cost_usd, 0)
  const s = props.totalSteps > 0 ? fmtPct(steps / props.totalSteps, 0) : '–'
  const c = props.totalCost > 0 ? fmtPct(cost / props.totalCost, 0) : '–'
  return view.value === 'effort'
    ? t('cards.effortSkill.captionEffort', { s, c })
    : t('cards.effortSkill.captionSkill', { s, cost: fmtMoney(cost), c })
})

function tip(r: Row): string {
  const models = Object.entries(r.a.by_model)
    .sort(([, x], [, y]) => y.cost_usd - x.cost_usd)
    .map(([m, x]) => `  ${modelLabel(m)}  ${tn('common.metric.stepsN', x.steps)} · ${x.unpriced ? t('cards.common.unpriced') : fmtMoneyExact(x.cost_usd)}`)
  return [
    r.name,
    `${tn('common.metric.stepsN', r.a.steps)} · ${t('cards.effortSkill.tipOutput', { out: fmtInt(r.a.tokens.output), time: fmtMs(r.a.model_ms) })}`,
    t('cards.common.costIs', { cost: r.a.unpriced ? t('cards.common.includesUnpriced') : fmtMoneyExact(r.a.cost_usd) }),
    t('cards.common.byModel'),
    ...models,
  ].join('\n')
}

const td = 'py-1 pl-3 text-right text-xs num'

/** 强度占比随时间：和趋势图同一套粒度（分时没有按强度的拆分，一天时按日）；少于两根柱不画 */
const trend = computed(() => {
  const span = spanOf(props.range, props.bounds, props.daily)
  if (!span) return null
  const days = dateRange(...span)
  const g = granOf(days.length)
  const gran = g === 'hour' ? 'day' : g
  const byDate = new Map(props.daily.map((d) => [d.date, d.by_effort]))
  const buckets: { key: string; steps: Record<string, number> }[] = []
  for (const day of days) {
    const key = bucketOf(day, gran)
    let b = buckets[buckets.length - 1]
    if (b?.key !== key) buckets.push((b = { key, steps: {} }))
    for (const [e, n] of Object.entries(byDate.get(day) ?? {})) b.steps[e] = (b.steps[e] ?? 0) + n
  }
  const levels = [...new Set(buckets.flatMap((b) => Object.keys(b.steps)))].sort((a, b) => level(a) - level(b) || a.localeCompare(b))
  return buckets.length >= 2 && levels.length ? { buckets, levels, gran } : null
})

const trendOption = computed<EChartsOption>(() => {
  const th = theme.value
  const ax = axisBase(th)
  const levelTrend = trend.value!
  const totals = levelTrend.buckets.map((b) => Object.values(b.steps).reduce((a, n) => a + n, 0))
  return {
    tooltip: {
      ...tooltipBase(th),
      trigger: 'axis',
      axisPointer: { type: 'shadow', shadowStyle: { color: th.fg, opacity: 0.06 } },
      formatter: (ps: unknown) => {
        const arr = ps as { seriesName: string; color: string; dataIndex: number }[]
        const b = levelTrend.buckets[arr[0]?.dataIndex ?? 0]
        if (!b) return ''
        const total = totals[arr[0]!.dataIndex] ?? 0
        return (
          ttTitle(levelTrend.gran === 'week' ? t('cards.common.weekOf', { key: b.key }) : b.key) +
          [...arr].reverse().filter((p) => b.steps[p.seriesName]).map((p) => ttRow(p.color, p.seriesName, `${fmtPct(b.steps[p.seriesName]! / total)} · ${tn('common.metric.stepsN', b.steps[p.seriesName]!)}`)).join('')
        )
      },
    },
    grid: { left: 4, right: 4, top: 8, bottom: 4, containLabel: true },
    xAxis: {
      type: 'category',
      data: levelTrend.buckets.map((b) => b.key),
      ...ax,
      splitLine: { show: false },
      axisLabel: { ...ax.axisLabel, formatter: (v: string) => (levelTrend.gran === 'month' ? v : v.slice(5)), hideOverlap: true },
    },
    yAxis: { type: 'value', max: 1, ...ax, axisLine: { show: false }, axisLabel: { ...ax.axisLabel, formatter: (v: number) => fmtPct(v, 0) } },
    series: levelTrend.levels.map((e) => ({
      type: 'bar',
      name: e,
      stack: 'share',
      data: levelTrend.buckets.map((b, i) => (totals[i] ? (b.steps[e] ?? 0) / totals[i]! : 0)),
      barMaxWidth: 22,
      barCategoryGap: '35%',
      itemStyle: { color: effortColor(e), borderColor: th.surface, borderWidth: 1 },
    })),
  }
})
</script>

<template>
  <DashCard
    icon="i-jannchie-gauge"
    :title="t('cards.effortSkill.title')"
    :title-hint="t('cards.effortSkill.titleHint')"
    :caption="caption"
  >
    <template #controls>
      <Segmented v-model="view" :label="t('cards.effortSkill.dimension')" :options="VIEWS" />
    </template>
    <div v-if="rows.length === 0" class="py-8 text-center text-sm ink-soft">
      {{ view === 'effort' ? t('cards.effortSkill.emptyEffort') : t('cards.effortSkill.emptySkill') }}
    </div>
    <template v-else>
      <div v-if="view === 'effort' && trend" class="mb-3">
        <div class="mb-1 text-xs ink-soft">{{ t('cards.effortSkill.trendCaption', { gran: t(`cards.common.gran.${trend.gran}`) }) }}</div>
        <EChart :option="trendOption" :height="140" :label="tn('cards.effortSkill.trendLabel', trend.buckets.length)" />
      </div>
      <div class="-mx-1 overflow-x-auto px-1">
        <table class="w-full border-collapse whitespace-nowrap text-sm">
          <thead>
            <tr class="text-xs ink-soft">
              <SortTh :sort="sort" k="name" align="left">{{ view === 'effort' ? t('cards.effortSkill.level') : 'Skill' }}</SortTh>
              <SortTh :sort="sort" k="cost" align="left" class="pl-3">{{ t('common.metric.cost') }}</SortTh>
              <SortTh :sort="sort" k="steps" class="pl-3">steps</SortTh>
              <SortTh :sort="sort" k="perStepCost" class="pl-3" :title="t('cards.common.perStepCostHint')">{{ t('cards.common.perStepCost') }}</SortTh>
              <SortTh :sort="sort" k="perStepOut" class="pl-3 max-sm:hidden" :title="t('cards.common.perStepOutHint')">{{ t('cards.common.perStepOut') }}</SortTh>
              <SortTh :sort="sort" k="perStepMs" class="pl-3" :title="t('cards.common.perStepMsHint')">{{ t('cards.common.perStepMs') }}</SortTh>
            </tr>
          </thead>
          <tbody>
            <tr v-for="r in sorted" :key="r.name" :title="tip(r)" class="surface-hover">
              <td class="rounded-l-lg py-1 pl-1 pr-3">
                <div class="flex items-center gap-2">
                  <span class="h-2 w-2 shrink-0 rounded-full" :style="{ background: r.color }"></span>
                  <span class="min-w-0 truncate font-mono text-xs">{{ r.name }}</span>
                </div>
              </td>
              <td class="w-2/5 py-1 pl-3">
                <div class="flex items-center gap-2">
                  <div class="meter-track h-1.5 flex-1">
                    <span class="meter-fill" :style="{ width: `${(r.a.cost_usd / maxCost) * 100}%`, background: r.color }"></span>
                  </div>
                  <span class="w-14 text-right text-xs num">{{ r.a.unpriced && r.a.cost_usd === 0 ? '—' : fmtMoney(r.a.cost_usd) }}</span>
                  <span class="w-10 text-right text-xs num ink-soft">{{ fmtPct(r.share, 0) }}</span>
                </div>
              </td>
              <td :class="td">{{ fmtInt(r.a.steps) }}</td>
              <td :class="td">{{ r.perStepCost == null ? '—' : fmtMoney(r.perStepCost) }}</td>
              <td :class="[td, 'max-sm:hidden']">{{ fmtBig(r.perStepOut) }}</td>
              <td :class="[td, 'rounded-r-lg pr-1']">{{ fmtMs(r.perStepMs) }}</td>
            </tr>
          </tbody>
        </table>
      </div>
    </template>
  </DashCard>
</template>
