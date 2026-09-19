<script setup lang="ts">
import { computed, inject, ref } from 'vue'
import { RouterLink } from 'vue-router'
import type { Event, Link } from '../api'
import type { Item } from '../lib/group'
import { inputSummary, outputText } from '../lib/group'
import { fmtDuration, fmtInt, truncate } from '../lib/format'
import { SESSIONS_KEY } from '../lib/ctx'
import JsonBlock from './JsonBlock.vue'
import MarkdownText from './MarkdownText.vue'
import TimeStamp from './TimeStamp.vue'

const props = defineProps<{ item: Item }>()
const ev = computed(() => props.item.ev)
const result = computed(() => props.item.result)

const sessions = inject(SESSIONS_KEY, ref(new Map()))

function routeFor(session: string | undefined) {
  if (!session) return null
  const s = sessions.value.get(session)
  return s ? { name: 'session', params: { id: s.id } } : null
}

const links = computed<Link[]>(() => [...(ev.value.links ?? []), ...(result.value?.links ?? [])])
const spawned = computed(() => links.value.find((l) => l.rel === 'spawned'))
const notifies = computed(() => links.value.find((l) => l.rel === 'notifies'))
const delivered = computed(() => links.value.find((l) => l.rel === 'delivered_from'))

const isTool = computed(() => ev.value.kind === 'tool_call' && !!ev.value.tool)
const isOrphanResult = computed(() => ev.value.kind === 'tool_result')
const toolName = computed(() => ev.value.tool?.name || result.value?.tool?.name || '(tool)')
const status = computed(() => result.value?.tool?.status ?? (isOrphanResult.value ? ev.value.tool?.status : undefined))
const failed = computed(() => status.value === 'failed' || status.value === 'error')
const summary = computed(() => (isTool.value ? truncate(inputSummary(ev.value.tool?.input).replace(/\s+/g, ' '), 200) : ''))
const duration = computed(() => fmtDuration(ev.value.ts, result.value?.ts))

const outText = computed(() => {
  const src = isOrphanResult.value ? ev.value : result.value
  return src?.tool ? outputText(src.tool.output) : ''
})
const outIsText = computed(() => {
  const src = isOrphanResult.value ? ev.value : result.value
  const o = src?.tool?.output
  return typeof o === 'string' || Array.isArray(o)
})

/** 助手正文：message role=assistant，或 tool_call 行上顺带的 text / reasoning */
const showAssistant = computed(() => ev.value.role === 'assistant' && (!!ev.value.text || !!ev.value.reasoning))
const isUser = computed(() => ev.value.kind === 'message' && ev.value.role === 'user')
const isNotification = computed(() => ev.value.kind === 'message' && !ev.value.role && !!notifies.value)
const isOtherText = computed(
  () => !isTool.value && !isOrphanResult.value && !showAssistant.value && !isUser.value && !isNotification.value,
)

const longUser = computed(() => (ev.value.text?.length ?? 0) > 1500)
const userOpen = ref(false)

function callAnchor(id: string | undefined) {
  return id ? `#call-${id}` : undefined
}
function evCallId(e: Event | undefined) {
  return e?.tool?.call_id
}
</script>

<template>
  <!-- assistant 文本 / reasoning（可能和 tool_call 在同一行） -->
  <div v-if="showAssistant" class="msg assistant">
    <div class="head">
      <span class="role">assistant</span>
      <span v-if="ev.model" class="mono muted">{{ ev.model }}</span>
      <TimeStamp :ts="ev.ts" />
    </div>
    <details v-if="ev.reasoning" class="reasoning">
      <summary>reasoning <span class="muted">({{ fmtInt(ev.reasoning.length) }} 字符）</span></summary>
      <pre class="reasoning-body">{{ ev.reasoning }}</pre>
    </details>
    <MarkdownText v-if="ev.text" :text="ev.text" />
  </div>

  <!-- tool_call + 配对的 tool_result -->
  <details v-if="isTool" class="tool" :class="{ failed }" :id="`call-${evCallId(ev)}`">
    <summary class="tool-head">
      <span class="name mono">{{ toolName }}</span>
      <span class="summary mono ellipsis" :title="summary">{{ summary }}</span>
      <span class="right">
        <RouterLink v-if="spawned && routeFor(spawned.session)" :to="routeFor(spawned.session)!" class="spawn" @click.stop>
          ↳ 子会话 {{ spawned.session }}
        </RouterLink>
        <span v-else-if="spawned" class="muted spawn" :title="'子会话未归档'">↳ {{ spawned.session }}</span>
        <span v-if="status" class="badge" :class="failed ? 'badge-danger' : 'badge-ok'">{{ status }}</span>
        <span v-else-if="!result" class="badge">no result</span>
        <span v-if="duration" class="mono muted">{{ duration }}</span>
        <TimeStamp :ts="ev.ts" />
      </span>
    </summary>
    <div class="tool-body">
      <div class="sect">
        <div class="lbl">input <span class="mono muted">{{ evCallId(ev) }}</span></div>
        <JsonBlock :value="ev.tool?.input" />
      </div>
      <div v-if="result" class="sect">
        <div class="lbl">
          output
          <span v-if="result.tool?.status" :class="{ danger: failed }">{{ result.tool.status }}</span>
          <TimeStamp :ts="result.ts" />
          <span v-if="!outIsText" class="muted">(json)</span>
        </div>
        <JsonBlock v-if="outText" :value="outIsText ? outText : result.tool?.output" />
        <div v-else class="muted">（空输出）</div>
        <MarkdownText v-if="result.text" :text="result.text" />
      </div>
      <div v-else class="sect muted">没有配对的 tool_result。</div>
    </div>
  </details>

  <!-- 没有 call 的孤儿 tool_result -->
  <details v-else-if="isOrphanResult" class="tool orphan" :class="{ failed }" :id="`call-${evCallId(ev)}`">
    <summary class="tool-head">
      <span class="name mono">{{ toolName }}</span>
      <span class="summary muted">tool_result 没有对应的 tool_call（{{ evCallId(ev) }}）</span>
      <span class="right">
        <span v-if="status" class="badge" :class="failed ? 'badge-danger' : 'badge-ok'">{{ status }}</span>
        <TimeStamp :ts="ev.ts" />
      </span>
    </summary>
    <div class="tool-body">
      <div class="sect">
        <div class="lbl">output</div>
        <JsonBlock :value="outIsText ? outText : ev.tool?.output" />
      </div>
    </div>
  </details>

  <!-- turn 中途的 user 消息（插话 / 父会话送来的） -->
  <div v-else-if="isUser" class="msg user">
    <div class="head">
      <span class="role">user</span>
      <span v-if="delivered" class="badge badge-user" :title="delivered.inferred ? '推断' : ''">
        delivered_from
        <RouterLink v-if="routeFor(delivered.session)" :to="routeFor(delivered.session)!">{{ delivered.session }}</RouterLink>
        <template v-else>{{ delivered.session ?? '?' }}</template>
      </span>
      <span v-if="ev.ext.promptSource === 'queued'" class="badge">queued</span>
      <TimeStamp :ts="ev.ts" />
    </div>
    <pre v-if="!longUser || userOpen" class="user-text">{{ ev.text }}</pre>
    <template v-else>
      <pre class="user-text">{{ ev.text?.slice(0, 1500) }}…</pre>
      <button type="button" @click="userOpen = true">显示全部（{{ fmtInt(ev.text?.length ?? 0) }} 字符）</button>
    </template>
  </div>

  <!-- 后台任务通知 -->
  <details v-else-if="isNotification" class="notify">
    <summary>
      <span class="badge">task-notification</span>
      <a v-if="notifies?.call_id" :href="callAnchor(notifies.call_id)" class="mono" @click.stop>→ {{ notifies.call_id }}</a>
      <span v-if="notifies?.task" class="mono muted">task {{ notifies.task }}</span>
      <span v-if="notifies?.inferred" class="muted" title="老日志没有 origin，靠前缀识别">(inferred)</span>
      <TimeStamp :ts="ev.ts" />
    </summary>
    <pre class="user-text">{{ ev.text }}</pre>
  </details>

  <!-- 兜底：其他带文本的事件 -->
  <details v-else-if="isOtherText" class="other">
    <summary>
      <span class="badge">{{ ev.kind }}</span>
      <span v-if="ev.role" class="muted">{{ ev.role }}</span>
      <span class="summary muted ellipsis">{{ truncate((ev.text ?? ev.reasoning ?? '').replace(/\s+/g, ' '), 140) }}</span>
      <TimeStamp :ts="ev.ts" />
    </summary>
    <pre v-if="ev.text" class="user-text">{{ ev.text }}</pre>
    <JsonBlock v-else :value="ev" />
  </details>
</template>

<style scoped>
.msg {
  padding: 6px 8px;
  border: 1px solid var(--line-1);
  border-radius: var(--radius);
  background: var(--bg);
}
.msg.user {
  border-left: 2px solid var(--user);
}
.msg.assistant {
  border-left: 2px solid var(--assistant);
}
.head {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: var(--fs-xs);
  margin-bottom: 4px;
}
.role {
  font-weight: 600;
  color: var(--fg-1);
}
.user .role {
  color: var(--user);
}
.assistant .role {
  color: var(--assistant);
}
.reasoning {
  margin-bottom: 4px;
  font-size: var(--fs-xs);
  color: var(--fg-1);
}
.reasoning-body {
  margin-top: 4px;
  padding: 6px 8px;
  border-left: 2px solid var(--line);
  color: var(--fg-1);
  font-family: var(--font-ui);
  font-size: var(--fs-sm);
  max-height: 50vh;
  overflow: auto;
}
.user-text {
  font-family: var(--font-ui);
  font-size: var(--fs);
}

.tool {
  border: 1px solid var(--line-1);
  border-left: 2px solid var(--tool);
  border-radius: var(--radius);
  background: var(--bg);
}
.tool.failed {
  border-left-color: var(--danger);
}
.tool.orphan {
  border-left-style: dashed;
}
.tool-head {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 3px 8px 3px 6px;
  min-width: 0;
}
.tool-head .name {
  color: var(--tool);
  font-weight: 600;
  flex: none;
}
.failed .tool-head .name {
  color: var(--danger);
}
.tool-head .summary {
  flex: 1;
  min-width: 0;
  color: var(--fg-1);
}
.tool-head .right {
  flex: none;
  display: inline-flex;
  align-items: center;
  gap: 8px;
  font-size: var(--fs-xs);
}
.spawn {
  font-size: var(--fs-xs);
}
.tool-body {
  padding: 4px 8px 8px;
  border-top: 1px solid var(--line-1);
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.sect .lbl {
  font-size: var(--fs-xs);
  color: var(--fg-2);
  margin-bottom: 3px;
  display: flex;
  gap: 8px;
  align-items: center;
}

.notify,
.other {
  padding: 3px 6px;
  border: 1px dashed var(--line);
  border-radius: var(--radius);
  font-size: var(--fs-sm);
}
.notify > summary,
.other > summary {
  display: flex;
  align-items: center;
  gap: 8px;
}
.notify .user-text,
.other .user-text {
  margin-top: 4px;
  font-size: var(--fs-sm);
  color: var(--fg-1);
}
.other .summary {
  flex: 1;
  min-width: 0;
}
</style>
