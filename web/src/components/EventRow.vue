<script setup lang="ts">
import { computed } from 'vue'
import type { Event } from '../api'
import { summarize } from '../lib/group'
import JsonBlock from './JsonBlock.vue'
import TimeStamp from './TimeStamp.vue'

const props = defineProps<{ ev: Event }>()
const summary = computed(() => summarize(props.ev, 200))
const scope = computed(() => {
  const t = props.ev.turn ? `t${props.ev.turn.n ?? '?'}` : ''
  const s = props.ev.step ? `s${props.ev.step.n ?? '?'}` : ''
  return [t, s].filter(Boolean).join('/')
})
const tags = computed(() => {
  const out: string[] = []
  for (const l of props.ev.links ?? []) out.push(`${l.rel}${l.inferred ? '?' : ''}`)
  if (props.ev.inferred?.length) out.push(`inferred:${props.ev.inferred.join(',')}`)
  return out
})
</script>

<template>
  <details class="row" :class="[`k-${ev.kind}`, ev.role ? `r-${ev.role}` : '']">
    <summary class="line">
      <span class="seq mono">{{ ev.seq }}</span>
      <TimeStamp :ts="ev.ts" />
      <span class="kind mono">{{ ev.kind }}</span>
      <span class="role mono">{{ ev.role ?? '' }}</span>
      <span class="scope mono muted" :title="[ev.turn?.id, ev.step?.id].filter(Boolean).join('\n')">{{ scope }}</span>
      <span class="sum ellipsis">
        <span v-for="t in tags" :key="t" class="badge">{{ t }}</span>
        {{ summary }}
      </span>
    </summary>
    <div class="body"><JsonBlock :value="ev" /></div>
  </details>
</template>

<style scoped>
.row {
  border-bottom: 1px solid var(--line-1);
}
.row[open] {
  background: var(--bg-1);
}
.line {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 1px 4px;
  font-size: var(--fs-sm);
  min-width: 0;
}
.line:hover {
  background: var(--bg-1);
}
.seq {
  width: 52px;
  flex: none;
  text-align: right;
  color: var(--fg-2);
  font-variant-numeric: tabular-nums;
}
.kind {
  width: 108px;
  flex: none;
  color: var(--fg-1);
}
.role {
  width: 64px;
  flex: none;
  color: var(--fg-2);
}
.scope {
  width: 56px;
  flex: none;
}
.sum {
  flex: 1;
  min-width: 0;
  color: var(--fg-1);
}
.sum .badge {
  margin-right: 4px;
}
.k-turn_started .kind,
.k-turn_completed .kind {
  color: var(--user);
  font-weight: 600;
}
.k-step_started .kind,
.k-step_completed .kind {
  color: var(--assistant);
}
.k-tool_call .kind,
.k-tool_result .kind {
  color: var(--tool);
}
.k-request_header .kind,
.k-meta .kind {
  color: var(--fg-2);
}
.body {
  padding: 4px 8px 8px 64px;
}
</style>
