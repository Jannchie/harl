<script setup lang="ts">
import { computed, onUnmounted, provide, ref, shallowRef, watch } from 'vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { api, ApiError, type Event, type SessionInfo } from '../api'
import { groupEvents, type Grouped } from '../lib/group'
import { fmtBytes, fmtInt, shortenPath, truncate } from '../lib/format'
import { SESSIONS_KEY } from '../lib/ctx'
import EventRow from '../components/EventRow.vue'
import ItemView from '../components/ItemView.vue'
import ProviderTag from '../components/ProviderTag.vue'
import TimeStamp from '../components/TimeStamp.vue'
import TurnBlock from '../components/TurnBlock.vue'

const props = defineProps<{ id: string }>()
const route = useRoute()
const router = useRouter()

const events = shallowRef<Event[] | null>(null)
const error = ref<string | null>(null)
const sessions = shallowRef<SessionInfo[] | null>(null)
const sessionsError = ref<string | null>(null)

/** session id（文件名）→ 列表项（同名多条时取最新 captured 的） */
const bySession = computed(() => {
  const m = new Map<string, SessionInfo>()
  for (const s of [...(sessions.value ?? [])].sort((a, b) => (a.captured_at < b.captured_at ? -1 : 1))) m.set(s.session, s)
  return m
})
provide(SESSIONS_KEY, bySession)

const info = computed(() => sessions.value?.find((s) => s.id === props.id) ?? null)
const parent = computed(() => (info.value?.parent_session ? (bySession.value.get(info.value.parent_session) ?? null) : null))
const children = computed(() =>
  info.value ? (sessions.value ?? []).filter((s) => s.parent_session === info.value!.session && s.id !== props.id) : [],
)

let ac: AbortController | null = null

async function load(id: string) {
  ac?.abort()
  ac = new AbortController()
  const signal = ac.signal
  events.value = null
  error.value = null
  const [ev, ss] = await Promise.allSettled([api.events(id, signal), sessions.value ? Promise.resolve(sessions.value) : api.sessions(signal)])
  if (signal.aborted) return
  if (ev.status === 'fulfilled') events.value = ev.value
  else error.value = describe(ev.reason)
  if (ss.status === 'fulfilled') sessions.value = ss.value
  else sessionsError.value = describe(ss.reason)
}

function describe(e: unknown): string {
  if (e instanceof ApiError) return e.message
  return e instanceof Error ? e.message : String(e)
}

watch(() => props.id, load, { immediate: true })
onUnmounted(() => ac?.abort())

const showAll = computed({
  get: () => route.query.all === '1',
  set: (v: boolean) => {
    const q = { ...route.query }
    if (v) q.all = '1'
    else delete q.all
    void router.replace({ query: q })
  },
})

const grouped = computed<Grouped | null>(() => (events.value ? groupEvents(events.value) : null))

const stats = computed(() => {
  const evs = events.value ?? []
  const g = grouped.value
  const turns = g ? g.blocks.filter((b) => b.type === 'turn').length : 0
  let steps = 0
  let tools = 0
  if (g) for (const b of g.blocks) if (b.type === 'turn') (steps += b.turn.stepCount), (tools += b.turn.toolCount)
  const kinds = new Map<string, number>()
  for (const e of evs) kinds.set(e.kind, (kinds.get(e.kind) ?? 0) + 1)
  return { events: evs.length, turns, steps, tools, kinds, first: evs[0]?.ts, last: evs[evs.length - 1]?.ts }
})

const outline = computed(() =>
  (grouped.value?.blocks ?? []).flatMap((b) =>
    b.type === 'turn'
      ? [{ key: b.turn.key, n: b.turn.n, text: truncate((b.turn.userText ?? '').replace(/\s+/g, ' ').trim() || '（无文本）', 64) }]
      : [],
  ),
)

const sessionLabel = computed(() => info.value?.session ?? events.value?.[0]?.session ?? props.id)
const provider = computed(() => info.value?.provider ?? events.value?.[0]?.provider ?? null)
</script>

<template>
  <div class="page session">
    <header class="sess-head">
      <div class="row1">
        <ProviderTag v-if="provider" :provider="provider" />
        <h1 class="mono">{{ sessionLabel }}</h1>
        <span v-if="parent" class="mono muted">
          ↰ <RouterLink :to="{ name: 'session', params: { id: parent.id } }">{{ parent.session }}</RouterLink>
        </span>
        <span v-else-if="info?.parent_session" class="mono muted" title="父会话未归档">↰ {{ info.parent_session }}</span>
        <span class="grow"></span>
        <label class="toggle">
          <input type="checkbox" v-model="showAll" />
          显示全部事件
          <span v-if="grouped && grouped.hidden" class="muted">（+{{ fmtInt(grouped.hidden) }} 隐藏）</span>
        </label>
        <a :href="api.rawUrl(id)" target="_blank" rel="noopener" class="raw">原文 ↗</a>
      </div>
      <div class="row2 mono" v-if="info">
        <span class="ellipsis src" :title="info.source">{{ shortenPath(info.source, 100) }}</span>
        <span class="muted">{{ info.machine }}</span>
        <span>{{ fmtInt(info.lines) }} lines · {{ fmtBytes(info.size) }}</span>
        <span class="muted">mtime <TimeStamp :ts="info.mtime" mode="full" /></span>
        <span class="muted">captured <TimeStamp :ts="info.captured_at" mode="rel" /></span>
        <span class="muted" :title="id">tree {{ id.slice(0, 12) }}</span>
      </div>
      <div class="row2 mono" v-else-if="sessionsError">
        <span class="danger">会话列表加载失败：{{ sessionsError }}</span>
      </div>
      <div class="row2 mono" v-if="events">
        <span>{{ fmtInt(stats.events) }} events · {{ stats.turns }} turns · {{ stats.steps }} steps · {{ stats.tools }} tool calls</span>
        <span class="muted"><TimeStamp :ts="stats.first" mode="full" /> → <TimeStamp :ts="stats.last" mode="full" /></span>
        <span class="muted kinds">
          <span v-for="[k, n] in stats.kinds" :key="k">{{ k }} {{ n }}</span>
        </span>
      </div>
      <div class="row2 mono" v-if="children.length">
        <span class="muted">子会话</span>
        <RouterLink v-for="c in children" :key="c.id" :to="{ name: 'session', params: { id: c.id } }">{{ c.session }}</RouterLink>
      </div>
    </header>

    <div v-if="error" class="state error">加载失败：<code>{{ error }}</code></div>
    <div v-else-if="!events" class="state">加载事件…</div>
    <div v-else-if="events.length === 0" class="state">这个快照没有事件。</div>

    <!-- 平铺：所有事件按原顺序 -->
    <div v-else-if="showAll" class="flat">
      <div class="flat-head mono muted">
        <span class="mark"></span><span class="seq">seq</span><span class="ts">ts</span><span class="kind">kind</span><span class="role">role</span
        ><span class="scope">t/s</span><span>summary（点开看完整事件）</span>
      </div>
      <EventRow v-for="e in events" :key="e.seq" :ev="e" />
    </div>

    <!-- 分组：turn → step -->
    <div v-else-if="grouped" class="grouped" :class="{ 'with-outline': outline.length > 1 }">
      <nav class="outline" v-if="outline.length > 1">
        <div class="outline-title muted">turns</div>
        <a v-for="o in outline" :key="o.key" :href="`#turn-${o.key}`" class="outline-item" :title="o.text">
          <span class="mono n">#{{ o.n ?? '?' }}</span> <span class="ellipsis t">{{ o.text }}</span>
        </a>
      </nav>
      <div class="content">
        <template v-for="b in grouped.blocks" :key="b.type === 'turn' ? b.turn.key : `loose:${b.firstSeq}`">
          <TurnBlock v-if="b.type === 'turn'" :turn="b.turn" />
          <details v-else class="loose" open>
            <summary class="loose-head muted">未分组 · {{ b.items.length }} 事件 · seq {{ b.firstSeq }}…</summary>
            <div class="loose-body">
              <ItemView v-for="it in b.items" :key="it.ev.seq" :item="it" />
            </div>
          </details>
        </template>
      </div>
    </div>
  </div>
</template>

<style scoped>
.sess-head {
  padding: 8px 0 8px;
  border-bottom: 1px solid var(--line);
  margin-bottom: 10px;
  display: flex;
  flex-direction: column;
  gap: 3px;
}
.row1 {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
}
.row1 h1 {
  font-size: var(--fs);
  font-weight: 700;
  margin: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.grow {
  flex: 1;
}
.toggle {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: var(--fs-sm);
  cursor: pointer;
  white-space: nowrap;
}
.raw {
  white-space: nowrap;
}
.row2 {
  display: flex;
  flex-wrap: wrap;
  gap: 4px 14px;
  font-size: var(--fs-xs);
  color: var(--fg-1);
  min-width: 0;
}
.src {
  max-width: 60%;
}
.kinds {
  display: inline-flex;
  gap: 8px;
  flex-wrap: wrap;
}

.flat-head {
  display: flex;
  gap: 8px;
  padding: 2px 4px;
  font-size: var(--fs-xs);
  border-bottom: 1px solid var(--line);
}
.flat-head .mark {
  width: 1em;
}
.flat-head .seq {
  width: 52px;
  text-align: right;
}
.flat-head .ts {
  width: 60px;
}
.flat-head .kind {
  width: 108px;
}
.flat-head .role {
  width: 64px;
}
.flat-head .scope {
  width: 56px;
}

.grouped {
  display: grid;
  grid-template-columns: minmax(0, 1fr);
  gap: 16px;
  align-items: start;
}
@media (min-width: 1100px) {
  .grouped.with-outline {
    grid-template-columns: 220px minmax(0, 1fr);
  }
}
.outline {
  position: sticky;
  top: 40px;
  max-height: calc(100vh - 56px);
  overflow: auto;
  font-size: var(--fs-xs);
  border-right: 1px solid var(--line-1);
  padding-right: 6px;
  display: none;
}
@media (min-width: 1100px) {
  .outline {
    display: block;
  }
}
.outline-title {
  padding: 2px 4px 4px;
  text-transform: uppercase;
  letter-spacing: 0.04em;
}
.outline-item {
  display: flex;
  gap: 6px;
  padding: 1px 4px;
  color: var(--fg-1);
  border-radius: var(--radius);
  min-width: 0;
}
.outline-item:hover {
  background: var(--bg-1);
  text-decoration: none;
}
.outline-item .n {
  color: var(--fg-2);
  flex: none;
  width: 30px;
}
.outline-item .t {
  min-width: 0;
}
.content {
  min-width: 0;
}
.loose {
  border: 1px dashed var(--line);
  border-radius: var(--radius);
  margin-bottom: 10px;
  padding: 3px 8px;
}
.loose-head {
  font-size: var(--fs-xs);
}
.loose-body {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 4px 0;
}
</style>
