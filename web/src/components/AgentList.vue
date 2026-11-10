<script setup lang="ts">
// 侧栏根会话下面的一串子 agent（普通子 agent、一次 workflow 运行里的 agent）：先给的那几行 + 「再显示 N 个」
import { useI18n } from 'vue-i18n'
import type { SessionInfo } from '../api'
import SessionRow from './SessionRow.vue'

defineProps<{
  /** 要显示的行 */
  items: SessionInfo[]
  /** 收起来没显示的个数 */
  more: number
  activeId: string | null
  /** 还在写的会话 id */
  running: ReadonlySet<string>
}>()
const emit = defineEmits<{ more: [] }>()
const { t } = useI18n()
</script>

<template>
  <SessionRow v-for="c in items" :key="c.id" :session="c" :selected="c.id === activeId" :running="running.has(c.id)" child />
  <button v-if="more > 0" type="button" class="rail-more" @click="emit('more')">
    {{ t('common.sidebar.showMore', { n: more }) }}
  </button>
</template>
