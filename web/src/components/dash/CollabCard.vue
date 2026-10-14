<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { Signals } from '../../api'
import { fmtInt, fmtPct, tn } from '../../lib/format'
import DashCard from './DashCard.vue'
import StatGrid, { type Stat } from './StatGrid.vue'

const props = defineProps<{ signals: Signals; turns: number }>()
const { t } = useI18n()

/** 每 100 个 turn 几次 */
const per100 = (n: number) => (props.turns > 0 ? ((n / props.turns) * 100).toFixed(1) : '–')

const stats = computed<Stat[]>(() => [
  {
    key: 'steers',
    label: t('cards.collabCard.steers'),
    value: fmtInt(props.signals.steers),
    sub: t('cards.collabCard.per100', { n: per100(props.signals.steers) }),
    title: t('cards.collabCard.steersHint'),
  },
  {
    key: 'interrupts',
    label: t('cards.collabCard.interrupts'),
    value: fmtPct(props.turns > 0 ? props.signals.interrupts / props.turns : 0),
    sub: tn('cards.common.times', props.signals.interrupts),
    title: t('cards.collabCard.interruptsHint'),
  },
  {
    key: 'compactions',
    label: t('cards.collabCard.compactions'),
    value: fmtInt(props.signals.compactions),
    sub: t('cards.collabCard.per100', { n: per100(props.signals.compactions) }),
    title: t('cards.collabCard.compactionsHint'),
  },
])
</script>

<template>
  <DashCard icon="i-jannchie-collaborate" :title="t('cards.collabCard.title')" :title-hint="t('cards.collabCard.titleHint')">
    <StatGrid :stats="stats" />
  </DashCard>
</template>
