<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { Usage } from '../api'
import { fmtCompact, fmtInt } from '../lib/format'

const props = defineProps<{
  usage: Usage
  /** 是按 step 求和得到的，不是日志里显式给的 */
  summed?: boolean
}>()
const { t } = useI18n()

const parts = computed(() => {
  const u = props.usage
  const out: { id: string; k: string; label: string; v: number }[] = []
  if (typeof u.input === 'number') out.push({ id: 'input', k: t('common.metric.input'), label: 'i-jannchie-arrow-down', v: u.input })
  if (typeof u.output === 'number') out.push({ id: 'output', k: t('common.metric.output'), label: 'i-jannchie-arrow-up', v: u.output })
  if (typeof u.cache_read === 'number') out.push({ id: 'cache_read', k: t('common.metric.cacheRead'), label: 'i-jannchie-database-arrow-down', v: u.cache_read })
  if (typeof u.cache_write === 'number' && u.cache_write > 0) out.push({ id: 'cache_write', k: t('common.metric.cacheWrite'), label: 'i-jannchie-database-arrow-up', v: u.cache_write })
  return out
})

const title = computed(() => {
  const lines = parts.value.map((p) => t('session.usageChip.line', { k: p.k, v: fmtInt(p.v) }))
  const extra = Object.entries(props.usage).filter(([k]) => !['input', 'output', 'cache_read', 'cache_write'].includes(k))
  for (const [k, v] of extra) lines.push(`${k}: ${typeof v === 'object' ? JSON.stringify(v) : String(v)}`)
  if (props.summed) lines.unshift(t('session.usageChip.summed'))
  return lines.join('\n')
})
</script>

<template>
  <span v-if="parts.length" class="inline-flex items-center gap-2.5 whitespace-nowrap" :title="title">
    <span v-for="p in parts" :key="p.id" class="inline-flex items-center gap-0.5">
      <i class="h-3 w-3 opacity-70" :class="p.label" />
      <span class="num">{{ fmtCompact(p.v) }}</span>
    </span>
  </span>
</template>
