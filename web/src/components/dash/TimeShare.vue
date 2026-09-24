<script setup lang="ts">
import { computed } from 'vue'
import type { SummaryTotals, ToolAgg, ToolCategory } from '../../api'
import { useChartTheme } from '../../lib/chartTheme'
import { fmtInt, fmtMs } from '../../lib/format'
import type { PricedModelAgg } from '../../lib/pricing'
import { categoryColor, categoryLabel, modelPalette } from '../../lib/summaryModel'
import DashCard from './DashCard.vue'
import IcicleBar, { type IcicleGroup, type ShareSeg } from './IcicleBar.vue'

const props = defineProps<{
  totals: SummaryTotals
  byModel: Record<string, PricedModelAgg>
  byCategory: Partial<Record<ToolCategory, ToolAgg>>
  /** 只用来在类别的 tooltip 里列工具名 */
  byTool: Record<string, ToolAgg>
}>()
const t = useChartTheme()

const TOP_MODELS = 7

/** 分母：墙钟；模型 + 工具 偶尔会超过墙钟（并行的子 agent），取大的那个 */
const wall = computed(() => Math.max(props.totals.wall_ms, props.totals.model_ms + props.totals.tool_ms))

/** 模型时间按模型拆，按时长降序；前 7 个单列，其余折成"其他模型" */
const modelSegs = computed<ShareSeg[]>(() => {
  const th = t.value
  const pal = modelPalette(props.byModel, th)
  const sorted = Object.entries(props.byModel)
    .filter(([, a]) => a.model_ms > 0)
    .sort(([, a], [, b]) => b.model_ms - a.model_ms)
  const head: ShareSeg[] = sorted.slice(0, TOP_MODELS).map(([m, a]) => ({
    key: m,
    name: m,
    value: a.model_ms,
    color: pal.color(m),
    hint: `${fmtInt(a.steps)} steps`,
  }))
  const tail = sorted.slice(TOP_MODELS)
  const rest = tail.reduce((s, [, a]) => s + a.model_ms, 0)
  if (rest > 0) {
    head.push({ key: '__other', name: `其他模型（${tail.length}）`, value: rest, color: th.other, tip: tail.map(([m]) => m).join(', ') })
  }
  return head
})

/** 工具时间按类别拆，按时长降序 */
const toolSegs = computed<ShareSeg[]>(() => {
  const th = t.value
  const names = new Map<string, string[]>()
  for (const [name, x] of Object.entries(props.byTool)) {
    if (x.ms > 0) names.set(x.category, [...(names.get(x.category) ?? []), name])
  }
  return (Object.entries(props.byCategory) as [ToolCategory, ToolAgg][])
    .filter(([, x]) => x.ms > 0)
    .sort(([, a], [, b]) => b.ms - a.ms)
    .map(([c, x]) => ({
      key: c,
      name: categoryLabel(c),
      value: x.ms,
      color: categoryColor(c, th),
      hint: `${fmtInt(x.calls)} 次`,
      tip: `${c} · ${categoryLabel(c)}\n${(names.get(c) ?? []).sort().join(', ')}`,
    }))
})

/** 墙钟 = 模型 + 工具 + 其他等待；模型、工具按时长排，其他等待是墙钟剩下的，IcicleBar 画在最后 */
const groups = computed<IcicleGroup[]>(() =>
  [
    { key: 'model', name: '模型', value: props.totals.model_ms, cap: t.value.fg2, segs: modelSegs.value },
    { key: 'tool', name: '工具', value: props.totals.tool_ms, cap: t.value.other, segs: toolSegs.value },
  ].sort((a, b) => b.value - a.value),
)
const IDLE = { name: '其他等待', tip: '墙钟 − 模型 − 工具，为负时记 0' }
</script>

<template>
  <DashCard
    icon="i-carbon-time"
    title="时间"
    title-hint="墙钟 = 模型 + 工具 + 其他等待；下层是上层对应段的展开，同一刻度。「等用户」类别的时长是在等人回答"
  >
    <div v-if="wall === 0" class="py-8 text-center text-sm ink-soft">没有时长数据。</div>
    <IcicleBar v-else label="墙钟" :amount="fmtMs(wall)" :groups="groups" :total="wall" :rest="IDLE" :fmt="fmtMs" of-label="占墙钟" />
  </DashCard>
</template>
