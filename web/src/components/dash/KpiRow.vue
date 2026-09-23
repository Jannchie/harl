<script setup lang="ts">
import { computed } from 'vue'
import type { PricedTotals as SummaryTotals, PricingSource } from '../../lib/pricing'
import { fmtBig, fmtInt, fmtMoney, fmtMoneyExact, fmtMs, fmtPct } from '../../lib/format'
import { cacheHitRate } from '../../lib/summaryModel'

const props = defineProps<{
  totals: SummaryTotals
  /** by_category.user.ms：工具时间里在等人回答的部分 */
  userWaitMs?: number
  pricing?: PricingSource
}>()

const priceNote = computed(() =>
  props.pricing === 'snapshot'
    ? '价格目录拉取失败，用的是 llm-pricing 内置快照；订阅用户实际不按此付费'
    : '按各家一方 API 价折算（models.dev）；订阅用户实际不按此付费',
)

const hit = computed(() => cacheHitRate(props.totals.tokens))
const failRate = computed(() =>
  props.totals.tool_calls > 0 ? props.totals.tool_failures / props.totals.tool_calls : null,
)
const unpricedShare = computed(() =>
  props.totals.steps > 0 ? props.totals.unpriced_steps / props.totals.steps : null,
)
// 每块共用的几档字
const label = 'flex items-center gap-1.5 text-xs ink-soft'
const value = 'text-2xl font-semibold leading-tight num text-neutral-900 dark:text-neutral-50'
const sub = 'text-xs ink-muted'
const note = 'mt-auto pt-1 text-xs ink-soft'
const unit = 'ml-1 text-xs font-normal ink-soft font-sans'

const msPerStep = computed(() => (props.totals.steps > 0 ? props.totals.model_ms / props.totals.steps : null))
const toolShare = computed(() => (props.totals.wall_ms > 0 ? props.totals.tool_ms / props.totals.wall_ms : null))
const modelShare = computed(() => (props.totals.wall_ms > 0 ? props.totals.model_ms / props.totals.wall_ms : null))
</script>

<template>
  <div class="grid grid-cols-2 gap-3 lg:grid-cols-4">
    <div class="card flex flex-col gap-1" :title="fmtMoneyExact(totals.cost_usd)">
      <div :class="label"><i class="i-carbon-currency-dollar h-3.5 w-3.5" />API 等价费用</div>
      <div :class="value">{{ fmtMoney(totals.cost_usd) }}</div>
      <div :class="sub">
        <template v-if="totals.unpriced_steps > 0">
          <span class="ink-warning">{{ fmtInt(totals.unpriced_steps) }} 步未定价</span>（{{ fmtPct(unpricedShare) }}）计 $0
        </template>
        <template v-else>全部步数都有定价</template>
      </div>
      <div :class="note">{{ priceNote }}</div>
    </div>

    <div class="card flex flex-col gap-1" :title="fmtInt(totals.tokens.input)">
      <div :class="label"><i class="i-carbon-arrow-down h-3.5 w-3.5" />输入 tokens</div>
      <div :class="value">{{ fmtBig(totals.tokens.input) }}</div>
      <div :class="sub">缓存写入 <span class="num">{{ fmtBig(totals.tokens.cache_write) }}</span></div>
      <div :class="note">不含缓存命中；未缓存的新输入</div>
    </div>

    <div class="card flex flex-col gap-1" :title="fmtInt(totals.tokens.output)">
      <div :class="label"><i class="i-carbon-arrow-up h-3.5 w-3.5" />输出 tokens</div>
      <div :class="value">{{ fmtBig(totals.tokens.output) }}</div>
      <div :class="sub">缓存读取 <span class="num">{{ fmtBig(totals.tokens.cache_read) }}</span></div>
      <div :class="note">模型生成的全部 token，含思考</div>
    </div>

    <div class="card flex flex-col gap-1">
      <div :class="label"><i class="i-carbon-data-base h-3.5 w-3.5" />缓存命中率</div>
      <div :class="value">{{ fmtPct(hit) }}</div>
      <div class="meter-track mt-1.5 h-1.5 max-w-40">
        <span class="meter-fill bg-blue-500" :style="{ width: `${(hit ?? 0) * 100}%` }"></span>
      </div>
      <div :class="note">cache_read ÷ (input + cache_read + cache_write)</div>
    </div>

    <div class="card flex flex-col gap-1">
      <div :class="label"><i class="i-carbon-chip h-3.5 w-3.5" />模型时间</div>
      <div :class="value">{{ fmtMs(totals.model_ms) }}</div>
      <div :class="sub">每步 <span class="num">{{ fmtMs(msPerStep) }}</span> · 占墙钟 <span class="num">{{ fmtPct(modelShare, 0) }}</span></div>
      <div :class="note">思考 + 输出，由事件时间戳推算</div>
    </div>

    <div class="card flex flex-col gap-1">
      <div :class="label"><i class="i-carbon-tool-kit h-3.5 w-3.5" />工具时间</div>
      <div :class="value">{{ fmtMs(totals.tool_ms) }}</div>
      <div :class="sub">
        占墙钟 <span class="num">{{ fmtPct(toolShare, 0) }}</span
        ><template v-if="userWaitMs"> · 含等用户 <span class="num">{{ fmtMs(userWaitMs) }}</span></template>
      </div>
      <div :class="note">tool_call → tool_result 之间的等待</div>
    </div>

    <div class="card flex flex-col gap-1">
      <div :class="label"><i class="i-carbon-hourglass h-3.5 w-3.5" />墙钟时间</div>
      <div :class="value">{{ fmtMs(totals.wall_ms) }}</div>
      <div :class="sub">
        <span class="num">{{ fmtInt(totals.turns) }}</span> turns · 每 turn
        <span class="num">{{ fmtMs(totals.turns ? totals.wall_ms / totals.turns : null) }}</span>
      </div>
      <div :class="note">turn 开始到结束，含用户等待</div>
    </div>

    <div class="card flex flex-col gap-1">
      <div :class="label"><i class="i-carbon-activity h-3.5 w-3.5" />活动量</div>
      <div class="flex flex-wrap items-baseline gap-x-3 text-xl font-semibold num">
        <span title="会话">{{ fmtBig(totals.sessions) }}<small :class="unit">会话</small></span>
        <span title="turns">{{ fmtBig(totals.turns) }}<small :class="unit">turn</small></span>
        <span title="工具调用">{{ fmtBig(totals.tool_calls) }}<small :class="unit">调用</small></span>
      </div>
      <div :class="sub">
        <span :class="{ 'ink-danger': (failRate ?? 0) > 0.05 }"
          ><span class="num">{{ fmtInt(totals.tool_failures) }}</span> 次失败（<span class="num">{{ fmtPct(failRate) }}</span>）</span
        >
      </div>
      <div :class="note">含子 agent 会话；失败按 tool_result 状态判断</div>
    </div>
  </div>
</template>
