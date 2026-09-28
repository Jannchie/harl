<script setup lang="ts">
import { computed } from 'vue'
import { RouterLink } from 'vue-router'
import type { PricedSummary } from '../../lib/pricing'
import { fmtDateTime, fmtInt, fmtMoney, fmtMs, fmtPct, truncate } from '../../lib/format'
import { providerDot, providerName } from '../../lib/providers'
import DashCard from './DashCard.vue'

const props = defineProps<{ d: PricedSummary['turn_durations'] }>()

/** 桶的上界写成 10s / 2min / 1h */
function edge(ms: number): string {
  const s = ms / 1000
  return s < 60 ? `${s}s` : s < 3600 ? `${s / 60}min` : `${s / 3600}h`
}

const total = computed(() => props.d.counts.reduce((a, n) => a + n, 0))
const bars = computed(() => {
  const e = props.d.edges_ms
  const max = Math.max(1, ...props.d.counts)
  return props.d.counts.map((n, i) => ({
    label: i === 0 ? `<${edge(e[0]!)}` : i === e.length ? `≥${edge(e[i - 1]!)}` : `${edge(e[i - 1]!)}–${edge(e[i]!)}`,
    n,
    share: total.value > 0 ? n / total.value : 0,
    height: n / max,
  }))
})

const stats = computed(() => [
  { key: 'p50', label: '中位数', value: fmtMs(props.d.p50_ms), title: '一半的 turn 在这个时间内结束' },
  { key: 'p90', label: 'P90', value: fmtMs(props.d.p90_ms), title: '九成的 turn 在这个时间内结束' },
  { key: 'max', label: '最长', value: fmtMs(props.d.max_ms), title: '范围内最慢的一个 turn' },
])
</script>

<template>
  <DashCard
    icon="i-carbon-hourglass"
    title="turn 耗时"
    title-hint="每个 turn 从用户发话到 agent 停下的墙钟时间（Claude Code 用日志里的 turn_duration，其他 harness 用首尾事件之差）。等用户回答的时间也算在内。"
    :caption="`${fmtInt(total)} 个 turn`"
  >
    <div class="grid grid-cols-[auto_1fr] gap-x-6 gap-y-4 max-md:grid-cols-1">
      <div class="flex flex-col gap-3 max-md:flex-row max-md:gap-6">
        <div v-for="s in stats" :key="s.key" :title="s.title">
          <div class="text-xs ink-soft">{{ s.label }}</div>
          <div class="text-xl font-semibold leading-tight num">{{ s.value }}</div>
        </div>
      </div>
      <!-- 分布：桶宽不等（越往后越宽），所以只画柱不画连续轴 -->
      <div class="flex h-36 items-end gap-1.5" role="img" :aria-label="bars.map((b) => `${b.label} ${b.n}`).join('，')">
        <div
          v-for="b in bars"
          :key="b.label"
          class="h-full min-w-0 flex flex-1 flex-col items-center gap-1"
          :title="`${b.label}：${fmtInt(b.n)} 个 turn（${fmtPct(b.share)}）`"
        >
          <!-- 柱区单独占满剩余高度，百分比标签在柱顶上方，不会顶出卡片 -->
          <div class="min-h-0 w-full flex flex-1 flex-col items-center justify-end gap-0.5">
            <span class="text-[10px] ink-soft num">{{ b.n ? fmtPct(b.share, 0) : '' }}</span>
            <div class="w-full rounded-t bg-[var(--accent)] opacity-80" :style="{ height: `${Math.max(b.height * 85, b.n ? 2 : 0)}%` }" />
          </div>
          <span class="w-full truncate text-center text-[10px] ink-soft num">{{ b.label }}</span>
        </div>
      </div>
    </div>

    <div v-if="d.slowest.length" class="mt-4">
      <div class="mb-1 text-xs ink-soft">最慢的 turn</div>
      <table class="w-full table-fixed border-collapse text-sm">
        <tbody>
          <tr v-for="t in d.slowest" :key="`${t.session_id}:${t.turn}`" class="surface-hover">
            <td class="rounded-l-lg py-1 pl-1 pr-3">
              <RouterLink
                :to="{ name: 'session', params: { id: t.session_id }, hash: `#turn-t:${t.turn}` }"
                class="min-w-0 flex items-center gap-2 kbd-ring"
                :title="`${providerName(t.provider)} · ${fmtDateTime(t.ts)}\n${t.title ?? '（无标题）'}`"
              >
                <span class="h-2 w-2 shrink-0 rounded-full" :style="{ background: providerDot(t.provider) }" aria-hidden="true"></span>
                <span class="min-w-0 truncate">{{ truncate(t.title ?? '（无标题）', 80) }}</span>
              </RouterLink>
            </td>
            <td class="w-28 py-1 text-right text-xs ink-soft num max-sm:hidden">{{ fmtDateTime(t.ts).slice(0, 16) }}</td>
            <td class="w-24 py-1 text-right text-xs num max-sm:hidden" :title="`${fmtInt(t.steps)} 步 · ${fmtInt(t.tool_calls)} 次工具`">
              {{ fmtInt(t.steps) }} 步
            </td>
            <td class="w-16 whitespace-nowrap py-1 text-right text-xs num" :class="{ 'ink-soft': t.unpriced }">{{ fmtMoney(t.cost_usd) }}</td>
            <td
              class="w-44 whitespace-nowrap rounded-r-lg py-1 pl-3 pr-1 text-right text-xs num"
              :title="`墙钟 ${fmtMs(t.wall_ms)}，其中模型 + 工具 ${fmtMs(t.active_ms)}；其余是等人或闲置`"
            >
              <span class="font-semibold">{{ fmtMs(t.wall_ms) }}</span>
              <span class="ink-soft max-sm:hidden"> · 活跃 {{ fmtMs(t.active_ms) }}</span>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </DashCard>
</template>
