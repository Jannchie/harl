<script setup lang="ts">
import { computed } from 'vue'
import type { ToolAgg } from '../../api'
import EChart from '../EChart.vue'
import type { EChartsOption } from '../../lib/echarts'
import { tooltipBase, ttRow, ttTitle, useChartTheme } from '../../lib/chartTheme'
import { fmtBig, fmtInt, fmtMs, fmtPct } from '../../lib/format'
import { CATEGORY_ORDER, categoryColor, categoryLabel } from '../../lib/summaryModel'
import DashCard from './DashCard.vue'

const props = defineProps<{ byTool: Record<string, ToolAgg> }>()
const t = useChartTheme()

const TOP = 15
const ROW = 19

interface Row {
  name: string
  category: string
  calls: number
  failures: number
  failRate: number
  avgMs: number
}

const rows = computed<Row[]>(() =>
  Object.entries(props.byTool)
    .map(([name, x]) => ({
      name,
      category: x.category,
      calls: x.calls,
      failures: x.failures,
      failRate: x.calls > 0 ? x.failures / x.calls : 0,
      avgMs: x.calls > 0 ? x.ms / x.calls : 0,
    }))
    .sort((a, b) => b.calls - a.calls || a.name.localeCompare(b.name))
    .slice(0, TOP),
)

const totalTools = computed(() => Object.keys(props.byTool).length)
const height = computed(() => rows.value.length * ROW + 44)

/** 图例：榜上出现的类别，按固定顺序 */
const legend = computed(() => {
  const present = new Set(rows.value.map((r) => r.category))
  const known = CATEGORY_ORDER.filter((c) => present.has(c))
  const unknown = [...present].filter((c) => !CATEGORY_ORDER.includes(c as never)).sort()
  return [...known, ...unknown].map((c) => ({ key: c, label: categoryLabel(c), color: categoryColor(c, t.value) }))
})

const option = computed<EChartsOption>(() => {
  const th = t.value
  const names = rows.value.map((r) => r.name)
  const byName = new Map(rows.value.map((r) => [r.name, r]))
  // 三个并排的 grid：left/right 都用百分比或像素，ECharts 不认 calc()
  const labelW = 130
  // 调用次数 / 平均耗时按类别着色（身份）；失败率是"坏"的量，用状态色
  const panes: { title: string; color: (r: Row) => string; pick: (r: Row) => number; fmt: (v: number) => string; left: number | string; right: number | string }[] = [
    { title: '调用次数', color: (r) => categoryColor(r.category, th), pick: (r) => r.calls, fmt: fmtBig, left: labelW, right: '62%' },
    { title: '失败率', color: () => th.danger, pick: (r) => r.failRate, fmt: (v) => fmtPct(v, 1), left: '44%', right: '32%' },
    { title: '平均耗时', color: (r) => categoryColor(r.category, th), pick: (r) => r.avgMs, fmt: fmtMs, left: '72%', right: 8 },
  ]
  return {
    tooltip: {
      ...tooltipBase(th),
      trigger: 'item',
      formatter: (p: unknown) => {
        const q = p as { name: string }
        const r = byName.get(q.name)
        if (!r) return q.name
        const isUser = r.category === 'user'
        return (
          ttTitle(r.name) +
          ttRow(categoryColor(r.category, th), '类别', `${r.category} · ${categoryLabel(r.category)}`, true) +
          ttRow(null, '调用', fmtInt(r.calls)) +
          ttRow(null, '失败', `${fmtInt(r.failures)}（${fmtPct(r.failRate)}）`) +
          ttRow(null, isUser ? '平均耗时（含等待用户）' : '平均耗时', fmtMs(r.avgMs)) +
          ttRow(null, isUser ? '总耗时（含等待用户）' : '总耗时', fmtMs(r.avgMs * r.calls), true)
        )
      },
    },
    grid: panes.map((p) => ({
      left: p.left,
      right: p.right,
      top: 24,
      bottom: 2,
      containLabel: false,
    })) as never,
    xAxis: panes.map((_, i) => ({
      type: 'value',
      gridIndex: i,
      show: false,
      max: (v: { max: number }) => v.max * 1.28,
    })) as never,
    yAxis: panes.map((_, i) => ({
      type: 'category',
      gridIndex: i,
      data: names,
      inverse: true,
      axisLine: { show: false },
      axisTick: { show: false },
      axisLabel: {
        show: i === 0,
        color: th.fg1,
        fontSize: 11,
        fontFamily: th.monoFamily,
        width: labelW - 10,
        overflow: 'truncate',
        ellipsis: '…',
      },
    })) as never,
    // 三列的标题是模板里的 HTML 叠在图上，见 .heads
    series: panes.map((p, i) => ({
      type: 'bar',
      name: p.title,
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
      emphasis: { itemStyle: { opacity: 0.8 } },
    })) as never,
  }
})
</script>

<template>
  <DashCard n="08" title="工具排行" :caption="`按调用次数取前 ${rows.length}，共 ${totalTools} 个工具`">
    <div v-if="rows.length === 0" class="state">没有工具调用。</div>
    <template v-else>
      <div class="legend">
        <span v-for="c in legend" :key="c.key" class="lg">
          <span class="sw" :style="{ background: c.color }"></span>
          <span class="mono">{{ c.key }}</span>
          <span class="muted">{{ c.label }}</span>
        </span>
      </div>
      <div class="heads mono">
        <span class="h h1">调用次数</span>
        <span class="h h2">失败率</span>
        <span class="h h3">平均耗时</span>
      </div>
      <EChart :option="option" :height="height" />
    </template>
  </DashCard>
</template>

<style scoped>
.legend {
  display: flex;
  gap: 12px;
  flex-wrap: wrap;
  padding: 2px 4px 4px;
  font-size: var(--fs-xs);
}
.lg {
  display: inline-flex;
  align-items: center;
  gap: 5px;
}
.sw {
  width: 9px;
  height: 9px;
  display: inline-block;
}
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
  left: 130px;
}
.h2 {
  left: 44%;
}
.h3 {
  left: 72%;
}
</style>
