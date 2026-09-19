<script setup lang="ts">
import { computed, ref } from 'vue'
import type { Turn } from '../lib/group'
import { fmtDuration, fmtInt } from '../lib/format'
import ItemView from './ItemView.vue'
import StepBlock from './StepBlock.vue'
import TimeStamp from './TimeStamp.vue'
import UsageChip from './UsageChip.vue'

const props = defineProps<{ turn: Turn }>()

const duration = computed(() => fmtDuration(props.turn.started?.ts ?? props.turn.firstTs, props.turn.completed?.ts ?? props.turn.lastTs))
const LIMIT = 2000
const longUser = computed(() => (props.turn.userText?.length ?? 0) > LIMIT)
const userOpen = ref(false)
const collapsed = ref(false)
</script>

<template>
  <section class="turn" :id="`turn-${turn.key}`" :class="{ collapsed }">
    <header class="turn-head" @click="collapsed = !collapsed">
      <span class="tn mono" :title="`turn ${turn.id}${turn.inferred ? '（推断）' : ''}`">
        {{ collapsed ? '▸' : '▾' }} #{{ turn.n ?? '?' }}
      </span>
      <span v-if="turn.inferred" class="badge" title="turn 边界是 normalizer 推断的">inferred</span>
      <TimeStamp :ts="turn.started?.ts ?? turn.firstTs" mode="full" />
      <span v-if="duration" class="mono muted">{{ duration }}</span>
      <span class="mono muted">{{ turn.stepCount }} steps · {{ turn.toolCount }} tools</span>
      <span v-if="turn.model" class="mono muted ellipsis">{{ turn.model }}</span>
      <span class="grow"></span>
      <UsageChip v-if="turn.usage" :usage="turn.usage" />
      <UsageChip v-else-if="turn.usageSum" :usage="turn.usageSum" summed />
      <span v-if="!turn.completed" class="badge" title="没有 turn_completed">open</span>
    </header>

    <div v-show="!collapsed" class="turn-body">
      <div class="user" v-if="turn.userText != null">
        <div class="head">
          <span class="role">user</span>
          <TimeStamp v-if="turn.started" :ts="turn.started.ts" />
        </div>
        <pre v-if="!longUser || userOpen" class="user-text">{{ turn.userText }}</pre>
        <template v-else>
          <pre class="user-text">{{ turn.userText.slice(0, LIMIT) }}…</pre>
          <button type="button" @click="userOpen = true">显示全部（{{ fmtInt(turn.userText.length) }} 字符）</button>
        </template>
      </div>
      <div class="user empty" v-else><span class="muted">（这个 turn 没有 user 文本）</span></div>

      <template v-for="(c, i) in turn.children" :key="c.type === 'step' ? c.step.key : `i:${c.item.ev.seq}`">
        <StepBlock v-if="c.type === 'step'" :step="c.step" />
        <div v-else class="loose-item" :class="{ first: i === 0 }"><ItemView :item="c.item" /></div>
      </template>
    </div>
  </section>
</template>

<style scoped>
.turn {
  border: 1px solid var(--line);
  border-radius: var(--radius);
  margin-bottom: 10px;
  background: var(--bg);
}
.turn-head {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 4px 8px;
  background: var(--bg-1);
  border-bottom: 1px solid var(--line);
  font-size: var(--fs-xs);
  cursor: pointer;
  user-select: none;
  min-width: 0;
  position: sticky;
  top: 32px;
  z-index: 2;
}
.collapsed .turn-head {
  border-bottom: 0;
}
.tn {
  font-weight: 700;
  color: var(--fg);
  font-size: var(--fs-sm);
}
.grow {
  flex: 1;
}
.turn-body {
  padding: 6px 8px 4px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.user {
  border-left: 2px solid var(--user);
  background: var(--user-bg);
  padding: 4px 8px;
  border-radius: var(--radius);
}
.user.empty {
  padding: 2px 8px;
}
.user .head {
  display: flex;
  gap: 10px;
  align-items: center;
  font-size: var(--fs-xs);
  margin-bottom: 2px;
}
.user .role {
  color: var(--user);
  font-weight: 600;
}
.user-text {
  font-family: var(--font-ui);
  font-size: var(--fs);
}
.loose-item {
  margin-left: 4px;
}
</style>
