<script setup lang="ts">
import { RouterLink } from 'vue-router'
import { useI18n } from 'vue-i18n'
import type { SessionInfo } from '../api'
import { fmtAgo, fmtDateTime, tn } from '../lib/format'
import { providerDot, providerName } from '../lib/providers'

/** 侧栏里的一个会话。选中是整行；子会话的展开按钮是行上面的另一个按钮（按钮不能嵌套） */
defineProps<{
  session: SessionInfo
  selected: boolean
  /** 有子 agent 会话时的个数 */
  childCount?: number
  open?: boolean
  child?: boolean
}>()

const emit = defineEmits<{ toggle: [] }>()
const { t } = useI18n()
</script>

<template>
  <div class="group relative">
    <RouterLink
      :to="{ name: 'session', params: { id: session.id } }"
      class="rail-item w-full gap-2 pl-2 pr-2.5 text-[13px]"
      :class="selected ? 'row-selected' : 'rail-item-idle'"
      :title="`${session.title ?? session.session}
${providerName(session.provider)} · ${fmtDateTime(session.mtime)}`"
    >
      <!-- 和项目行的箭头同一列：顶层会话只是一个 provider 色点，子 agent 会话是机器人 -->
      <span class="h-4 w-4 shrink-0 flex items-center justify-center" aria-hidden="true">
        <i v-if="child" class="i-jannchie-robot h-3.5 w-3.5 opacity-80" />
        <span v-else class="h-1.5 w-1.5 rounded-full" :class="providerDot(session.provider)" />
      </span>
      <span class="min-w-0 flex-1 truncate" :class="session.title ? '' : 'font-mono text-xs'">
        {{ session.title ?? session.session }}
      </span>
      <!-- 有子会话的行把这格让给展开按钮（按钮浮在上面，这里留同样的宽度） -->
      <span v-if="childCount" class="w-10 shrink-0" />
      <span v-else class="shrink-0 text-xs font-normal ink-soft num">{{ fmtAgo(session.mtime) }}</span>
    </RouterLink>

    <button
      v-if="childCount"
      type="button"
      class="absolute inset-y-1 right-1.5 flex items-center gap-0.5 rounded px-1 text-xs ink-soft kbd-ring transition-colors duration-150 hover:text-ink"
      :title="open ? t('session.sessionRow.collapseChildren') : tn('session.sessionRow.expandChildren', childCount)"
      :aria-label="open ? t('session.sessionRow.collapseChildren') : tn('session.sessionRow.expandChildren', childCount)"
      :aria-expanded="open"
      @click="emit('toggle')"
    >
      <i class="i-jannchie-robot h-3.5 w-3.5" aria-hidden="true" />
      <span class="num">{{ childCount }}</span>
      <i class="h-3 w-3 transition-transform duration-150 i-jannchie-chevron-right" :class="open ? 'rotate-90' : ''" aria-hidden="true" />
    </button>
  </div>
</template>
