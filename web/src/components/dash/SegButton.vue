<script setup lang="ts">
// DashCard 右上角分段选择里的一段
defineProps<{ active: boolean }>()
</script>

<template>
  <button type="button" class="segment border-0" :class="active ? 'segment-selected' : 'bg-transparent ink-muted hover:text-neutral-900 dark:hover:text-neutral-100'">
    <slot />
  </button>
</template>
