<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { SummaryTotals, ToolAgg, ToolCategory } from '../../api'
import { useChartTheme } from '../../lib/chartTheme'
import { fmtInt, fmtList, fmtMs, tn } from '../../lib/format'
import { modelLabel, type PricedModelAgg } from '../../lib/pricing'
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
const theme = useChartTheme()
const { t } = useI18n()

const TOP_MODELS = 7

/** 分母：墙钟；模型 + 工具 + hook 偶尔会超过墙钟（并行的子 agent），取大的那个 */
const wall = computed(() => Math.max(props.totals.wall_ms, props.totals.model_ms + props.totals.tool_ms + props.totals.hook_ms))

/** 模型时间按模型拆，按时长降序；前 7 个单列，其余折成"其他模型" */
const modelSegs = computed<ShareSeg[]>(() => {
  const th = theme.value
  const pal = modelPalette(props.byModel, th)
  const sorted = Object.entries(props.byModel)
    .filter(([, a]) => a.model_ms > 0)
    .sort(([, a], [, b]) => b.model_ms - a.model_ms)
  const head: ShareSeg[] = sorted.slice(0, TOP_MODELS).map(([m, a]) => ({
    key: m,
    name: modelLabel(m),
    value: a.model_ms,
    color: pal.color(m),
    hint: `${fmtInt(a.steps)} steps`,
  }))
  const tail = sorted.slice(TOP_MODELS)
  const rest = tail.reduce((s, [, a]) => s + a.model_ms, 0)
  if (rest > 0) {
    head.push({ key: '__other', name: t('cards.timeShare.otherModels', { n: tail.length }), value: rest, color: th.other, tip: fmtList(tail.map(([m]) => modelLabel(m))) })
  }
  return head
})

/** 工具时间按类别拆，按时长降序 */
const toolSegs = computed<ShareSeg[]>(() => {
  const th = theme.value
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
      hint: tn('cards.common.times', x.calls),
      tip: `${c} · ${categoryLabel(c)}\n${fmtList((names.get(c) ?? []).sort())}`,
    }))
})

/** 墙钟 = 模型 + 工具 + hook + 其他等待；按时长排，其他等待是墙钟剩下的，IcicleBar 画在最后 */
const groups = computed<IcicleGroup[]>(() => {
  const th = theme.value
  const hook = props.totals.hook_ms
  return [
    { key: 'model', name: t('common.metric.model'), value: props.totals.model_ms, cap: th.fg2, segs: modelSegs.value },
    { key: 'tool', name: t('common.metric.tool'), value: props.totals.tool_ms, cap: th.other, segs: toolSegs.value },
    // 没配 hook 的就不占位
    ...(hook > 0
      ? [{ key: 'hook', name: 'hook', value: hook, cap: th.other, segs: [{ key: 'stop', name: 'stop hook', value: hook, color: th.other, tip: t('cards.timeShare.stopHookHint') }] }]
      : []),
  ].sort((a, b) => b.value - a.value)
})
const IDLE = computed(() => ({ name: t('cards.timeShare.idle'), tip: t('cards.timeShare.idleHint') }))
</script>

<template>
  <DashCard
    icon="i-jannchie-clock"
    :title="t('common.metric.time')"
    :title-hint="t('cards.timeShare.titleHint')"
  >
    <div v-if="wall === 0" class="py-8 text-center text-sm ink-soft">{{ t('cards.timeShare.empty') }}</div>
    <IcicleBar v-else :label="t('common.metric.wall')" :amount="fmtMs(wall)" :groups="groups" :total="wall" :rest="IDLE" :fmt="fmtMs" :of-label="t('cards.timeShare.ofWall')" />
  </DashCard>
</template>
