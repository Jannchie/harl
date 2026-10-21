<script setup lang="ts">
// 上下文趋势：每次请求（或每轮的最后一次请求）一根堆叠柱，按类别拆；变化口径画相对上一根的增减。
// ✂ 标出压缩发生在哪根柱子之前。悬停预览那一步的简报，点击把它在浏览器里打开。
import { computed, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import { axisBase, esc, tooltipBase, ttRow, ttTitle, useChartTheme } from '../../lib/chartTheme'
import { ALL_CATS, briefOf, sumParts, turnSteps, type Cat, type CtxFold, type CtxNode, type Parts, type StepRec } from '../../lib/context'
import { catColor, catLabel, usePref } from '../../lib/ctxView'
import type { ECharts, EChartsOption } from '../../lib/echarts'
import { fmtBig, fmtInt, fmtMs } from '../../lib/format'
import DashCard from '../dash/DashCard.vue'
import EChart from '../EChart.vue'
import Segmented from '../Segmented.vue'

const props = defineProps<{ fold: CtxFold; selected: number }>()
const emit = defineEmits<{ select: [idx: number]; focus: [node: CtxNode] }>()
const { t } = useI18n()
const theme = useChartTheme()

const gran = usePref<'step' | 'turn'>('trend.gran', 'step')
const mode = usePref<'total' | 'delta'>('trend.mode', 'total')
const curve = usePref<boolean>('trend.curve', true)

interface Row {
  /** 代表这根柱子的请求 */
  step: StepRec
  label: string
  title: string
  parts: Parts
  ms: number
}

const rows = computed<Row[]>(() => {
  if (gran.value === 'turn')
    return turnSteps(props.fold).map((x, i) => ({
      step: x.step,
      label: String(x.turn.n ?? i + 1),
      title: `${t('ctx.trend.turnN', { n: x.turn.n ?? i + 1 })} · ${t('ctx.trend.turnSteps', { n: x.count }, x.count)}`,
      parts: x.step.anchored,
      ms: x.modelMs + x.toolMs,
    }))
  return props.fold.steps.map((s) => ({
    step: s,
    label: String(s.idx + 1),
    title: `${t('ctx.trend.stepN', { n: s.idx + 1 })}${s.turnN != null ? ` · ${t('ctx.trend.turnN', { n: s.turnN })}` : ''}`,
    parts: s.anchored,
    ms: s.modelMs + s.toolMs,
  }))
})

const cats = computed(() => ALL_CATS.filter((c) => rows.value.some((r) => r.parts[c] !== 0)))

/** 每类每根柱子的值：累计口径是校准值，变化口径是相对上一根的差 */
const values = computed(() => {
  const out = {} as Record<Cat, number[]>
  for (const c of cats.value)
    out[c] = rows.value.map((r, i) => {
      if (mode.value === 'total') return r.parts[c]
      const prev = rows.value[i - 1]
      return r.parts[c] - (prev ? prev.parts[c] : 0)
    })
  return out
})

/** 压缩落在哪根柱子上：事件之后的第一次请求 */
const markers = computed(() => {
  const out = new Set<number>()
  for (const e of props.fold.events) {
    if (e.kind !== 'compaction' && e.kind !== 'prune') continue
    const i = rows.value.findIndex((r) => r.step.idx >= e.step)
    if (i >= 0) out.add(i)
  }
  return [...out]
})

const selectedRow = computed(() => rows.value.findIndex((r) => (gran.value === 'turn' ? r.step.turnId === props.fold.steps[props.selected]?.turnId : r.step.idx === props.selected)))

const option = computed<EChartsOption>(() => {
  const th = theme.value
  const ax = axisBase(th)
  const n = rows.value.length
  const zoom = n > 160
  const marks = [
    ...markers.value.map((i) => ({ xAxis: i, label: { formatter: '✂', color: th.danger }, lineStyle: { color: th.danger, type: 'dashed' as const } })),
    ...(selectedRow.value >= 0
      ? [{ xAxis: selectedRow.value, label: { show: false }, lineStyle: { color: th.accent, type: 'solid' as const, width: 2, opacity: 0.6 } }]
      : []),
  ]
  return {
    grid: { left: 8, right: curve.value ? 8 : 12, top: 18, bottom: zoom ? 48 : 8, containLabel: true },
    tooltip: {
      ...tooltipBase(th),
      trigger: 'axis',
      axisPointer: { type: 'shadow' },
      formatter: (ps: unknown) => {
        const list = ps as { dataIndex: number }[]
        const i = list[0]?.dataIndex ?? 0
        return tooltipHtml(i)
      },
    },
    dataZoom: zoom
      ? [
          // 只用滑块：inside 型会吃掉页面的滚轮
          { type: 'slider', height: 18, bottom: 8, start: Math.max(0, 100 - (160 / n) * 100), end: 100, borderColor: th.line, textStyle: { color: th.fg2 } },
        ]
      : [],
    xAxis: { type: 'category', data: rows.value.map((r) => r.label), ...ax, splitLine: { show: false } },
    yAxis: [
      { type: 'value', ...ax, axisLabel: { ...ax.axisLabel, formatter: (v: number) => fmtBig(v) } },
      { type: 'value', show: curve.value, ...ax, splitLine: { show: false }, axisLabel: { ...ax.axisLabel, formatter: (v: number) => fmtMs(v) } },
    ],
    series: [
      ...cats.value.map((c, k) => ({
        type: 'bar' as const,
        name: catLabel(c),
        stack: 'ctx',
        data: values.value[c],
        itemStyle: { color: catColor(c, th) },
        barMaxWidth: 22,
        barCategoryGap: '18%',
        emphasis: { focus: 'series' as const },
        markLine: k === 0 && marks.length ? { symbol: ['none', 'none'], silent: true, animation: false, data: marks } : undefined,
      })),
      ...(curve.value
        ? [
            {
              type: 'line' as const,
              name: t('ctx.trend.duration'),
              yAxisIndex: 1,
              data: rows.value.map((r) => r.ms),
              symbol: 'none',
              smooth: true,
              lineStyle: { color: th.fg2, width: 1, opacity: 0.7 },
              itemStyle: { color: th.fg2 },
              z: 5,
            },
          ]
        : []),
    ],
  }
})

function tooltipHtml(i: number): string {
  const th = theme.value
  const r = rows.value[i]
  if (!r) return ''
  let html = ttTitle(r.title)
  const sign = (v: number) => (v > 0 ? `+${fmtInt(v)}` : fmtInt(v))
  let net = 0
  let churn = 0
  for (const c of cats.value) {
    const v = values.value[c]![i]!
    if (mode.value === 'delta') {
      net += v
      churn += Math.abs(v)
    }
    if (v !== 0) html += ttRow(catColor(c, th), catLabel(c), mode.value === 'total' ? fmtInt(v) : sign(v))
  }
  html += '<div style="height:4px"></div>'
  if (mode.value === 'delta') {
    html += ttRow(null, t('ctx.trend.net'), sign(net))
    html += ttRow(null, t('ctx.trend.churn'), fmtInt(churn), true)
  } else html += ttRow(null, t('ctx.trend.estimate'), fmtInt(sumParts(r.parts)), true)
  const u = r.step.usage
  if (u) {
    html += ttRow(null, t('ctx.trend.prompt'), fmtInt(r.step.prompt ?? 0))
    html += ttRow(null, t('ctx.trend.cacheRead'), fmtInt(u.cache_read), true)
    html += ttRow(null, t('ctx.trend.output'), fmtInt(u.output), true)
  }
  if (r.ms) html += ttRow(null, t('ctx.trend.duration'), fmtMs(r.ms), true)
  if (r.step.model) html += `<div style="margin-top:4px;color:${th.fg2}">${esc(r.step.model)}</div>`
  return html
}

const hovered = shallowRef<number | null>(null)

function onReady(c: ECharts) {
  c.on('updateAxisPointer', (e: unknown) => {
    const info = (e as { axesInfo?: { value?: number }[] }).axesInfo?.[0]
    const i = typeof info?.value === 'number' ? info.value : null
    hovered.value = i != null ? (rows.value[i]?.step.idx ?? null) : null
  })
  c.on('globalout', () => {
    hovered.value = null
  })
  c.getZr().on('click', (ev: { offsetX: number; offsetY: number }) => {
    const p = [ev.offsetX, ev.offsetY]
    if (!c.containPixel('grid', p)) return
    // 类目轴按 xAxisIndex 换算会得到 null，按第一个系列换算拿 [类目序号, 值]
    const x = c.convertFromPixel({ seriesIndex: 0 }, p) as number[] | null
    const r = x ? rows.value[Math.round(x[0]!)] : undefined
    // 按轮时 r.step 已经是这一轮的最后一次请求
    if (r) emit('select', r.step.idx)
  })
}

const briefIdx = computed(() => hovered.value ?? (props.selected < props.fold.steps.length ? props.selected : props.fold.steps.length - 1))
const brief = computed(() => briefOf(props.fold, briefIdx.value))
const briefStep = computed(() => props.fold.steps[briefIdx.value])

const GRAN_OPTIONS = computed(() => [
  { value: 'step' as const, label: t('ctx.trend.step') },
  { value: 'turn' as const, label: t('ctx.trend.turn') },
])
const MODE_OPTIONS = computed(() => [
  { value: 'total' as const, label: t('ctx.trend.total') },
  { value: 'delta' as const, label: t('ctx.trend.delta') },
])

const chartLabel = computed(() => {
  const last = rows.value[rows.value.length - 1]
  return `${t('ctx.trend.title')}: ${rows.value.length}${last?.step.prompt != null ? `, ${fmtInt(last.step.prompt)}` : ''}`
})

function nodeLine(n: CtxNode): string {
  return n.name && n.cat !== 'user' ? `${n.name} · ${n.label}` : n.label
}
</script>

<template>
  <DashCard icon="i-jannchie-chart-stacked" :title="t('ctx.trend.title')" :caption="t('ctx.trend.caption')">
    <template #controls>
      <div class="flex flex-wrap items-center justify-end gap-2">
        <Segmented v-model="gran" :label="t('ctx.trend.granularity')" :options="GRAN_OPTIONS" />
        <Segmented v-model="mode" :label="t('ctx.trend.mode')" :options="MODE_OPTIONS" />
        <button
          type="button"
          class="segment border-0"
          :class="curve ? 'segment-selected' : 'bg-transparent ink-muted surface'"
          :aria-pressed="curve"
          @click="curve = !curve"
        >
          <i class="i-jannchie-chart-line h-3.5 w-3.5" aria-hidden="true" />{{ t('ctx.trend.duration') }}
        </button>
      </div>
    </template>
    <EChart :option="option" :height="260" :label="chartLabel" class="cursor-pointer" @ready="onReady" />
    <p class="m-0 mt-1 text-xs ink-soft">{{ t('ctx.trend.pickHint') }}</p>

    <!-- 步骤简报：谁开的头、新进来了什么、回了什么 -->
    <div v-if="briefStep" class="mt-3 grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1 rounded-xl px-3 py-2 text-xs surface">
      <span class="ink-soft">#{{ briefStep.idx + 1 }}</span>
      <span class="truncate ink-muted">
        <template v-if="briefStep.turnN != null">{{ t('ctx.trend.turnN', { n: briefStep.turnN }) }} · </template>{{ briefStep.model }}
        <template v-if="briefStep.prompt != null"> · <span class="num">{{ fmtInt(briefStep.prompt) }}</span></template>
      </span>
      <span class="ink-soft">{{ t('ctx.trend.brief.user') }}</span>
      <button v-if="brief.opener" type="button" class="brief-row" @click="emit('focus', brief.opener)">{{ brief.opener.label }}</button>
      <span v-else class="ink-soft">{{ t('ctx.trend.brief.none') }}</span>
      <span class="ink-soft">{{ t('ctx.trend.brief.in') }}</span>
      <span v-if="brief.inputs.length" class="flex min-w-0 flex-col">
        <button v-for="n in brief.inputs.slice(0, 4)" :key="n.key" type="button" class="brief-row" @click="emit('focus', n)">
          <span class="mr-1 inline-block h-2 w-2 rounded-full" :style="{ background: catColor(n.cat, theme) }" />{{ nodeLine(n) }}
          <span class="ml-1 num ink-soft">≈{{ fmtBig(n.est) }}</span>
        </button>
        <span v-if="brief.inputs.length > 4" class="ink-soft">+{{ brief.inputs.length - 4 }}</span>
      </span>
      <span v-else class="ink-soft">{{ t('ctx.trend.brief.none') }}</span>
      <span class="ink-soft">{{ t('ctx.trend.brief.response') }}</span>
      <span v-if="brief.response.length" class="flex min-w-0 flex-col">
        <button v-for="n in brief.response.slice(0, 4)" :key="n.key" type="button" class="brief-row" @click="emit('focus', n)">
          <i v-if="n.kinds?.includes('tools')" class="i-jannchie-toolbox mr-1 h-3 w-3" aria-hidden="true" />{{ nodeLine(n) }}
        </button>
        <span v-if="brief.response.length > 4" class="ink-soft">+{{ brief.response.length - 4 }}</span>
      </span>
      <span v-else class="ink-soft">{{ t('ctx.trend.brief.none') }}</span>
    </div>
  </DashCard>
</template>

<style scoped>
.brief-row {
  display: block;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  border: 0;
  background: transparent;
  padding: 0;
  text-align: left;
  color: var(--fg-1);
  cursor: pointer;
}
.brief-row:hover {
  color: var(--fg);
  text-decoration: underline;
}
</style>
