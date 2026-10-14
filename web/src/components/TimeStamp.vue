<script setup lang="ts">
import { computed, ref } from 'vue'
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
/** 悬停 / 聚焦时才算：事件流里每行都有时间戳，绝大多数不会被悬停 */
const hovered = ref(false)
const title = computed(() => (hovered.value && props.ts ? `${fmtDateTime(props.ts)} (${fmtRelative(props.ts)})\n${props.ts}` : undefined))
</script>

<template>
  <time class="whitespace-nowrap tabular-nums" :datetime="ts ?? undefined" :title="title" @pointerenter="hovered = true" @focusin="hovered = true">{{ text }}</time>
</template>
