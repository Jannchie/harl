<script setup lang="ts">
import { computed } from 'vue'
import { RouterLink, RouterView, useRoute } from 'vue-router'

const route = useRoute()
// 会话详情页也算在 Sessions 下
const onSessions = computed(() => route.name === 'sessions' || route.name === 'session')
</script>

<template>
  <header class="top">
    <RouterLink to="/" class="brand">harl</RouterLink>
    <span class="muted sub">coding-agent session archive</span>
    <nav class="nav">
      <RouterLink :to="{ name: 'dashboard' }" class="navlink" active-class="on" exact-active-class="on">Dashboard</RouterLink>
      <RouterLink :to="{ name: 'sessions' }" class="navlink" :class="{ on: onSessions }">Sessions</RouterLink>
    </nav>
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
.nav {
  display: flex;
  gap: 2px;
  margin-left: 14px;
  align-self: stretch;
}
.navlink {
  color: var(--fg-1);
  padding: 0 8px;
  border-bottom: 2px solid transparent;
  line-height: 30px;
  font-size: var(--fs-sm);
}
.navlink:hover {
  text-decoration: none;
  color: var(--fg);
}
.navlink.on {
  color: var(--fg);
  border-bottom-color: var(--accent);
}
</style>
