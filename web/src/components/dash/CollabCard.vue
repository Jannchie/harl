<script setup lang="ts">
import { computed } from 'vue'
import type { Signals } from '../../api'
import { fmtInt, fmtPct } from '../../lib/format'
import DashCard from './DashCard.vue'
import StatGrid, { type Stat } from './StatGrid.vue'

const props = defineProps<{ signals: Signals; turns: number }>()

/** 每 100 个 turn 几次 */
const per100 = (n: number) => (props.turns > 0 ? ((n / props.turns) * 100).toFixed(1) : '–')

const stats = computed<Stat[]>(() => [
  {
    key: 'steers',
    label: '中途插话',
    value: fmtInt(props.signals.steers),
    sub: `每 100 turn ${per100(props.signals.steers)} 次`,
    title: 'agent 还在干活时用户补充的消息（不开新 turn）',
  },
  {
    key: 'interrupts',
    label: '打断',
    value: fmtPct(props.turns > 0 ? props.signals.interrupts / props.turns : 0),
    sub: `${fmtInt(props.signals.interrupts)} 次`,
    title: '被用户打断的次数 / turn 数',
  },
  {
    key: 'compactions',
    label: '上下文压缩',
    value: fmtInt(props.signals.compactions),
    sub: `每 100 turn ${per100(props.signals.compactions)} 次`,
    title: '上下文快满时 harness 压缩历史的次数（自动和手动 /compact 都算）',
  },
])
</script>

<template>
  <DashCard icon="i-jannchie-collaborate" title="协作" title-hint="人和 agent 之间的互动：插话、打断，以及上下文压缩。只统计日志里有显式记录的 harness（Claude Code、Codex）。">
    <StatGrid :stats="stats" />
  </DashCard>
</template>
