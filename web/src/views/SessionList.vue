<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { RouterLink } from 'vue-router'
import { api, ApiError, type SessionInfo, type Status } from '../api'
import { fmtBytes, fmtInt, shortenPath } from '../lib/format'
import ProviderTag from '../components/ProviderTag.vue'
import StatusBar from '../components/StatusBar.vue'
import TimeStamp from '../components/TimeStamp.vue'

const sessions = ref<SessionInfo[] | null>(null)
const status = ref<Status | null>(null)
const error = ref<string | null>(null)
const statusError = ref<string | null>(null)
const query = ref('')
const expanded = ref(new Set<string>())

const ac = new AbortController()

async function load() {
  error.value = null
  const [s, st] = await Promise.allSettled([api.sessions(ac.signal), api.status(ac.signal)])
  if (s.status === 'fulfilled') sessions.value = s.value
  else if (!(s.reason instanceof DOMException)) error.value = describe(s.reason)
  if (st.status === 'fulfilled') status.value = st.value
  else if (!(st.reason instanceof DOMException)) statusError.value = describe(st.reason)
}

function describe(e: unknown): string {
  if (e instanceof ApiError) return e.message
  return e instanceof Error ? e.message : String(e)
}

onMounted(load)
onUnmounted(() => ac.abort())

interface Row {
  s: SessionInfo
  children: Row[]
}

const byCaptured = (a: SessionInfo, b: SessionInfo) => (a.captured_at < b.captured_at ? 1 : a.captured_at > b.captured_at ? -1 : 0)

/** 父子树：parent_session 能在列表里找到的挂到父下面，找不到的当顶层 */
const tree = computed<Row[]>(() => {
  const list = sessions.value ?? []
  const bySession = new Map<string, Row>()
  const rows = list.map((s) => ({ s, children: [] as Row[] }))
  // 同一 session id 可能有多条（多机器 / 多路径），子会话挂到最新一条上
  for (const r of [...rows].sort((a, b) => byCaptured(a.s, b.s)).reverse()) bySession.set(r.s.session, r)
  const top: Row[] = []
  for (const r of rows) {
    const parent = r.s.parent_session ? bySession.get(r.s.parent_session) : undefined
    if (parent && parent !== r) parent.children.push(r)
    else top.push(r)
  }
  const sortRows = (rs: Row[]) => {
    rs.sort((a, b) => byCaptured(a.s, b.s))
    for (const r of rs) sortRows(r.children)
  }
  sortRows(top)
  return top
})

const q = computed(() => query.value.trim().toLowerCase())

function matches(s: SessionInfo): boolean {
  if (!q.value) return true
  const hay = `${s.source}\n${s.session}\n${s.machine}\n${s.provider}`.toLowerCase()
  return q.value.split(/\s+/).every((w) => hay.includes(w))
}

interface FlatRow {
  s: SessionInfo
  depth: number
  childCount: number
  open: boolean
}

/** 过滤 + 展开后的平铺行。搜索时命中的子会话把父会话一起带出来并展开 */
const flat = computed<FlatRow[]>(() => {
  const out: FlatRow[] = []
  const walk = (r: Row, depth: number) => {
    const selfHit = matches(r.s)
    const hitChildren = r.children.filter((c) => subtreeHit(c))
    if (!selfHit && hitChildren.length === 0) return
    const shownChildren = q.value ? hitChildren : r.children
    const open = q.value ? hitChildren.length > 0 : expanded.value.has(r.s.id)
    out.push({ s: r.s, depth, childCount: r.children.length, open })
    if (open) for (const c of shownChildren) walk(c, depth + 1)
  }
  const subtreeHit = (r: Row): boolean => matches(r.s) || r.children.some(subtreeHit)
  for (const r of tree.value) walk(r, 0)
  return out
})

function toggle(id: string) {
  const set = new Set(expanded.value)
  if (set.has(id)) set.delete(id)
  else set.add(id)
  expanded.value = set
}

const totalShown = computed(() => flat.value.length)
</script>

<template>
  <div class="page">
    <div class="bar">
      <input
        v-model="query"
        type="search"
        placeholder="按路径 / session / machine 过滤（空格分隔多个词）"
        class="search mono"
        autofocus
      />
      <span class="muted count" v-if="sessions">
        {{ fmtInt(totalShown) }} / {{ fmtInt(sessions.length) }}
      </span>
      <button type="button" @click="load()" :disabled="!sessions && !error">刷新</button>
    </div>

    <div v-if="error" class="state error">加载失败：<code>{{ error }}</code></div>
    <div v-else-if="!sessions" class="state">加载会话列表…</div>
    <div v-else-if="sessions.length === 0" class="state">还没有归档任何会话。先跑 <code>harl capture</code>。</div>
    <div v-else-if="flat.length === 0" class="state">没有匹配的会话。</div>
    <table v-else class="list">
      <thead>
        <tr>
          <th class="c-prov">provider</th>
          <th class="c-sess">session</th>
          <th class="c-src">source</th>
          <th class="c-num num">lines</th>
          <th class="c-num num">size</th>
          <th class="c-time">mtime</th>
          <th class="c-time">captured</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="r in flat" :key="r.s.id" :class="{ child: r.depth > 0 }">
          <td class="c-prov"><ProviderTag :provider="r.s.provider" /></td>
          <td class="c-sess mono">
            <span class="indent" :style="{ width: `${r.depth * 16}px` }"></span>
            <span v-if="r.depth > 0" class="muted">└ </span>
            <RouterLink :to="{ name: 'session', params: { id: r.s.id } }" :title="`tree ${r.s.id}`">{{ r.s.session }}</RouterLink>
            <span v-if="r.s.title" class="title dim" :title="r.s.title">{{ r.s.title }}</span>
            <button
              v-if="r.childCount > 0"
              type="button"
              class="kids"
              :class="{ active: r.open }"
              :title="r.open ? '折叠子会话' : '展开子会话'"
              @click="toggle(r.s.id)"
            >
              {{ r.open ? '▾' : '▸' }} {{ r.childCount }} 子会话
            </button>
            <span v-else-if="r.s.parent_session && r.depth === 0" class="muted orphan" :title="`父会话 ${r.s.parent_session} 未归档`">
              ↰ {{ r.s.parent_session }}
            </span>
          </td>
          <td class="c-src mono ellipsis" :title="`${r.s.source}\n${r.s.machine}`">{{ shortenPath(r.s.source, 72) }}</td>
          <td class="c-num num mono">{{ fmtInt(r.s.lines) }}</td>
          <td class="c-num num mono" :title="`${fmtInt(r.s.size)} B`">{{ fmtBytes(r.s.size) }}</td>
          <td class="c-time"><TimeStamp :ts="r.s.mtime" mode="full" /></td>
          <td class="c-time"><TimeStamp :ts="r.s.captured_at" mode="rel" /></td>
        </tr>
      </tbody>
    </table>

    <StatusBar :status="status" :error="statusError" />
  </div>
</template>

<style scoped>
.bar {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 0 8px;
  position: sticky;
  top: 32px;
  background: var(--bg);
  z-index: 5;
}
.search {
  flex: 1;
  max-width: 560px;
}
.count {
  font-size: var(--fs-xs);
  font-variant-numeric: tabular-nums;
}
.list {
  table-layout: fixed;
}
.list td {
  padding-top: 2px;
  padding-bottom: 2px;
}
.c-prov {
  width: 96px;
}
.c-sess {
  width: 34%;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.c-src {
  color: var(--fg-1);
}
.c-num {
  width: 76px;
}
.c-time {
  width: 150px;
  white-space: nowrap;
}
.indent {
  display: inline-block;
}
.child td {
  background: var(--bg-1);
}
.title {
  margin-left: 8px;
  font-family: var(--font-ui);
}
.kids {
  margin-left: 8px;
  font-size: var(--fs-xs);
  padding: 0 5px;
}
.orphan {
  margin-left: 8px;
  font-size: var(--fs-xs);
}
@media (max-width: 900px) {
  .c-time,
  .c-num {
    display: none;
  }
}
</style>
