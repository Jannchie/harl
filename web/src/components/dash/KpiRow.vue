<script setup lang="ts">
import { computed } from 'vue'
import type { SummaryTotals } from '../../api'
import { fmtBig, fmtInt, fmtMoney, fmtMoneyExact, fmtMs, fmtPct } from '../../lib/format'
import { cacheHitRate } from '../../lib/summaryModel'

const props = defineProps<{
  totals: SummaryTotals
  /** by_category.user.ms：工具时间里在等人回答的部分 */
  userWaitMs?: number
}>()

const hit = computed(() => cacheHitRate(props.totals.tokens))
const failRate = computed(() =>
  props.totals.tool_calls > 0 ? props.totals.tool_failures / props.totals.tool_calls : null,
)
const unpricedShare = computed(() =>
  props.totals.steps > 0 ? props.totals.unpriced_steps / props.totals.steps : null,
)
const msPerStep = computed(() => (props.totals.steps > 0 ? props.totals.model_ms / props.totals.steps : null))
const toolShare = computed(() => (props.totals.wall_ms > 0 ? props.totals.tool_ms / props.totals.wall_ms : null))
const modelShare = computed(() => (props.totals.wall_ms > 0 ? props.totals.model_ms / props.totals.wall_ms : null))
</script>

<template>
  <div class="kpis">
    <div class="tile hero" :title="fmtMoneyExact(totals.cost_usd)">
      <div class="label">API 等价费用</div>
      <div class="value">{{ fmtMoney(totals.cost_usd) }}</div>
      <div class="sub">
        <template v-if="totals.unpriced_steps > 0">
          <span class="warn">▲</span> {{ fmtInt(totals.unpriced_steps) }} 步未定价（{{ fmtPct(unpricedShare) }}）计 $0
        </template>
        <template v-else>全部步数都有定价</template>
      </div>
      <div class="note">按 Anthropic 一方 API 价折算；订阅用户实际不按此付费</div>
    </div>

    <div class="tile" :title="fmtInt(totals.tokens.input)">
      <div class="label">输入 tokens</div>
      <div class="value">{{ fmtBig(totals.tokens.input) }}</div>
      <div class="sub">缓存写入 {{ fmtBig(totals.tokens.cache_write) }}</div>
      <div class="note">不含缓存命中；未缓存的新输入</div>
    </div>

    <div class="tile" :title="fmtInt(totals.tokens.output)">
      <div class="label">输出 tokens</div>
      <div class="value">{{ fmtBig(totals.tokens.output) }}</div>
      <div class="sub">缓存读取 {{ fmtBig(totals.tokens.cache_read) }}</div>
      <div class="note">模型生成的全部 token，含思考</div>
    </div>

    <div class="tile">
      <div class="label">缓存命中率</div>
      <div class="value">{{ fmtPct(hit) }}</div>
      <div class="sub">
        <span class="meter" aria-hidden="true"><span class="fill" :style="{ width: `${(hit ?? 0) * 100}%` }"></span></span>
      </div>
      <div class="note">cache_read ÷ (input + cache_read + cache_write)</div>
    </div>

    <div class="tile">
      <div class="label">模型时间</div>
      <div class="value">{{ fmtMs(totals.model_ms) }}</div>
      <div class="sub">每步 {{ fmtMs(msPerStep) }} · 占墙钟 {{ fmtPct(modelShare, 0) }}</div>
      <div class="note">思考 + 输出，由事件时间戳推算</div>
    </div>

    <div class="tile">
      <div class="label">工具时间</div>
      <div class="value">{{ fmtMs(totals.tool_ms) }}</div>
      <div class="sub">
        占墙钟 {{ fmtPct(toolShare, 0) }}<template v-if="userWaitMs"> · 含等用户 {{ fmtMs(userWaitMs) }}</template>
      </div>
      <div class="note">tool_call → tool_result 之间的等待</div>
    </div>

    <div class="tile">
      <div class="label">墙钟时间</div>
      <div class="value">{{ fmtMs(totals.wall_ms) }}</div>
      <div class="sub">{{ fmtInt(totals.turns) }} turns · 每 turn {{ fmtMs(totals.turns ? totals.wall_ms / totals.turns : null) }}</div>
      <div class="note">turn 开始到结束，含用户等待</div>
    </div>

    <div class="tile">
      <div class="label">活动量</div>
      <div class="value triple">
        <span :title="'会话'">{{ fmtBig(totals.sessions) }}<small>会话</small></span>
        <span :title="'turns'">{{ fmtBig(totals.turns) }}<small>turn</small></span>
        <span :title="'工具调用'">{{ fmtBig(totals.tool_calls) }}<small>调用</small></span>
      </div>
      <div class="sub">
        <span :class="{ danger: (failRate ?? 0) > 0.05 }">{{ fmtInt(totals.tool_failures) }} 次失败（{{ fmtPct(failRate) }}）</span>
      </div>
      <div class="note">含子 agent 会话；失败按 tool_result 状态判断</div>
    </div>
  </div>
</template>

<style scoped>
.kpis {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 1px;
  background: var(--line);
  border: 1px solid var(--line);
}
.tile {
  background: var(--bg);
  padding: 10px 14px 9px;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.label {
  font-size: var(--fs-xs);
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--fg-2);
}
.value {
  font-size: 26px;
  line-height: 1.15;
  font-weight: 600;
  letter-spacing: -0.01em;
  color: var(--fg);
  margin-top: 2px;
}
.hero .value {
  font-size: 40px;
  line-height: 1.05;
}
.hero {
  grid-row: span 1;
}
.triple {
  display: flex;
  gap: 14px;
  flex-wrap: wrap;
  font-size: 22px;
}
.triple small {
  font-size: var(--fs-xs);
  font-weight: 400;
  color: var(--fg-2);
  margin-left: 3px;
  letter-spacing: 0.04em;
}
.sub {
  font-size: var(--fs-sm);
  color: var(--fg-1);
  font-variant-numeric: tabular-nums;
  min-height: 1.4em;
}
.note {
  font-size: var(--fs-xs);
  color: var(--fg-2);
  margin-top: auto;
  padding-top: 4px;
}
.warn {
  color: var(--tool);
  font-size: 9px;
  vertical-align: 1px;
}
.meter {
  display: block;
  height: 4px;
  width: 100%;
  max-width: 160px;
  background: var(--seq-1);
  margin-top: 6px;
}
.meter .fill {
  display: block;
  height: 100%;
  background: var(--seq-5);
}
@media (max-width: 1100px) {
  .kpis {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}
@media (max-width: 560px) {
  .kpis {
    grid-template-columns: minmax(0, 1fr);
  }
}
</style>
