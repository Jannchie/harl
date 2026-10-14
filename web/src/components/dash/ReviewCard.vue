<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { ReviewAgg } from '../../api'
import { fmtBig, fmtInt, fmtPct, tn } from '../../lib/format'
import DashCard from './DashCard.vue'
import StatGrid, { type Stat } from './StatGrid.vue'

const props = defineProps<{ review: ReviewAgg; toolCalls: number }>()
const { t } = useI18n()

const ratio = (a: number, b: number) => (b > 0 ? a / b : 0)

const stats = computed<Stat[]>(() => [
  {
    key: 'reviews',
    label: t('cards.reviewCard.reviews'),
    value: fmtBig(props.review.reviews),
    sub: t('cards.reviewCard.reviewsSub', { pct: fmtPct(ratio(props.review.reviews, props.toolCalls)) }),
    title: t('cards.reviewCard.reviewsHint', { reviews: fmtInt(props.review.reviews), calls: fmtInt(props.toolCalls) }),
  },
  {
    key: 'blocked',
    label: t('cards.reviewCard.blockRate'),
    value: fmtPct(ratio(props.review.blocked, props.review.reviews), 2),
    sub: tn('cards.reviewCard.blockedSub', props.review.blocked),
    title: t('cards.reviewCard.blockRateHint'),
  },
  {
    key: 'unavailable',
    label: t('cards.reviewCard.unavailable'),
    value: fmtPct(ratio(props.review.unavailable, props.review.reviews), 2),
    sub: tn('cards.common.times', props.review.unavailable),
    title: t('cards.reviewCard.unavailableHint'),
  },
])

const reasons = computed(() => {
  const rows = Object.entries(props.review.by_reason)
    .map(([reason, n]) => ({ reason: reason || t('cards.reviewCard.noReason'), n }))
    .sort((a, b) => b.n - a.n || a.reason.localeCompare(b.reason))
  const max = Math.max(1, ...rows.map((x) => x.n))
  return rows.map((x) => ({ ...x, share: x.n / max }))
})
</script>

<template>
  <DashCard
    icon="i-jannchie-shield"
    :title="t('cards.reviewCard.title')"
    :title-hint="t('cards.reviewCard.titleHint')"
  >
    <StatGrid :stats="stats" />
    <div v-if="reasons.length" class="mt-4">
      <div class="mb-1 text-xs ink-soft">{{ t('cards.reviewCard.reasons') }}</div>
      <table class="w-full border-collapse text-sm">
        <tbody>
          <tr v-for="x in reasons" :key="x.reason" class="surface-hover">
            <td class="w-2/5 rounded-l-lg py-1 pl-1 pr-3">
              <span class="block truncate text-xs">{{ x.reason }}</span>
            </td>
            <td class="rounded-r-lg py-1 pr-1">
              <div class="flex items-center gap-2">
                <div class="meter-track h-1.5 flex-1">
                  <span class="meter-fill bg-red-500/70" :style="{ width: `${x.share * 100}%` }"></span>
                </div>
                <span class="w-10 text-right text-xs num">{{ fmtInt(x.n) }}</span>
              </div>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </DashCard>
</template>
