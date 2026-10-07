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
  const out: { k: string; label: string; v: number }[] = []
  if (typeof u.input === 'number') out.push({ k: '输入', label: 'i-jannchie-arrow-down', v: u.input })
  if (typeof u.output === 'number') out.push({ k: '输出', label: 'i-jannchie-arrow-up', v: u.output })
  if (typeof u.cache_read === 'number') out.push({ k: '缓存读', label: 'i-jannchie-database-arrow-down', v: u.cache_read })
  if (typeof u.cache_write === 'number' && u.cache_write > 0) out.push({ k: '缓存写', label: 'i-jannchie-database-arrow-up', v: u.cache_write })
  return out
})

const title = computed(() => {
  const lines = parts.value.map((p) => `${p.k}：${fmtInt(p.v)} tokens`)
  const extra = Object.entries(props.usage).filter(([k]) => !['input', 'output', 'cache_read', 'cache_write'].includes(k))
  for (const [k, v] of extra) lines.push(`${k}: ${typeof v === 'object' ? JSON.stringify(v) : String(v)}`)
  if (props.summed) lines.unshift('按 step 求和（turn 上没有显式 usage）')
  return lines.join('\n')
})
</script>

<template>
  <span v-if="parts.length" class="inline-flex items-center gap-2.5 whitespace-nowrap" :title="title">
    <span v-for="p in parts" :key="p.k" class="inline-flex items-center gap-0.5">
      <i class="h-3 w-3 opacity-70" :class="p.label" />
      <span class="num">{{ fmtCompact(p.v) }}</span>
    </span>
  </span>
</template>
