<script setup lang="ts">
// 会话概况：计数、计费 token 按组成拆、时间拆分。对应 dsh-context 的 Context / Token / Timing Stats 三张卡。
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useChartTheme } from '../../lib/chartTheme'
import { ALL_CATS } from '../../lib/context'
import { catColor, catLabel, type CtxSummary } from '../../lib/ctxView'
import { fmtBig, fmtInt, fmtMoney, fmtMoneyExact, fmtMs, fmtPct } from '../../lib/format'
import DashCard from '../dash/DashCard.vue'
import StatGrid, { type Stat } from '../dash/StatGrid.vue'
import CompBar, { type BarSeg } from './CompBar.vue'

const props = defineProps<{
  s: CtxSummary
  /** 子 agent 一起算的费用；没有子 agent 时不给 */
  familyCost?: number | null
}>()
const { t } = useI18n()
const theme = useChartTheme()

const counts = computed<Stat[]>(() => [
  { key: 'turns', label: t('ctx.stats.turns'), value: fmtInt(props.s.turns), sub: `${fmtInt(props.s.steps)} ${t('ctx.stats.steps')}` },
  { key: 'human', label: t('ctx.stats.humanInputs'), value: fmtInt(props.s.humanInputs), sub: `${fmtInt(props.s.answers)} ${t('ctx.stats.answers')}` },
  {
    key: 'tools',
    label: t('ctx.stats.toolCalls'),
    value: fmtInt(props.s.toolCalls),
    sub: props.s.toolFailures ? t('ctx.stats.failures', { n: fmtInt(props.s.toolFailures) }) : undefined,
  },
  {
    key: 'ctx',
    label: t('ctx.stats.compactions'),
    value: fmtInt(props.s.compactions),
    sub: `${fmtInt(props.s.injects)} ${t('ctx.stats.injects')} · ${fmtInt(props.s.skills)} ${t('ctx.stats.skills')}`,
  },
  {
    key: 'cost',
    label: t('ctx.stats.cost'),
    value: fmtMoney(props.s.cost),
    sub: props.familyCost != null ? `${fmtMoney(props.familyCost)} ${t('ctx.stats.family')}` : props.s.unpriced ? t('ctx.stats.unpriced') : undefined,
    title: `${fmtMoneyExact(props.s.cost)}\n${t('ctx.stats.costHint')}`,
  },
])

const splitSegs = computed<BarSeg[]>(() => [
  ...ALL_CATS.map((c) => ({ key: c, label: catLabel(c), value: Math.round(props.s.split[c]), color: catColor(c, theme.value) })),
  { key: 'output', label: t('ctx.tokens.output'), value: props.s.split.output, color: theme.value.fg2 },
])

const timing = computed(() => {
  const s = props.s
  const wall = Math.max(s.wallMs, s.modelMs + s.toolMs)
  const ttft = Math.min(s.ttftMs ?? 0, wall)
  const model = Math.min(s.modelMs - ttft, wall - ttft)
  const tools = Math.min(s.toolMs, wall - ttft - model)
  const idle = Math.max(0, wall - ttft - model - tools)
  const th = theme.value
  const segs: BarSeg[] = [
    { key: 'ttft', label: t('ctx.timing.ttft'), value: ttft, color: th.cat[3]! },
    { key: 'model', label: t('ctx.timing.model'), value: model, color: th.accent },
    { key: 'tools', label: t('ctx.timing.tools'), value: tools, color: th.tool },
    { key: 'idle', label: t('ctx.timing.idle'), value: idle, color: th.other },
  ]
  return { wall, segs }
})
</script>

<template>
  <DashCard icon="i-jannchie-gauge-list" :title="t('ctx.stats.title')">
    <div class="flex flex-col gap-5">
      <StatGrid :stats="counts" />

      <div class="grid gap-5 lg:grid-cols-2">
        <section class="flex min-w-0 flex-col gap-2">
          <div class="flex items-baseline justify-between gap-3">
            <h3 class="m-0 text-xs font-medium ink-soft">{{ t('ctx.tokens.billed') }}</h3>
            <span class="text-xs ink-soft" :title="t('ctx.tokens.cacheHitHint')">
              {{ t('ctx.tokens.cacheHit') }} <span class="num ink-strong">{{ fmtPct(s.cacheHit, 2) }}</span>
            </span>
          </div>
          <div class="text-xl font-semibold leading-tight num" :title="fmtInt(s.billed)">{{ fmtBig(s.billed) }}</div>
          <CompBar :segs="splitSegs" :height="12" />
          <ul class="m-0 grid list-none grid-cols-2 gap-x-4 gap-y-1 p-0 text-xs sm:grid-cols-3" :aria-label="t('ctx.tokens.split')">
            <li v-for="seg in splitSegs.filter((x) => x.value > 0)" :key="seg.key" class="flex min-w-0 items-center gap-1.5">
              <span class="h-2 w-2 shrink-0 rounded-full" :style="{ background: seg.color }" />
              <span class="min-w-0 flex-1 truncate ink-muted">{{ seg.label }}</span>
              <span class="num">{{ fmtBig(seg.value) }}</span>
            </li>
          </ul>
        </section>

        <section class="flex min-w-0 flex-col gap-2">
          <div class="flex items-baseline justify-between gap-3">
            <h3 class="m-0 text-xs font-medium ink-soft">{{ t('ctx.timing.wall') }}</h3>
            <span v-if="s.outputPerSec" class="text-xs ink-soft">
              {{ t('ctx.timing.tps') }} <span class="num ink-strong">{{ t('ctx.timing.tpsUnit', { n: s.outputPerSec.toFixed(1) }) }}</span>
            </span>
          </div>
          <div class="text-xl font-semibold leading-tight num">{{ fmtMs(timing.wall) }}</div>
          <CompBar :segs="timing.segs" :height="12" />
          <ul class="m-0 grid list-none grid-cols-2 gap-x-4 gap-y-1 p-0 text-xs sm:grid-cols-4">
            <li
              v-for="seg in timing.segs.filter((x) => x.value > 0)"
              :key="seg.key"
              class="flex min-w-0 items-center gap-1.5"
              :title="seg.key === 'idle' ? t('ctx.timing.idleHint') : undefined"
            >
              <span class="h-2 w-2 shrink-0 rounded-full" :style="{ background: seg.color }" />
              <span class="min-w-0 flex-1 truncate ink-muted">{{ seg.label }}</span>
              <span class="num">{{ fmtMs(seg.value) }}</span>
            </li>
          </ul>
          <div v-if="s.slowTools.length" class="flex flex-wrap gap-x-3 gap-y-1 text-xs ink-soft">
            <span>{{ t('ctx.timing.slowTools') }}</span>
            <span v-for="[name, x] in s.slowTools" :key="name" class="ink-muted">
              <span class="font-mono">{{ name }}</span> <span class="num">{{ fmtMs(x.ms) }}</span>
            </span>
          </div>
        </section>
      </div>
    </div>
  </DashCard>
</template>
