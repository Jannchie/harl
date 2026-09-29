<script setup lang="ts">
export interface Stat {
  key: string
  label: string
  value: string
  sub?: string
  /** 悬停说明口径 */
  title?: string
}

defineProps<{ stats: Stat[] }>()
</script>

<template>
  <!-- 卡片里的一排大数字，窄屏竖排 -->
  <div class="grid grid-cols-[repeat(var(--cols),minmax(0,1fr))] gap-3 max-sm:grid-cols-1" :style="{ '--cols': stats.length }">
    <div v-for="s in stats" :key="s.key" class="flex flex-col gap-0.5" :title="s.title">
      <div class="text-xs ink-soft">{{ s.label }}</div>
      <div class="text-xl font-semibold leading-tight num">{{ s.value }}</div>
      <div v-if="s.sub" class="text-xs ink-muted num">{{ s.sub }}</div>
    </div>
  </div>
</template>
