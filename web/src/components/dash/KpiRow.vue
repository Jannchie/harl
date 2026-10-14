<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { PricedTotals as SummaryTotals, PricingSource } from '../../lib/pricing'
import { fmtBig, fmtInt, fmtMoney, fmtMoneyExact, fmtMs, fmtPct, tn } from '../../lib/format'
import { cacheHitRate, promptTokens as promptOf, totalTokens } from '../../lib/summaryModel'

// 一块只说三件事：是什么、多少、一句最要紧的补充。口径说明放在悬停提示里，不占版面。
const props = defineProps<{
  totals: SummaryTotals
  /** by_category.user.ms：工具时间里在等人回答的部分 */
  userWaitMs?: number
  pricing?: PricingSource
}>()

const { t } = useI18n()

const hit = computed(() => cacheHitRate(props.totals.tokens))
/** 缓存省下的钱：假如没有缓存的费用 − 实际费用 */
const saved = computed(() => props.totals.nocache_usd - props.totals.cost_usd)
const promptTokens = computed(() => promptOf(props.totals.tokens))
const failRate = computed(() => (props.totals.tool_calls > 0 ? props.totals.tool_failures / props.totals.tool_calls : null))
const share = (ms: number) => (props.totals.wall_ms > 0 ? ms / props.totals.wall_ms : null)

interface Kpi {
  key: string
  icon: string
  label: string
  value: string
  sub?: string
  /** sub 要提醒的时候用 */
  subTone?: string
  title: string
}

const kpis = computed<Kpi[]>(() => {
  const totals = props.totals
  const priceSrc = props.pricing === 'snapshot' ? t('dash.kpi.priceSnapshot') : t('dash.kpi.priceLive')
  return [
    {
      key: 'cost',
      icon: 'i-jannchie-currency-dollar',
      label: t('dash.kpi.cost'),
      value: fmtMoney(totals.cost_usd),
      sub: totals.unpriced_steps > 0 ? tn('dash.kpi.unpriced', totals.unpriced_steps) : undefined,
      // 未定价的 tokens 占到 1% 以上，总费用才明显偏低，这时才标黄
      subTone: totals.unpriced_tokens > 0.01 * totalTokens(totals.tokens) ? 'ink-warning' : undefined,
      title: t('dash.kpi.costTitle', { exact: fmtMoneyExact(totals.cost_usd), src: priceSrc }),
    },
    {
      key: 'tokens',
      icon: 'i-jannchie-sigma',
      label: 'tokens',
      value: fmtBig(totalTokens(totals.tokens)),
      // 输入含缓存读写，输入 + 输出 = 总数
      sub: t('dash.kpi.tokensSub', { input: fmtBig(promptTokens.value), output: fmtBig(totals.tokens.output) }),
      title: t('dash.kpi.tokensTitle', {
        prompt: fmtInt(promptTokens.value),
        input: fmtInt(totals.tokens.input),
        read: fmtInt(totals.tokens.cache_read),
        write: fmtInt(totals.tokens.cache_write),
        output: fmtInt(totals.tokens.output),
      }),
    },
    {
      key: 'cache',
      icon: 'i-jannchie-database',
      label: t('dash.kpi.cache'),
      value: fmtPct(hit.value),
      sub: saved.value > 0 ? t('dash.kpi.saved', { v: fmtMoney(saved.value) }) : undefined,
      title: t('dash.kpi.cacheTitle', {
        read: fmtInt(totals.tokens.cache_read),
        nocache: fmtMoneyExact(totals.nocache_usd),
        actual: fmtMoneyExact(totals.cost_usd),
        saved: fmtMoneyExact(saved.value),
      }),
    },
    {
      key: 'wall',
      icon: 'i-jannchie-hourglass',
      label: t('common.metric.wall'),
      value: fmtMs(totals.wall_ms),
      sub: t('dash.kpi.wallSub', { model: fmtPct(share(totals.model_ms), 0), tool: fmtPct(share(totals.tool_ms), 0) }),
      title: props.userWaitMs
        ? t('dash.kpi.wallTitleWait', { model: fmtMs(totals.model_ms), tool: fmtMs(totals.tool_ms), wait: fmtMs(props.userWaitMs) })
        : t('dash.kpi.wallTitle', { model: fmtMs(totals.model_ms), tool: fmtMs(totals.tool_ms) }),
    },
    {
      key: 'sessions',
      icon: 'i-jannchie-chat',
      label: t('common.metric.sessions'),
      value: fmtBig(totals.sessions),
      sub: t('dash.kpi.sessionsSub', { turns: fmtBig(totals.turns), steps: fmtBig(totals.steps) }),
      title: t('dash.kpi.sessionsTitle', { avg: fmtMs(totals.turns ? totals.wall_ms / totals.turns : null) }),
    },
    {
      key: 'tools',
      icon: 'i-jannchie-toolbox',
      label: t('common.metric.toolCalls'),
      value: fmtBig(totals.tool_calls),
      sub: t('dash.kpi.failSub', { v: fmtPct(failRate.value) }),
      subTone: (failRate.value ?? 0) > 0.05 ? 'ink-danger' : undefined,
      title: t('dash.kpi.toolsTitle', { calls: fmtInt(totals.tool_calls), fails: fmtInt(totals.tool_failures) }),
    },
  ]
})
</script>

<template>
  <div class="grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-6">
    <div v-for="k in kpis" :key="k.key" class="card flex flex-col gap-1" :title="k.title">
      <div class="flex items-center gap-1.5 text-xs ink-soft"><i class="h-3.5 w-3.5" :class="k.icon" />{{ k.label }}</div>
      <div class="text-2xl font-semibold leading-tight num text-ink">{{ k.value }}</div>
      <div v-if="k.sub" class="truncate text-xs num" :class="k.subTone ?? 'ink-muted'">{{ k.sub }}</div>
    </div>
  </div>
</template>
