<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { RouterLink } from 'vue-router'
import type { PricedSummary } from '../../lib/pricing'
import { fmtDateTime, fmtList, fmtMoney, fmtMs, fmtPct, tn, truncate } from '../../lib/format'
import { providerDot, providerName } from '../../lib/providers'
import DashCard from './DashCard.vue'

const props = defineProps<{ d: PricedSummary['turn_durations'] }>()
const { t } = useI18n()

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
  { key: 'p50', label: t('cards.turnDurations.median'), value: fmtMs(props.d.p50_ms), title: t('cards.turnDurations.medianHint') },
  { key: 'p90', label: 'P90', value: fmtMs(props.d.p90_ms), title: t('cards.turnDurations.p90Hint') },
  { key: 'max', label: t('cards.turnDurations.max'), value: fmtMs(props.d.max_ms), title: t('cards.turnDurations.maxHint') },
])
</script>

<template>
  <DashCard
    icon="i-jannchie-hourglass"
    :title="t('cards.turnDurations.title')"
    :title-hint="t('cards.turnDurations.titleHint')"
    :caption="tn('common.metric.turnsN', total)"
  >
    <div class="grid grid-cols-[auto_1fr] gap-x-6 gap-y-4 max-md:grid-cols-1">
      <div class="flex flex-col gap-3 max-md:flex-row max-md:gap-6">
        <div v-for="s in stats" :key="s.key" :title="s.title">
          <div class="text-xs ink-soft">{{ s.label }}</div>
          <div class="text-xl font-semibold leading-tight num">{{ s.value }}</div>
        </div>
      </div>
      <!-- 分布：桶宽不等（越往后越宽），所以只画柱不画连续轴 -->
      <div class="flex h-36 items-end gap-1.5" role="img" :aria-label="fmtList(bars.map((b) => `${b.label} ${b.n}`))">
        <div
          v-for="b in bars"
          :key="b.label"
          class="h-full min-w-0 flex flex-1 flex-col items-center gap-1"
          :title="t('cards.turnDurations.barTitle', { label: b.label, turns: tn('common.metric.turnsN', b.n), pct: fmtPct(b.share) })"
        >
          <!-- 柱区单独占满剩余高度，百分比标签在柱顶上方，不会顶出卡片 -->
          <div class="min-h-0 w-full flex flex-1 flex-col items-center justify-end gap-0.5">
            <span class="text-[10px] ink-soft num">{{ b.n ? fmtPct(b.share, 0) : '' }}</span>
            <div class="w-full rounded-t bg-accent opacity-80" :style="{ height: `${Math.max(b.height * 85, b.n ? 2 : 0)}%` }" />
          </div>
          <span class="w-full truncate text-center text-[10px] ink-soft num">{{ b.label }}</span>
        </div>
      </div>
    </div>

    <div v-if="d.slowest.length" class="mt-4">
      <div class="mb-1 text-xs ink-soft">{{ t('cards.turnDurations.slowest') }}</div>
      <table class="w-full table-fixed border-collapse text-sm">
        <tbody>
          <tr v-for="row in d.slowest" :key="`${row.session_id}:${row.turn}`" class="surface-hover">
            <td class="rounded-l-lg py-1 pl-1 pr-3">
              <RouterLink
                :to="{ name: 'session', params: { id: row.session_id }, hash: `#turn-t:${row.turn}` }"
                class="min-w-0 flex items-center gap-2 kbd-ring"
                :title="`${providerName(row.provider)} · ${fmtDateTime(row.ts)}\n${row.title ?? t('cards.common.untitled')}`"
              >
                <span class="h-2 w-2 shrink-0 rounded-full" :style="{ background: providerDot(row.provider) }" aria-hidden="true"></span>
                <span class="min-w-0 truncate">{{ truncate(row.title ?? t('cards.common.untitled'), 80) }}</span>
              </RouterLink>
            </td>
            <td class="w-28 py-1 text-right text-xs ink-soft num max-sm:hidden">{{ fmtDateTime(row.ts).slice(0, 16) }}</td>
            <td class="w-24 py-1 text-right text-xs num max-sm:hidden" :title="`${tn('common.metric.stepsN', row.steps)} · ${tn('common.metric.toolCallsN', row.tool_calls)}`">
              {{ tn('common.metric.stepsN', row.steps) }}
            </td>
            <td class="w-16 whitespace-nowrap py-1 text-right text-xs num" :class="{ 'ink-soft': row.unpriced }">{{ fmtMoney(row.cost_usd) }}</td>
            <td
              class="w-44 whitespace-nowrap rounded-r-lg py-1 pl-3 pr-1 text-right text-xs num"
              :title="t('cards.turnDurations.wallHint', { wall: fmtMs(row.wall_ms), active: fmtMs(row.active_ms) })"
            >
              <span class="font-semibold">{{ fmtMs(row.wall_ms) }}</span>
              <span class="ink-soft max-sm:hidden"> · {{ t('cards.turnDurations.active', { ms: fmtMs(row.active_ms) }) }}</span>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </DashCard>
</template>
