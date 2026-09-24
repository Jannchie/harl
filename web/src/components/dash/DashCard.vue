<script setup lang="ts">
defineProps<{
  /** carbon 图标类，如 i-carbon-calendar */
  icon: string
  title: string
  /** 口径说明，一行 */
  caption?: string
  /** 更长的口径说明，悬停在标题上看 */
  titleHint?: string
}>()
</script>

<template>
  <!-- 一块 surface，不描边；区分靠标题字重和留白 -->
  <section class="card min-w-0 flex flex-col gap-3">
    <header class="flex flex-wrap items-start gap-x-3 gap-y-2">
      <div class="min-w-0 flex-1">
        <h2 class="card-title m-0" :title="titleHint">
          <i class="h-4 w-4 shrink-0 ink-soft" :class="icon" />
          {{ title }}
          <i v-if="titleHint" class="i-carbon-information h-3.5 w-3.5 ink-soft opacity-70" />
        </h2>
        <p v-if="caption" class="m-0 mt-0.5 pl-6 text-xs ink-soft">{{ caption }}</p>
      </div>
      <div v-if="$slots.controls" class="segmented shrink-0"><slot name="controls" /></div>
    </header>
    <div class="min-w-0 flex-1"><slot /></div>
    <footer v-if="$slots.footer" class="text-xs ink-soft"><slot name="footer" /></footer>
  </section>
</template>
