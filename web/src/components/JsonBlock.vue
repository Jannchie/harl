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
  <div class="json">
    <pre class="body">{{ shown }}</pre>
    <div v-if="truncated" class="more">
      <button type="button" @click="expanded = true">显示全部（{{ full.length.toLocaleString() }} 字符）</button>
    </div>
  </div>
</template>

<style scoped>
.json {
  background: var(--bg-code);
  border: 1px solid var(--line-1);
  border-radius: var(--radius);
}
.body {
  padding: 6px 8px;
  max-height: 60vh;
  overflow: auto;
  font-size: var(--fs-xs);
  line-height: 1.4;
  tab-size: 2;
}
.more {
  border-top: 1px solid var(--line-1);
  padding: 4px 8px;
}
</style>
