<script setup lang="ts">
import { computed } from 'vue'
import type { Step } from '../lib/group'
import { fmtDuration } from '../lib/format'
import ItemView from './ItemView.vue'
import TimeStamp from './TimeStamp.vue'
import UsageChip from './UsageChip.vue'

const props = defineProps<{ step: Step }>()
const duration = computed(() => fmtDuration(props.step.firstTs, props.step.lastTs))
</script>

<template>
  <section class="step" :id="`step-${step.key}`">
    <header class="step-head">
      <span class="sn mono" :title="`step ${step.id}`">step {{ step.n ?? '?' }}</span>
      <TimeStamp :ts="step.firstTs" />
      <span v-if="duration" class="mono muted">{{ duration }}</span>
      <span v-if="step.model" class="mono muted ellipsis">{{ step.model }}</span>
      <UsageChip v-if="step.usage" :usage="step.usage" />
      <span v-if="step.items.length === 0" class="muted">（没有可显示的事件）</span>
    </header>
    <div class="items">
      <ItemView v-for="it in step.items" :key="it.ev.seq" :item="it" />
    </div>
  </section>
</template>

<style scoped>
.step {
  border-left: 1px solid var(--line);
  margin-left: 4px;
  padding-left: 10px;
}
.step-head {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: var(--fs-xs);
  color: var(--fg-2);
  padding: 2px 0 4px;
  min-width: 0;
}
.sn {
  color: var(--fg-1);
}
.items {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding-bottom: 6px;
}
</style>
