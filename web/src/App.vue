<script setup lang="ts">
import { RouterLink, RouterView } from 'vue-router'
</script>

<template>
  <header class="top">
    <RouterLink to="/" class="brand">harl</RouterLink>
    <span class="muted sub">coding-agent session archive</span>
  </header>
  <main>
    <RouterView />
  </main>
</template>

<style scoped>
.top {
  display: flex;
  align-items: baseline;
  gap: 10px;
  height: 32px;
  padding: 0 16px;
  border-bottom: 1px solid var(--line);
  background: var(--bg-1);
  line-height: 32px;
  position: sticky;
  top: 0;
  z-index: 10;
}
.brand {
  font-family: var(--font-mono);
  font-weight: 700;
  color: var(--fg);
  letter-spacing: 0.02em;
}
.brand:hover {
  text-decoration: none;
  color: var(--accent);
}
.sub {
  font-size: var(--fs-xs);
}
</style>
