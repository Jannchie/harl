<script setup lang="ts">
defineProps<{
  /** 两位编号，像仪表面板的分区号 */
  n: string
  title: string
  /** 口径说明，一行 */
  caption?: string
}>()
</script>

<template>
  <section class="card">
    <header class="head">
      <span class="n mono">{{ n }}</span>
      <h2 class="title">{{ title }}</h2>
      <span v-if="caption" class="caption muted">{{ caption }}</span>
      <div class="spacer"></div>
      <div class="controls"><slot name="controls" /></div>
    </header>
    <div class="body"><slot /></div>
    <footer v-if="$slots.footer" class="foot muted"><slot name="footer" /></footer>
  </section>
</template>

<style scoped>
.card {
  border: 1px solid var(--line);
  background: var(--bg);
  min-width: 0;
  display: flex;
  flex-direction: column;
}
.head {
  display: flex;
  align-items: baseline;
  gap: 8px;
  padding: 7px 12px 6px;
  border-bottom: 1px solid var(--line-1);
  flex-wrap: wrap;
}
.n {
  font-size: var(--fs-xs);
  color: var(--fg-2);
  letter-spacing: 0.08em;
}
.title {
  font-size: var(--fs);
  font-weight: 600;
  margin: 0;
  letter-spacing: 0.01em;
}
.caption {
  font-size: var(--fs-xs);
}
.spacer {
  flex: 1;
}
.controls {
  display: flex;
  gap: 0;
  align-items: center;
}
.controls :deep(button) {
  font-size: var(--fs-xs);
  padding: 1px 8px;
  border-radius: 0;
  margin-left: -1px;
}
.controls :deep(button:first-child) {
  border-radius: var(--radius) 0 0 var(--radius);
  margin-left: 0;
}
.controls :deep(button:last-child) {
  border-radius: 0 var(--radius) var(--radius) 0;
}
.controls :deep(button.active) {
  position: relative;
  z-index: 1;
}
.body {
  padding: 8px 8px 4px;
  min-width: 0;
  flex: 1;
}
.foot {
  font-size: var(--fs-xs);
  padding: 4px 12px 6px;
  border-top: 1px solid var(--line-1);
}
</style>
