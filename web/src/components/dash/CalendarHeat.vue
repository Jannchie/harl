<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import type { SessionRow } from '../../api'
import type { PricedDailyAgg as DailyAgg } from '../../lib/pricing'
import EChart from '../EChart.vue'
import type { EChartsOption } from '../../lib/echarts'
import { tooltipBase, ttRow, ttTitle, useChartTheme } from '../../lib/chartTheme'
import { fmtBig, fmtInt, fmtMoney, fmtMoneyExact, fmtMs, monthNames as monthNamesOf, weekdayNames } from '../../lib/format'
import { addDays, chartSummary, type DateRange, localDate, monthEnd, monthStart, totalTokens } from '../../lib/summaryModel'
import DashCard from './DashCard.vue'
import Segmented from '../Segmented.vue'

const props = defineProps<{
  daily: DailyAgg[]
  /** 选中的范围（本地日期，可单边） */
  range: DateRange
  /** 全部数据的本地日期边界 */
  bounds: { min: string | null; max: string | null }
  /** 范围内的会话：按首尾时间数每天的活跃会话 */
  sessions: Pick<SessionRow, 'first_ts' | 'last_ts'>[]
  /** 选中的那一天（仪表盘正只看这天）；没选是 null */
  selected?: string | null
}>()
/** 点了有数据的一天；父组件决定是选中还是取消 */
const emit = defineEmits<{ pick: [date: string] }>()
const theme = useChartTheme()
const { t } = useI18n()

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

/**
 * 每天的活跃会话：首尾时间（本地日期）覆盖这一天就算，跨天的会话每天都记一次。
 * 后端的日桶没有会话数，这里按会话的首尾时间近似
 */
const sessionsByDate = computed(() => {
  const m = new Map<string, number>()
  for (const s of props.sessions) {
    if (!s.first_ts) continue
    const a = localDate(new Date(s.first_ts))
    const b = s.last_ts ? localDate(new Date(s.last_ts)) : a
    // 极端长的会话别把循环拖死：最多数 400 天
    for (let d = a, i = 0; d <= b && i < 400; d = addDays(d, 1), i++) m.set(d, (m.get(d) ?? 0) + 1)
  }
  return m
})

type Metric = 'cost' | 'wall_ms' | 'tokens' | 'sessions'
const metric = ref<Metric>('cost')
/** labelKey 为 null 的直接用 key 当名字（tokens 不翻译） */
const METRICS: { key: Metric; labelKey: string | null; pick: (d: DailyAgg) => number; fmt: (v: number) => string }[] = [
  { key: 'cost', labelKey: 'common.metric.cost', pick: (d) => d.cost_usd, fmt: fmtMoney },
  { key: 'wall_ms', labelKey: 'common.metric.wall', pick: (d) => d.wall_ms, fmt: fmtMs },
  { key: 'tokens', labelKey: null, pick: (d) => totalTokens(d.tokens), fmt: fmtBig },
  { key: 'sessions', labelKey: 'dashMore.calendar.sessions', pick: (d) => sessionsByDate.value.get(d.date) ?? 0, fmt: fmtInt },
]
const metricLabel = (m: (typeof METRICS)[number]) => (m.labelKey ? t(m.labelKey) : m.key)
const METRIC_OPTIONS = computed(() => METRICS.map((m) => ({ value: m.key, label: metricLabel(m) })))
const cur = computed(() => METRICS.find((m) => m.key === metric.value) ?? METRICS[0]!)

const byDate = computed(() => new Map(props.daily.map((d) => [d.date, d])))

function onClick(p: { data?: unknown }) {
  const v = (p.data as { value?: [string, number] } | undefined)?.value
  if (v && byDate.value.has(v[0])) emit('pick', v[0])
}

const ariaLabel = computed(() => {
  const m = cur.value
  return chartSummary(
    t('dash.calendarHeat.ariaHead', { metric: metricLabel(m) }),
    props.daily.map((d) => ({ label: d.date, v: m.pick(d) })),
    'dash.common.activeDays',
    m.fmt,
  )
})

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

/** 月份、星期的短名跟着界面语言 */
const monthNames = computed(() => monthNamesOf('short'))
const dayNames = computed(() => {
  // ECharts 要周日开头
  const w = weekdayNames('compact')
  return [w[6]!, ...w.slice(0, 6)]
})

const option = computed<EChartsOption>(() => {
  const th = theme.value
  const m = cur.value
  const data = props.daily.map((d) => [d.date, m.pick(d)] as [string, number])
  const max = Math.max(1e-9, ...data.map((x) => x[1]))
  const sel = props.selected ?? null
  // 选中的那天描一圈前景色；其余天在选了某天时变淡，一眼看出现在只看哪天
  const items = data.map((v) =>
    sel == null
      ? { value: v }
      : v[0] === sel
        ? { value: v, itemStyle: { borderColor: th.fg, borderWidth: 2 } }
        : { value: v, itemStyle: { opacity: 0.45 } },
  )
  return {
    tooltip: {
      ...tooltipBase(th),
      formatter: (p: unknown) => {
        const v = (p as { value: [string, number] }).value
        const d = byDate.value.get(v[0])
        if (!d) return ttTitle(v[0])
        return (
          ttTitle(v[0]) +
          ttRow(null, metricLabel(m), m.fmt(v[1])) +
          (m.key === 'sessions' ? '' : ttRow(null, t('dashMore.calendar.activeSessions'), fmtInt(sessionsByDate.value.get(v[0]) ?? 0), true)) +
          `<hr style="border:0;border-top:1px solid ${th.line};margin:4px 0">` +
          ttRow(null, 'turns / steps', `${fmtInt(d.turns)} / ${fmtInt(d.steps)}`, true) +
          ttRow(null, t('common.metric.toolCalls'), fmtInt(d.tool_calls), true) +
          ttRow(null, t('common.metric.cost'), fmtMoneyExact(d.cost_usd), true) +
          ttRow(null, 'tokens in / out', `${fmtBig(d.tokens.input)} / ${fmtBig(d.tokens.output)}`, true) +
          ttRow(null, t('common.metric.cacheRead'), fmtBig(d.tokens.cache_read), true) +
          ttRow(null, t('dash.calendarHeat.modelToolWall'), `${fmtMs(d.model_ms)} / ${fmtMs(d.tool_ms)} / ${fmtMs(d.wall_ms)}`, true)
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
      monthLabel: { color: th.fg2, fontSize: 10, fontFamily: th.fontFamily, nameMap: monthNames.value, margin: 6 },
      dayLabel: {
        color: th.fg2,
        fontSize: 10,
        fontFamily: th.fontFamily,
        firstDay: 1,
        nameMap: dayNames.value,
        margin: 6,
      },
    },
    series: [
      {
        type: 'heatmap',
        coordinateSystem: 'calendar',
        data: items,
        cursor: 'pointer',
        itemStyle: { borderColor: th.surface, borderWidth: 3, borderRadius: 3 },
        emphasis: { itemStyle: { borderColor: th.fg2, borderWidth: 1 } },
      },
    ],
  }
})
</script>

<template>
  <DashCard
    icon="i-jannchie-calendar"
    :title="t('dash.common.calendar')"
    :caption="selected ? t('dashMore.calendar.picked', { day: selected }) + ' · ' + t('dashMore.calendar.pickHint') : t('dashMore.calendar.pickHint')"
    :title-hint="metric === 'sessions' ? t('dashMore.calendar.sessionsHint') : undefined"
  >
    <template #controls>
      <Segmented v-model="metric" :label="t('dash.calendarHeat.metric')" :options="METRIC_OPTIONS" />
    </template>
    <div ref="wrap"><EChart :option="option" :height="CAL_TOP + CAL_BOTTOM + cell * 7" :label="ariaLabel" @click="onClick" /></div>
  </DashCard>
</template>
