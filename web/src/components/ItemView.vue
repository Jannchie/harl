<script setup lang="ts">
import { computed, ref } from 'vue'
import { RouterLink } from 'vue-router'
import { useI18n } from 'vue-i18n'
import type { Link } from '../api'
import type { Item } from '../lib/group'
import { inputSummary, outputText } from '../lib/group'
import { fmtDuration, tn } from '../lib/format'
import { TOOL_ICON } from '../lib/providers'
import { bySession } from '../lib/sessions'
import ActivityRow from './ActivityRow.vue'
import JsonBlock from './JsonBlock.vue'
import MarkdownText from './MarkdownText.vue'
import UserBubble from './UserBubble.vue'

const props = defineProps<{ item: Item }>()
const { t } = useI18n()
const ev = computed(() => props.item.ev)
const result = computed(() => props.item.result)
const open = ref(false)
const thinkingOpen = ref(false)

function routeFor(session: string | undefined) {
  const s = session ? bySession.value.get(session) : undefined
  return s ? { name: 'session', params: { id: s.id } } : undefined
}

const links = computed<Link[]>(() => [...(ev.value.links ?? []), ...(result.value?.links ?? [])])
const spawned = computed(() => links.value.find((l) => l.rel === 'spawned'))
const notifies = computed(() => links.value.find((l) => l.rel === 'notifies'))
const delivered = computed(() => links.value.find((l) => l.rel === 'delivered_from'))

const isTool = computed(() => ev.value.kind === 'tool_call' && !!ev.value.tool)
const isOrphanResult = computed(() => ev.value.kind === 'tool_result')
const toolName = computed(() => ev.value.tool?.name || result.value?.tool?.name || t('common.metric.tool'))
const status = computed(() => result.value?.tool?.status ?? (isOrphanResult.value ? ev.value.tool?.status : undefined))
const failed = computed(() => status.value === 'failed' || status.value === 'error')
const category = computed(() => ev.value.tool?.category ?? result.value?.tool?.category ?? 'other')
const summary = computed(() => (isTool.value ? inputSummary(ev.value.tool?.input).replace(/\s+/g, ' ') : ''))
const duration = computed(() => fmtDuration(ev.value.ts, result.value?.ts) ?? undefined)

const toolMeta = computed(() => {
  if (failed.value) return t('session.itemView.failed')
  if (isTool.value && !result.value) return t('session.itemView.noResult')
  return undefined
})

const output = computed(() => {
  const src = isOrphanResult.value ? ev.value : result.value
  const o = src?.tool?.output
  if (o == null) return null
  return typeof o === 'string' || Array.isArray(o) ? outputText(o) : o
})

/** 助手正文：message role=assistant，或 tool_call 行上顺带的 text / reasoning */
const isAssistant = computed(() => ev.value.role === 'assistant' && (!!ev.value.text || !!ev.value.reasoning))
const isUser = computed(() => ev.value.kind === 'message' && ev.value.role === 'user')
const isNotification = computed(() => ev.value.kind === 'message' && !ev.value.role && !!notifies.value)

function firstLine(s: string | undefined): string {
  for (const l of (s ?? '').split('\n')) if (l.trim()) return l.trim()
  return ''
}
</script>

<template>
  <!-- 思考：和工具行同一个形状，都是 agent 在给出答案前做的一件事 -->
  <div v-if="isAssistant && ev.reasoning">
    <div :class="thinkingOpen ? 'row-pin' : undefined">
      <ActivityRow
        icon="i-jannchie-lightbulb"
        :label="t('session.itemView.thinking')"
        :detail="thinkingOpen ? '' : firstLine(ev.reasoning)"
        :meta="tn('session.itemView.chars', ev.reasoning.length)"
        interactive
        :selected="thinkingOpen"
        @activate="thinkingOpen = !thinkingOpen"
      />
    </div>
    <MarkdownText v-if="thinkingOpen" :text="ev.reasoning" class="reasoning-prose mb-2 mt-1 pl-7 ink-soft" />
  </div>
  <MarkdownText v-if="isAssistant && ev.text" :text="ev.text" class="py-1" />

  <!-- 工具调用和配对的结果：一行，点开看输入输出 -->
  <div v-if="isTool" :id="`call-${ev.tool?.call_id}`">
    <div :class="open ? 'row-pin' : undefined">
      <ActivityRow
        :icon="failed ? 'i-jannchie-alert-triangle' : TOOL_ICON[category]"
        :tone="failed ? 'failed' : toolMeta ? 'muted' : 'neutral'"
        :label="toolName"
        :detail="summary"
        :time="duration"
        :meta="toolMeta"
        :to="routeFor(spawned?.session)"
        interactive
        :selected="open"
        @activate="open = !open"
      />
    </div>
    <div v-if="open" class="flex flex-col gap-3 pb-3 pl-7 pr-1 pt-1">
      <section class="flex flex-col gap-1">
        <div class="flex items-center gap-2 text-xs ink-soft">
          {{ t('common.metric.input') }} <span class="num ink-soft">{{ ev.tool?.call_id }}</span>
        </div>
        <JsonBlock :value="ev.tool?.input" />
      </section>
      <section v-if="result" class="flex flex-col gap-1">
        <div class="flex items-center gap-2 text-xs" :class="failed ? 'ink-danger' : 'ink-soft'">
          {{ t('common.metric.output') }} <span v-if="status">{{ status }}</span>
        </div>
        <JsonBlock v-if="output != null && output !== ''" :value="output" />
        <p v-else class="m-0 text-xs ink-soft">{{ t('session.itemView.empty') }}</p>
        <MarkdownText v-if="result.text" :text="result.text" />
      </section>
      <p v-else class="m-0 text-xs ink-soft">{{ t('session.itemView.noPairedResult') }}</p>
      <p v-if="spawned && !routeFor(spawned.session)" class="m-0 text-xs ink-soft">{{ t('session.itemView.childNotArchived', { id: spawned.session }) }}</p>
    </div>
  </div>

  <!-- 没有调用的孤儿结果 -->
  <div v-else-if="isOrphanResult" :id="`call-${ev.tool?.call_id}`">
    <ActivityRow
      :icon="failed ? 'i-jannchie-alert-triangle' : 'i-jannchie-unlink'"
      :tone="failed ? 'failed' : 'muted'"
      :label="toolName"
      :detail="t('session.itemView.noMatchingCall')"
      :meta="status"
      interactive
      :selected="open"
      @activate="open = !open"
    />
    <div v-if="open && output != null" class="pb-3 pl-7 pt-1"><JsonBlock :value="output" /></div>
  </div>

  <!-- turn 中途的 user 消息：插话、排队的输入、父会话送来的 -->
  <UserBubble v-else-if="isUser" :text="ev.text ?? ''">
    <template v-if="delivered">
      <i class="i-jannchie-robot h-3.5 w-3.5" />
      {{ t('session.itemView.from') }}
      <RouterLink v-if="routeFor(delivered.session)" :to="routeFor(delivered.session)!" class="ink-accent hover:underline">
        {{ delivered.session }}
      </RouterLink>
      <span v-else>{{ delivered.session ?? '?' }}</span>
      <span v-if="delivered.inferred">{{ t('session.itemView.inferredParen') }}</span>
    </template>
    <span v-else-if="ev.ext.promptSource === 'queued'">{{ t('session.itemView.queued') }}</span>
  </UserBubble>

  <!-- 后台任务通知 -->
  <div v-else-if="isNotification">
    <ActivityRow
      icon="i-jannchie-bell"
      :label="t('session.itemView.notification')"
      :detail="firstLine(ev.text)"
      :meta="notifies?.task"
      interactive
      :selected="open"
      @activate="open = !open"
    />
    <div v-if="open" class="flex flex-col gap-1 pb-3 pl-7 pt-1">
      <a v-if="notifies?.call_id" :href="`#call-${notifies.call_id}`" class="text-xs ink-accent hover:underline">{{ t('session.itemView.jumpToCall') }}</a>
      <JsonBlock :value="ev.text ?? ''" />
    </div>
  </div>

  <!-- 兜底：其他带内容的事件。什么都没有的（Claude Code 只带思考签名的 assistant 行）不画 -->
  <div v-else-if="!isAssistant && (ev.text || ev.reasoning)">
    <ActivityRow
      icon="i-jannchie-info"
      :label="ev.kind"
      :detail="firstLine(ev.text ?? ev.reasoning)"
      :meta="ev.role"
      interactive
      :selected="open"
      @activate="open = !open"
    />
    <div v-if="open" class="pb-3 pl-7 pt-1"><JsonBlock :value="ev.text ?? ev" /></div>
  </div>
</template>
