<script setup lang="ts">
import { computed } from 'vue'
import { fmtClock, fmtDateTime, fmtRelative } from '../lib/format'

const props = withDefaults(
  defineProps<{
    ts?: string | null
    /** clock: HH:mm:ss；full: 日期+时间；rel: 相对时间 */
    mode?: 'clock' | 'full' | 'rel'
  }>(),
  { mode: 'clock', ts: null },
)

const text = computed(() => {
  if (!props.ts) return '–'
  if (props.mode === 'full') return fmtDateTime(props.ts)
  if (props.mode === 'rel') return fmtRelative(props.ts)
  return fmtClock(props.ts)
})
const title = computed(() => (props.ts ? `${fmtDateTime(props.ts)} (${fmtRelative(props.ts)})\n${props.ts}` : ''))
</script>

<template>
  <time class="ts mono" :datetime="ts ?? undefined" :title="title">{{ text }}</time>
</template>

<style scoped>
.ts {
  color: var(--fg-2);
  white-space: nowrap;
  font-variant-numeric: tabular-nums;
}
</style>
