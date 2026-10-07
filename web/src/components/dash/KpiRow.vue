<script setup lang="ts">
import { computed } from 'vue'
import type { PricedTotals as SummaryTotals, PricingSource } from '../../lib/pricing'
import { fmtBig, fmtInt, fmtMoney, fmtMoneyExact, fmtMs, fmtPct } from '../../lib/format'
import { cacheHitRate, promptTokens as promptOf, totalTokens } from '../../lib/summaryModel'

// 一块只说三件事：是什么、多少、一句最要紧的补充。口径说明放在悬停提示里，不占版面。
const props = defineProps<{
  totals: SummaryTotals
  /** by_category.user.ms：工具时间里在等人回答的部分 */
  userWaitMs?: number
  pricing?: PricingSource
}>()

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
  const t = props.totals
  const priceSrc =
    props.pricing === 'snapshot' ? '价格目录拉取失败，用的是 llm-pricing 内置快照。' : '按各家一方 API 价折算（models.dev）。'
  return [
    {
      key: 'cost',
      icon: 'i-jannchie-currency-dollar',
      label: '等价费用',
      value: fmtMoney(t.cost_usd),
      sub: t.unpriced_steps > 0 ? `${fmtInt(t.unpriced_steps)} 步未定价` : undefined,
      subTone: 'ink-warning',
      title: `${fmtMoneyExact(t.cost_usd)}\n${priceSrc}订阅用户实际不按此付费；未定价的步数计 $0。`,
    },
    {
      key: 'tokens',
      icon: 'i-jannchie-sigma',
      label: 'tokens',
      value: fmtBig(totalTokens(t.tokens)),
      // 输入含缓存读写，输入 + 输出 = 总数
      sub: `输入 ${fmtBig(promptTokens.value)} · 输出 ${fmtBig(t.tokens.output)}`,
      title: `输入 ${fmtInt(promptTokens.value)}\n  新输入 ${fmtInt(t.tokens.input)}（未命中缓存）\n  缓存读 ${fmtInt(t.tokens.cache_read)}\n  缓存写 ${fmtInt(t.tokens.cache_write)}\n输出 ${fmtInt(t.tokens.output)}（含思考）`,
    },
    {
      key: 'cache',
      icon: 'i-jannchie-database',
      label: '缓存命中',
      value: fmtPct(hit.value),
      sub: saved.value > 0 ? `省下 ${fmtMoney(saved.value)}` : undefined,
      title:
        `缓存读 ${fmtInt(t.tokens.cache_read)}\ncache_read ÷ (input + cache_read + cache_write)\n\n` +
        `不用缓存（缓存读写都按新输入价）约 ${fmtMoneyExact(t.nocache_usd)}，实际 ${fmtMoneyExact(t.cost_usd)}，` +
        `省下 ${fmtMoneyExact(saved.value)}（已扣掉缓存写入的溢价）`,
    },
    {
      key: 'wall',
      icon: 'i-jannchie-hourglass',
      label: '墙钟时间',
      value: fmtMs(t.wall_ms),
      sub: `模型 ${fmtPct(share(t.model_ms), 0)} · 工具 ${fmtPct(share(t.tool_ms), 0)}`,
      title:
        `turn 开始到结束，含等用户。\n模型 ${fmtMs(t.model_ms)}（思考 + 输出）\n工具 ${fmtMs(t.tool_ms)}` +
        (props.userWaitMs ? `，其中等用户 ${fmtMs(props.userWaitMs)}` : '') +
        '\n都是从事件时间戳推的近似。',
    },
    {
      key: 'sessions',
      icon: 'i-jannchie-chat',
      label: '会话',
      value: fmtBig(t.sessions),
      sub: `${fmtBig(t.turns)} 轮 · ${fmtBig(t.steps)} 步`,
      title: `含子 agent 会话\n每轮平均 ${fmtMs(t.turns ? t.wall_ms / t.turns : null)}`,
    },
    {
      key: 'tools',
      icon: 'i-jannchie-toolbox',
      label: '工具调用',
      value: fmtBig(t.tool_calls),
      sub: `失败 ${fmtPct(failRate.value)}`,
      subTone: (failRate.value ?? 0) > 0.05 ? 'ink-danger' : undefined,
      title: `${fmtInt(t.tool_calls)} 次，失败 ${fmtInt(t.tool_failures)} 次\n失败按 tool_result 的状态判断`,
    },
  ]
})
</script>

<template>
  <div class="grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-6">
    <div v-for="k in kpis" :key="k.key" class="card flex flex-col gap-1" :title="k.title">
      <div class="flex items-center gap-1.5 text-xs ink-soft"><i class="h-3.5 w-3.5" :class="k.icon" />{{ k.label }}</div>
      <div class="text-2xl font-semibold leading-tight num text-neutral-900 dark:text-neutral-50">{{ k.value }}</div>
      <div v-if="k.sub" class="truncate text-xs num" :class="k.subTone ?? 'ink-muted'">{{ k.sub }}</div>
    </div>
  </div>
</template>
