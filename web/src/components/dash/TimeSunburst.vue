<script setup lang="ts">
import { computed } from 'vue'
import type { ModelAgg, SummaryTotals, ToolAgg } from '../../api'
import EChart from '../EChart.vue'
import type { EChartsOption } from '../../lib/echarts'
import { tooltipBase, ttRow, ttTitle, useChartTheme } from '../../lib/chartTheme'
import { fmtInt, fmtMs, fmtPct } from '../../lib/format'
import { colored, sunburstSeries, type SunNode } from '../../lib/sunburst'
import { CATEGORY_ORDER, categoryColor, categoryLabel, modelOrder, modelPalette } from '../../lib/summaryModel'
import DashCard from './DashCard.vue'

const props = defineProps<{
  totals: SummaryTotals
  byModel: Record<string, ModelAgg>
  byTool: Record<string, ToolAgg>
}>()
const t = useChartTheme()

const TOP_MODELS = 7
/** 每个类别下最多单列几个工具，其余折成"其他" */
const TOP_PER_CATEGORY = 5

function fold(
  entries: [string, number][],
  top: number,
  make: (name: string, v: number, folded: string[]) => SunNode,
  otherName: string,
): SunNode[] {
  const sorted = entries.filter(([, v]) => v > 0).sort((a, b) => b[1] - a[1])
  const head = sorted.slice(0, top).map(([n, v]) => make(n, v, []))
  const tail = sorted.slice(top)
  const rest = tail.reduce((a, [, v]) => a + v, 0)
  if (rest > 0)
    head.push(
      make(
        `${otherName}（${tail.length}）`,
        rest,
        tail.map(([n]) => n),
      ),
    )
  return head
}

const split = computed(() => {
  const toolSum = Object.values(props.byTool).reduce((a, x) => a + x.ms, 0)
  const modelSum = Object.values(props.byModel).reduce((a, x) => a + x.model_ms, 0)
  return { toolSum, modelSum, idle: Math.max(0, props.totals.wall_ms - toolSum - modelSum) }
})

const data = computed<SunNode[]>(() => {
  const th = t.value
  const pal = modelPalette(props.byModel, th)

  // 模型：各模型用它在别的图里的颜色
  const modelKids = fold(
    modelOrder(props.byModel).map((m) => [m, props.byModel[m]!.model_ms] as [string, number]),
    TOP_MODELS,
    (n, v, folded) =>
      colored(
        { name: n, value: v, meta: folded.length ? { members: folded.join(', ') } : { steps: fmtInt(props.byModel[n]?.steps ?? 0) } },
        folded.length ? th.other : pal.color(n),
        0.9,
      ),
    '其他模型',
  )

  // 工具：类别 → 工具名；工具沿用类别色，淡一档
  const byCat = new Map<string, [string, number][]>()
  for (const [name, x] of Object.entries(props.byTool)) {
    if (x.ms <= 0) continue
    const arr = byCat.get(x.category) ?? []
    arr.push([name, x.ms])
    byCat.set(x.category, arr)
  }
  const cats = [...byCat.keys()].sort((a, b) => {
    const ia = CATEGORY_ORDER.indexOf(a as never)
    const ib = CATEGORY_ORDER.indexOf(b as never)
    return (ia < 0 ? 99 : ia) - (ib < 0 ? 99 : ib)
  })
  const toolKids: SunNode[] = cats.map((c) => {
    const color = categoryColor(c, th)
    const kids = fold(
      byCat.get(c)!,
      TOP_PER_CATEGORY,
      (n, v, folded) =>
        colored(
          {
            name: n,
            value: v,
            meta: folded.length ? { members: folded.join(', '), category: c } : { calls: fmtInt(props.byTool[n]?.calls ?? 0), category: c },
          },
          color,
          0.7,
        ),
      '其他',
    )
    return colored({ name: c, children: kids, meta: { category: c } }, color)
  })

  const out: SunNode[] = []
  if (modelKids.length) out.push(colored({ name: '模型', children: modelKids }, th.fg2))
  if (toolKids.length) out.push(colored({ name: '工具', children: toolKids }, th.fg1))
  if (split.value.idle > 0) out.push(colored({ name: '其他等待', value: split.value.idle }, th.other))
  return out
})

const option = computed<EChartsOption>(() => {
  const th = t.value
  const wall = props.totals.wall_ms
  const series = sunburstSeries(th, data.value)
  // 三层：模型/工具/其他等待 → 模型名/类别 → 工具名
  series.levels = [
    {},
    { r0: '14%', r: '34%', label: { rotate: 'tangential', fontWeight: 600 } },
    { r0: '34%', r: '62%', label: { rotate: 'tangential' } },
    { r0: '62%', r: '92%', label: { align: 'left', minAngle: 6 } },
  ]
  series.radius = ['14%', '92%']
  return {
    tooltip: {
      ...tooltipBase(th),
      formatter: (p: unknown) => {
        const q = p as { name: string; value: number; data: SunNode; treePathInfo: { name: string }[] }
        const path = q.treePathInfo
          .slice(1)
          .map((x) => x.name)
          .join(' › ')
        const m = q.data.meta ?? {}
        const isUser = m.category === 'user'
        return (
          ttTitle(path || q.name) +
          ttRow(null, isUser ? '时长（含等待用户）' : '时长', fmtMs(q.value)) +
          ttRow(null, '占墙钟', fmtPct(wall > 0 ? q.value / wall : null), true) +
          (m.category ? ttRow(null, '类别', `${m.category} · ${categoryLabel(m.category)}`, true) : '') +
          (m.steps ? ttRow(null, 'steps', m.steps, true) : '') +
          (m.calls ? ttRow(null, '调用', m.calls, true) : '') +
          (m.members ? `<div style="margin-top:3px;opacity:.7;max-width:320px;white-space:normal">${m.members}</div>` : '')
        )
      },
    },
    series: [series],
  }
})
</script>

<template>
  <DashCard n="05" title="时间 · 墙钟拆分" :caption="`墙钟 ${fmtMs(totals.wall_ms)}`">
    <div v-if="data.length === 0" class="state">没有时长数据。</div>
    <EChart v-else :option="option" :height="300" />
    <template #footer>
      工具环 = 类别 › 工具名（每类前 {{ TOP_PER_CATEGORY }} 个）；user 类别的时长是在等人。其他等待 = 墙钟 − 模型 − 工具，为负时记 0。
    </template>
  </DashCard>
</template>
