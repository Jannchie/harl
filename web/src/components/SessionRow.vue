<script setup lang="ts">
import { RouterLink } from 'vue-router'
import type { SessionInfo } from '../api'
import { fmtAgo, fmtDateTime } from '../lib/format'
import { providerDot } from '../lib/providers'

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
</script>

<template>
  <div class="group relative">
    <RouterLink
      :to="{ name: 'session', params: { id: session.id } }"
      class="w-full flex items-center gap-2 rounded-full py-1.5 pl-2 pr-2.5 text-left text-sm kbd-ring"
      :class="selected ? 'pill-selected' : 'surface-hover'"
      :title="`${session.title ?? session.session}\n${session.provider} · ${fmtDateTime(session.mtime)}`"
    >
      <!-- provider 的颜色只落在图标角上这一个点 -->
      <span class="relative h-4 w-4 shrink-0">
        <i class="h-4 w-4 opacity-70" :class="child ? 'i-carbon-bot' : 'i-carbon-chat'" />
        <span class="absolute h-1.5 w-1.5 rounded-full -right-0.5 -top-0.5" :class="providerDot(session.provider)" />
      </span>
      <span class="min-w-0 flex-1 truncate" :class="session.title ? '' : 'ink-soft'">
        {{ session.title ?? session.session }}
      </span>
      <!-- 有子会话的行把这格让给展开按钮（按钮浮在上面，这里留同样的宽度） -->
      <span v-if="childCount" class="w-10 shrink-0" />
      <span v-else class="shrink-0 text-xs text-neutral-500 tabular-nums">{{ fmtAgo(session.mtime) }}</span>
    </RouterLink>

    <button
      v-if="childCount"
      type="button"
      class="absolute inset-y-0 right-2 flex items-center gap-0.5 text-xs text-neutral-500 kbd-ring transition-quick hover:ink-accent"
      :title="open ? '收起子会话' : `展开 ${childCount} 个子会话`"
      :aria-expanded="open"
      @click="emit('toggle')"
    >
      <i class="i-carbon-bot h-3.5 w-3.5" />
      <span class="num">{{ childCount }}</span>
      <i class="h-3 w-3" :class="open ? 'i-carbon-chevron-down' : 'i-carbon-chevron-right'" />
    </button>
  </div>
</template>
