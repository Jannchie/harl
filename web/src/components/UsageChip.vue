<script setup lang="ts">
import { computed } from 'vue'
import type { Usage } from '../api'
import { fmtCompact, fmtInt } from '../lib/format'

const props = defineProps<{
  usage: Usage
  /** 是按 step 求和得到的，不是日志里显式给的 */
  summed?: boolean
}>()

const parts = computed(() => {
  const u = props.usage
  const out: { k: string; v: number }[] = []
  if (typeof u.input === 'number') out.push({ k: 'in', v: u.input })
  if (typeof u.output === 'number') out.push({ k: 'out', v: u.output })
  if (typeof u.cache_read === 'number') out.push({ k: 'cache↓', v: u.cache_read })
  if (typeof u.cache_write === 'number') out.push({ k: 'cache↑', v: u.cache_write })
  return out
})

const title = computed(() => {
  const lines = parts.value.map((p) => `${p.k}: ${fmtInt(p.v)} tokens`)
  const extra = Object.entries(props.usage).filter(([k]) => !['input', 'output', 'cache_read', 'cache_write'].includes(k))
  for (const [k, v] of extra) lines.push(`${k}: ${typeof v === 'object' ? JSON.stringify(v) : String(v)}`)
  if (props.summed) lines.unshift('按 step 求和（turn_completed 上没有 usage）')
  return lines.join('\n')
})
</script>

<template>
  <span v-if="parts.length" class="usage mono" :title="title">
    <span v-if="summed" class="sigma">Σ</span>
    <span v-for="p in parts" :key="p.k" class="part"><span class="k">{{ p.k }}</span> {{ fmtCompact(p.v) }}</span>
  </span>
</template>

<style scoped>
.usage {
  display: inline-flex;
  gap: 8px;
  color: var(--fg-2);
  font-size: var(--fs-xs);
  white-space: nowrap;
}
.k {
  color: var(--fg-2);
  opacity: 0.8;
}
.part {
  color: var(--fg-1);
}
.sigma {
  color: var(--fg-2);
}
</style>
