<script setup lang="ts">
import { computed } from 'vue'
import type { ToolAgg, ToolCategory } from '../../api'
import EChart from '../EChart.vue'
import type { EChartsOption } from '../../lib/echarts'
import { tooltipBase, ttRow, ttTitle, useChartTheme } from '../../lib/chartTheme'
import { fmtBig, fmtInt, fmtMs, fmtPct } from '../../lib/format'
import { CATEGORY_ORDER, categoryColor, categoryLabel } from '../../lib/summaryModel'
import DashCard from './DashCard.vue'

const props = defineProps<{ byCategory: Partial<Record<ToolCategory, ToolAgg>> }>()
const t = useChartTheme()

const ROW = 19

interface Row {
  key: string
  label: string
  calls: number
  failures: number
  failRate: number
  ms: number
}

/** 固定类别顺序（不按值排），颜色跟着类别走 */
const rows = computed<Row[]>(() => {
  const entries = Object.entries(props.byCategory) as [string, ToolAgg][]
  const known = CATEGORY_ORDER.filter((c) => props.byCategory[c])
  const unknown = entries.map(([k]) => k).filter((k) => !CATEGORY_ORDER.includes(k as never)).sort()
  return [...known, ...unknown].map((k) => {
    const x = props.byCategory[k as ToolCategory]!
    return {
      key: k,
      label: categoryLabel(k),
      calls: x.calls,
      failures: x.failures,
      failRate: x.calls > 0 ? x.failures / x.calls : 0,
      ms: x.ms,
    }
  })
})

const height = computed(() => rows.value.length * ROW + 44)

const option = computed<EChartsOption>(() => {
  const th = t.value
  const byKey = new Map(rows.value.map((r) => [r.key, r]))
  const labelW = 96
  const panes: { pick: (r: Row) => number; color: (r: Row) => string; fmt: (v: number) => string; left: number | string; right: number | string }[] = [
    { pick: (r) => r.calls, color: (r) => categoryColor(r.key, th), fmt: fmtBig, left: labelW, right: '66%' },
    { pick: (r) => r.failRate, color: () => th.danger, fmt: (v) => fmtPct(v, 1), left: '38%', right: '38%' },
    { pick: (r) => r.ms, color: (r) => categoryColor(r.key, th), fmt: fmtMs, left: '66%', right: 8 },
  ]
  return {
    tooltip: {
      ...tooltipBase(th),
      trigger: 'item',
      formatter: (p: unknown) => {
        const q = p as { name: string }
        const r = byKey.get(q.name)
        if (!r) return q.name
        const isUser = r.key === 'user'
        return (
          ttTitle(`${r.key} · ${r.label}`) +
          ttRow(null, '调用', fmtInt(r.calls)) +
          ttRow(null, '失败', `${fmtInt(r.failures)}（${fmtPct(r.failRate)}）`) +
          ttRow(null, isUser ? '总耗时（含等待用户）' : '总耗时', fmtMs(r.ms)) +
          ttRow(null, '平均', fmtMs(r.calls > 0 ? r.ms / r.calls : null), true)
        )
      },
    },
    grid: panes.map((p) => ({ left: p.left, right: p.right, top: 24, bottom: 2, containLabel: false })) as never,
    xAxis: panes.map((_, i) => ({ type: 'value', gridIndex: i, show: false, max: (v: { max: number }) => v.max * 1.3 })) as never,
    yAxis: panes.map((_, i) => ({
      type: 'category',
      gridIndex: i,
      data: rows.value.map((r) => r.key),
      inverse: true,
      axisLine: { show: false },
      axisTick: { show: false },
      axisLabel: { show: i === 0, color: th.fg1, fontSize: 11, fontFamily: th.monoFamily },
    })) as never,
    series: panes.map((p, i) => ({
      type: 'bar',
      xAxisIndex: i,
      yAxisIndex: i,
      data: rows.value.map((r) => ({ value: p.pick(r), itemStyle: { color: p.color(r) } })),
      barWidth: 11,
      itemStyle: { borderRadius: [0, 3, 3, 0] },
      label: {
        show: true,
        position: 'right',
        distance: 4,
        color: th.fg1,
        fontSize: 10,
        fontFamily: th.monoFamily,
        formatter: (q: { value: unknown }) => p.fmt(Number(q.value)),
      },
    })) as never,
  }
})
</script>

<template>
  <DashCard n="07" title="工具类别" caption="按后端归类：不同 harness 的同类工具合并">
    <div v-if="rows.length === 0" class="state">没有工具调用。</div>
    <template v-else>
      <div class="heads mono">
        <span class="h h1">调用次数</span>
        <span class="h h2">失败率</span>
        <span class="h h3">总耗时</span>
      </div>
      <EChart :option="option" :height="height" />
    </template>
    <template #footer>user 类别（AskUserQuestion 等）的耗时是在等人回答，不是执行时间。</template>
  </DashCard>
</template>

<style scoped>
.heads {
  position: relative;
  height: 0;
  z-index: 1;
  font-size: var(--fs-xs);
  color: var(--fg-2);
  letter-spacing: 0.04em;
}
.h {
  position: absolute;
  top: 6px;
}
.h1 {
  left: 96px;
}
.h2 {
  left: 38%;
}
.h3 {
  left: 66%;
}
</style>
