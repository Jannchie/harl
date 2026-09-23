<script setup lang="ts">
import { computed, ref } from 'vue'
import { prettyJson } from '../lib/format'

const props = withDefaults(
  defineProps<{
    /** 已经是字符串就原样显示，否则 pretty JSON */
    value: unknown
    /** 超过这个字符数先截断 */
    limit?: number
  }>(),
  { limit: 20_000 },
)

const full = computed(() => (typeof props.value === 'string' ? props.value : prettyJson(props.value)))
const expanded = ref(false)
const truncated = computed(() => full.value.length > props.limit && !expanded.value)
const shown = computed(() => (truncated.value ? full.value.slice(0, props.limit) : full.value))
</script>

<template>
  <div>
    <pre class="payload-block max-h-[60vh] leading-relaxed">{{ shown }}</pre>
    <button v-if="truncated" type="button" class="mt-1 rounded-md px-2 py-0.5 text-xs ink-soft surface-hover kbd-ring" @click="expanded = true">
      显示全部（{{ full.length.toLocaleString() }} 字符）
    </button>
  </div>
</template>
