<script setup lang="ts">
import { computed } from 'vue'
import { RouterLink } from 'vue-router'
import { useI18n } from 'vue-i18n'
import type { SessionInfo } from '../api'
import { sessionTitle } from '../lib/agents'
import { fmtAgo, fmtDateTime, fmtInt, tn } from '../lib/format'
import { providerDot, providerName } from '../lib/providers'

/** 侧栏里的一个会话。选中是整行；子 agent 的展开按钮是行上面的另一个按钮（按钮不能嵌套） */
const props = defineProps<{
  session: SessionInfo
  selected: boolean
  /** 根会话下面所有子 agent 的个数（父链拉平） */
  childCount?: number
  /** 其中还在写的个数 */
  childRunning?: number
  open?: boolean
  child?: boolean
  /** 子 agent 行：自己还在写 */
  running?: boolean
}>()

const emit = defineEmits<{ toggle: [] }>()
const { t } = useI18n()

const name = computed(() => sessionTitle(props.session))
const tip = computed(() => {
  const s = props.session
  const head = s.label && s.title ? `${s.label}\n${s.title}` : (name.value ?? s.session)
  return `${head}\n${providerName(s.provider)} · ${fmtDateTime(s.mtime)}`
})
const badgeLabel = computed(() => {
  const n = tn('session.sessionRow.agents', props.childCount ?? 0)
  return props.childRunning ? `${n} · ${tn('session.sessionRow.running', props.childRunning)}` : n
})
</script>

<template>
  <div class="group relative">
    <RouterLink
      :to="{ name: 'session', params: { id: session.id } }"
      class="rail-item w-full gap-2 pl-2 pr-2.5 text-[13px]"
      :class="selected ? 'row-selected' : 'rail-item-idle'"
      :title="tip"
    >
      <!-- 和项目行的箭头同一列：顶层会话只是一个 provider 色点，子 agent 会话是机器人（在写的标成绿色） -->
      <span class="h-4 w-4 shrink-0 flex items-center justify-center" aria-hidden="true">
        <i v-if="child" class="i-jannchie-robot h-3.5 w-3.5" :class="running ? 'ink-success' : 'opacity-80'" />
        <span v-else class="h-1.5 w-1.5 rounded-full" :class="providerDot(session.provider)" />
      </span>
      <span class="min-w-0 flex-1 truncate" :class="name ? '' : 'font-mono text-xs'">
        {{ name ?? session.session }}
      </span>
      <!-- 有子 agent 的行把这格让给展开按钮：按钮浮在上面，这里放一份看不见的同样内容占住宽度 -->
      <span v-if="childCount" class="invisible shrink-0 flex items-center gap-1 pr-1.5 text-xs" aria-hidden="true">
        <span class="h-3.5 w-3.5" /><span class="num">{{ fmtInt(childCount) }}</span>
        <template v-if="childRunning"><span class="h-1.5 w-1.5" /><span class="num">{{ fmtInt(childRunning) }}</span></template>
        <span class="h-3 w-3" />
      </span>
      <span v-else class="shrink-0 text-xs font-normal ink-soft num">{{ fmtAgo(session.mtime) }}</span>
    </RouterLink>

    <button
      v-if="childCount"
      type="button"
      class="absolute inset-y-1 right-1.5 flex items-center gap-1 rounded px-1 text-xs ink-soft kbd-ring transition-colors duration-150 hover:text-ink"
      :title="open ? t('session.sessionRow.collapseChildren') : badgeLabel"
      :aria-label="open ? t('session.sessionRow.collapseChildren') : t('session.sessionRow.expandChildren', { what: badgeLabel })"
      :aria-expanded="open"
      @click="emit('toggle')"
    >
      <i class="i-jannchie-robot h-3.5 w-3.5" aria-hidden="true" />
      <span class="num">{{ fmtInt(childCount) }}</span>
      <template v-if="childRunning">
        <span class="h-1.5 w-1.5 animate-pulse rounded-full bg-current ink-success" aria-hidden="true" />
        <span class="num ink-success">{{ fmtInt(childRunning) }}</span>
      </template>
      <i class="h-3 w-3 transition-transform duration-150 i-jannchie-chevron-right" :class="open ? 'rotate-90' : ''" aria-hidden="true" />
    </button>
  </div>
</template>
