<script setup lang="ts">
// 推理强度 / skill 两种切法看同一件事：钱和时间花在了哪种调用上。
// 两个维度都只覆盖一部分步（pfh 没有强度，大多数步不归属 skill），覆盖率写在标题下面。
import { computed, ref } from 'vue'
import { modelLabel, type PricedTag } from '../../lib/pricing'
import { fmtBig, fmtInt, fmtMoney, fmtMoneyExact, fmtMs, fmtPct } from '../../lib/format'
import { useChartTheme } from '../../lib/chartTheme'
import { useSort } from '../../lib/sort'
import DashCard from './DashCard.vue'
import SortTh from './SortTh.vue'
import Segmented from '../Segmented.vue'

const props = defineProps<{
  byEffort: Record<string, PricedTag>
  bySkill: Record<string, PricedTag>
  totalSteps: number
  totalCost: number
}>()
const t = useChartTheme()

type View = 'effort' | 'skill'
// Dashboard 只在两者至少有一个非空时才渲染这张卡
const view = ref<View>(Object.keys(props.byEffort).length ? 'effort' : 'skill')
const VIEWS = [
  { value: 'effort', label: '推理强度' },
  { value: 'skill', label: 'Skill' },
] as const

/** 强度是有序的：从低到高，没见过的排在后面 */
const LEVELS = ['minimal', 'low', 'medium', 'high', 'xhigh', 'max']
const level = (e: string) => {
  const i = LEVELS.indexOf(e.toLowerCase())
  return i < 0 ? LEVELS.length : i
}

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
    const color = view.value === 'effort' ? (t.value.seq[Math.min(t.value.seq.length - 1, 1 + order)] ?? t.value.accent) : t.value.accent
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
    ? `日志记了强度的步占 ${s}，费用占 ${c}`
    : `归属到 skill 的步占 ${s}，费用 ${fmtMoney(cost)}（${c}）`
})

function tip(r: Row): string {
  const models = Object.entries(r.a.by_model)
    .sort(([, x], [, y]) => y.cost_usd - x.cost_usd)
    .map(([m, x]) => `  ${modelLabel(m)}  ${fmtInt(x.steps)} 步 · ${x.unpriced ? '未定价' : fmtMoneyExact(x.cost_usd)}`)
  return [
    r.name,
    `${fmtInt(r.a.steps)} 步 · 输出 ${fmtInt(r.a.tokens.output)} · 模型时间 ${fmtMs(r.a.model_ms)}`,
    `费用 ${r.a.unpriced ? '含未定价模型' : fmtMoneyExact(r.a.cost_usd)}`,
    '按模型：',
    ...models,
  ].join('\n')
}

const td = 'py-1 pl-3 text-right text-xs num'
</script>

<template>
  <DashCard
    icon="i-carbon-meter"
    title="推理强度与 Skill"
    title-hint="强度来自每次调用记下的 effort（Claude Code）或 turn_context 的 reasoning_effort（Codex）；skill 是 Claude Code 记的 attributionSkill。费用按整段范围的价格折算"
    :caption="caption"
  >
    <template #controls>
      <Segmented v-model="view" label="维度" :options="VIEWS" />
    </template>
    <div v-if="rows.length === 0" class="py-8 text-center text-sm ink-soft">
      {{ view === 'effort' ? '范围内的调用都没有记推理强度。' : '范围内没有归属到 skill 的调用。' }}
    </div>
    <div v-else class="-mx-1 overflow-x-auto px-1">
      <table class="w-full border-collapse whitespace-nowrap text-sm">
        <thead>
          <tr class="text-xs ink-soft">
            <SortTh :sort="sort" k="name" align="left">{{ view === 'effort' ? '强度' : 'Skill' }}</SortTh>
            <SortTh :sort="sort" k="cost" align="left" class="pl-3">费用</SortTh>
            <SortTh :sort="sort" k="steps" class="pl-3">steps</SortTh>
            <SortTh :sort="sort" k="perStepCost" class="pl-3" title="费用 ÷ steps">每步费用</SortTh>
            <SortTh :sort="sort" k="perStepOut" class="pl-3 max-sm:hidden" title="输出 token（含思考）÷ steps">每步输出</SortTh>
            <SortTh :sort="sort" k="perStepMs" class="pl-3" title="模型时间 ÷ steps">每步时间</SortTh>
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
  </DashCard>
</template>
