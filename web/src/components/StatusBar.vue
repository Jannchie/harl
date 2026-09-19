<script setup lang="ts">
import { computed } from 'vue'
import type { Status } from '../api'
import { fmtBytes, fmtInt } from '../lib/format'

const props = defineProps<{ status: Status | null; error?: string | null }>()

const ratio = computed(() => {
  const s = props.status
  if (!s || !s.logical_bytes) return null
  return s.pack_bytes / s.logical_bytes
})
</script>

<template>
  <div class="status mono">
    <template v-if="status">
      <span class="cell" :title="status.data_dir"><span class="k">data</span> {{ status.data_dir }}</span>
      <span class="cell"><span class="k">machine</span> {{ status.machine }}</span>
      <span class="cell"><span class="k">sources</span> {{ fmtInt(status.sources) }}</span>
      <span class="cell"><span class="k">records</span> {{ fmtInt(status.records) }}</span>
      <span class="cell" :title="`${fmtInt(status.logical_bytes)} B`"
        ><span class="k">logical</span> {{ fmtBytes(status.logical_bytes) }}</span
      >
      <span class="cell" :title="`${fmtInt(status.packs)} packs, ${fmtInt(status.pack_bytes)} B`"
        ><span class="k">packs</span> {{ fmtInt(status.packs) }} / {{ fmtBytes(status.pack_bytes) }}</span
      >
      <span class="cell" title="pack_bytes / logical_bytes"
        ><span class="k">ratio</span> {{ ratio == null ? '–' : `${(ratio * 100).toFixed(1)}%` }}</span
      >
      <span class="cell"><span class="k">chunks</span> {{ fmtInt(status.chunks) }}</span>
      <span class="cell"><span class="k">index</span> {{ fmtBytes(status.index_bytes) }}</span>
    </template>
    <span v-else-if="error" class="danger">status: {{ error }}</span>
    <span v-else class="muted">status…</span>
  </div>
</template>

<style scoped>
.status {
  display: flex;
  flex-wrap: wrap;
  gap: 4px 18px;
  padding: 6px 0;
  font-size: var(--fs-xs);
  color: var(--fg-1);
  border-top: 1px solid var(--line);
}
.cell {
  white-space: nowrap;
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
}
.k {
  color: var(--fg-2);
  margin-right: 2px;
}
</style>
