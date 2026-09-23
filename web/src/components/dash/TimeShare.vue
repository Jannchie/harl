<script setup lang="ts">
import { computed } from 'vue'
import type { SummaryTotals, ToolAgg, ToolCategory } from '../../api'
import { useChartTheme } from '../../lib/chartTheme'
import { fmtInt, fmtMs } from '../../lib/format'
import type { PricedModelAgg } from '../../lib/pricing'
import { categoryColor, categoryLabel, modelPalette, orderCategories } from '../../lib/summaryModel'
import DashCard from './DashCard.vue'
import ShareBar, { type ShareSeg } from './ShareBar.vue'

const props = defineProps<{
  totals: SummaryTotals
  byModel: Record<string, PricedModelAgg>
  byCategory: Partial<Record<ToolCategory, ToolAgg>>
  /** 只用来在类别的 tooltip 里列工具名 */
  byTool: Record<string, ToolAgg>
}>()
const t = useChartTheme()

const TOP_MODELS = 7

const idle = computed(() => Math.max(0, props.totals.wall_ms - props.totals.tool_ms - props.totals.model_ms))
/** 分母：墙钟；模型 + 工具 偶尔会超过墙钟（并行的子 agent），取大的那个 */
const wall = computed(() => Math.max(props.totals.wall_ms, props.totals.model_ms + props.totals.tool_ms))

/** 第一行：墙钟 = 模型 + 工具 + 其他等待 */
const wallSegs = computed<ShareSeg[]>(() => {
  const th = t.value
  return [
    { key: 'model', name: '模型', value: props.totals.model_ms, color: th.cat[0]! },
    { key: 'tool', name: '工具', value: props.totals.tool_ms, color: th.cat[1]! },
    { key: 'idle', name: '其他等待', value: idle.value, color: th.other, tip: '墙钟 − 模型 − 工具，为负时记 0' },
  ].filter((x) => x.value > 0)
})

/** 第二行：模型时间按模型拆，前 7 个单列，其余折成"其他模型" */
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

/** 第三行：工具时间按类别拆，固定类别顺序 */
const toolSegs = computed<ShareSeg[]>(() => {
  const th = t.value
  const names = new Map<string, string[]>()
  for (const [name, x] of Object.entries(props.byTool)) {
    if (x.ms > 0) names.set(x.category, [...(names.get(x.category) ?? []), name])
  }
  return orderCategories(Object.keys(props.byCategory))
    .map((c) => [c, props.byCategory[c as ToolCategory]!] as const)
    .filter(([, x]) => x.ms > 0)
    .map(([c, x]) => ({
      key: c,
      name: categoryLabel(c),
      value: x.ms,
      color: categoryColor(c, th),
      hint: `${fmtInt(x.calls)} 次`,
      tip: `${c} · ${categoryLabel(c)}\n${(names.get(c) ?? []).sort().join(', ')}`,
    }))
})
</script>

<template>
  <DashCard icon="i-carbon-time" title="时间" :caption="`墙钟 ${fmtMs(totals.wall_ms)}，按模型 / 工具拆开`">
    <div v-if="wallSegs.length === 0" class="py-8 text-center text-sm ink-soft">没有时长数据。</div>
    <div v-else class="flex flex-col gap-5">
      <ShareBar label="墙钟" :amount="fmtMs(wall)" :segments="wallSegs" :total="wall" :fmt="fmtMs" />
      <ShareBar v-if="modelSegs.length" label="模型" :amount="fmtMs(totals.model_ms)" :segments="modelSegs" :total="wall" :fmt="fmtMs" />
      <ShareBar v-if="toolSegs.length" label="工具" :amount="fmtMs(totals.tool_ms)" :segments="toolSegs" :total="wall" :fmt="fmtMs" />
    </div>
    <template #footer>三行共用墙钟做分母，下两行是第一行对应段的展开；等用户类别的时长是在等人。</template>
  </DashCard>
</template>
