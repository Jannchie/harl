<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { RouterLink, useRoute } from 'vue-router'
import type { SessionInfo } from '../api'
import { fmtBytes, fmtInt } from '../lib/format'
import { projectOf } from '../lib/project'
import { providerDot, providerName } from '../lib/providers'
import { cycleScheme, schemeMode } from '../lib/scheme'
import { bySession, loadSessions, loading, machineName, sessions, sessionsError, status } from '../lib/sessions'
import SessionRow from './SessionRow.vue'
import Segmented from './Segmented.vue'
import AtomSpinner from './AtomSpinner.vue'

const route = useRoute()
onMounted(() => loadSessions())

const query = ref('')
const provider = ref('all')
/** 默认展开最近活跃的这几个项目，其余收起；点过的按点的来 */
const OPEN_BY_DEFAULT = 4
/** 一个项目先显示这么多会话 */
const PAGE = 10

const toggledGroups = ref(new Set<string>())
const openParents = ref(new Set<string>())
const fullGroups = ref(new Set<string>())

const activeId = computed(() => (route.name === 'session' ? String(route.params.id) : null))

const providers = computed(() => [...new Set((sessions.value ?? []).map((s) => s.provider))].sort())
const providerOptions = computed(() => [
  { value: 'all', label: '全部' },
  ...providers.value.map((p) => ({ value: p, label: providerName(p), dot: providerDot(p) })),
])

const words = computed(() => query.value.trim().toLowerCase().split(/\s+/).filter(Boolean))

function hit(s: SessionInfo): boolean {
  if (provider.value !== 'all' && s.provider !== provider.value) return false
  if (words.value.length === 0) return true
  const hay = `${s.title ?? ''}\n${s.session}\n${s.cwd ?? ''}\n${s.source}`.toLowerCase()
  return words.value.every((w) => hay.includes(w))
}

interface Node {
  s: SessionInfo
  children: SessionInfo[]
}

interface Group {
  key: string
  label: string
  path: string
  nodes: Node[]
}

const byMtime = (a: SessionInfo, b: SessionInfo) => (a.mtime < b.mtime ? 1 : a.mtime > b.mtime ? -1 : 0)

/** 项目 → 会话 → 子 agent 会话。父会话不在列表里的子会话当普通会话 */
const groups = computed<Group[]>(() => {
  const list = [...(sessions.value ?? [])].sort(byMtime)
  const kids = new Map<string, SessionInfo[]>()
  const top: SessionInfo[] = []
  for (const s of list) {
    const parent = s.parent_session ? bySession.value.get(s.parent_session) : undefined
    if (parent && parent.id !== s.id) kids.set(parent.id, [...(kids.get(parent.id) ?? []), s])
    else top.push(s)
  }
  const out = new Map<string, Group>()
  for (const s of top) {
    const children = (kids.get(s.id) ?? []).filter(hit)
    if (!hit(s) && children.length === 0) continue
    const p = projectOf(s)
    let g = out.get(p.key)
    if (!g) {
      g = { key: p.key, label: p.label, path: p.path, nodes: [] }
      out.set(p.key, g)
    }
    g.nodes.push({ s, children: words.value.length ? children : (kids.get(s.id) ?? []) })
  }
  return [...out.values()]
})

/** 当前会话所在的项目 / 父会话，保证它在侧栏里看得到 */
const activeHome = computed(() => {
  for (const g of groups.value)
    for (const n of g.nodes) {
      if (n.s.id === activeId.value) return { group: g.key, parent: null as string | null }
      if (n.children.some((c) => c.id === activeId.value)) return { group: g.key, parent: n.s.id }
    }
  return null
})

watch(
  activeHome,
  (h) => {
    if (!h) return
    if (!isOpen(h.group, groups.value.findIndex((g) => g.key === h.group))) toggleGroup(h.group)
    if (h.parent && !openParents.value.has(h.parent)) toggleParent(h.parent)
  },
  { immediate: true },
)

function isOpen(key: string, index: number): boolean {
  if (words.value.length) return true
  return (index < OPEN_BY_DEFAULT) !== toggledGroups.value.has(key)
}

function flip(set: typeof toggledGroups, key: string) {
  const next = new Set(set.value)
  if (next.has(key)) next.delete(key)
  else next.add(key)
  set.value = next
}
const toggleGroup = (key: string) => flip(toggledGroups, key)
const toggleParent = (id: string) => flip(openParents, id)

function shown(g: Group): Node[] {
  if (fullGroups.value.has(g.key) || words.value.length) return g.nodes
  const head = g.nodes.slice(0, PAGE)
  // 当前会话排在后面时也要露出来
  const active = g.nodes.find((n, i) => i >= PAGE && (n.s.id === activeId.value || n.children.some((c) => c.id === activeId.value)))
  return active ? [...head, active] : head
}

const matched = computed(() => groups.value.reduce((a, g) => a + g.nodes.length, 0))

const schemeIcon = computed(() =>
  schemeMode.value === 'light' ? 'i-jannchie-sun' : schemeMode.value === 'dark' ? 'i-jannchie-moon' : 'i-jannchie-monitor',
)
const schemeLabel = computed(() =>
  schemeMode.value === 'light' ? '浅色' : schemeMode.value === 'dark' ? '深色' : '跟随系统',
)

const statusLine = computed(() => {
  const s = status.value
  if (!s) return null
  return {
    text: `${fmtInt(s.sources)} 个文件 · ${fmtBytes(s.logical_bytes)}`,
    title: `${s.data_dir}\n机器 ${machineName(s.machine)}（${s.machine}）\n${fmtInt(s.records)} 条快照，${fmtInt(s.packs)} 个 pack 共 ${fmtBytes(s.pack_bytes)}`,
  }
})
</script>

<template>
  <!-- 和正文同一个底色；两列之间靠留白分开，不画分隔线 -->
  <aside class="w-72 shrink-0 flex flex-col px-3">
    <div class="flex items-center justify-between py-3">
      <RouterLink to="/" class="flex items-center gap-2 rounded-md px-1 text-sm font-semibold tracking-tight kbd-ring">
        <i class="i-jannchie-database h-4 w-4 text-blue-500" />
        harl
      </RouterLink>
      <button type="button" class="icon-button" :title="`配色：${schemeLabel}`" :aria-label="`配色：${schemeLabel}`" @click="cycleScheme">
        <i class="h-4 w-4" :class="schemeIcon" />
      </button>
    </div>

    <nav class="flex flex-col gap-0.5 pb-3">
      <RouterLink
        :to="{ name: 'dashboard' }"
        class="flex items-center gap-2 rounded-full py-1.5 pl-2 pr-2.5 text-sm kbd-ring"
        :class="route.name === 'dashboard' ? 'pill-selected' : 'surface-hover'"
      >
        <i class="i-jannchie-chart-column h-4 w-4 opacity-70" />
        概览
      </RouterLink>
      <RouterLink
        :to="{ name: 'sessions' }"
        class="flex items-center gap-2 rounded-full py-1.5 pl-2 pr-2.5 text-sm kbd-ring"
        :class="route.name === 'sessions' ? 'pill-selected' : 'surface-hover'"
      >
        <i class="i-jannchie-archive h-4 w-4 opacity-70" />
        归档
      </RouterLink>
    </nav>

    <div class="flex flex-col gap-2 pb-2">
      <label class="relative block">
        <i class="i-jannchie-search pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 ink-soft" />
        <input v-model="query" type="search" placeholder="搜索会话" aria-label="搜索会话" class="field py-1.5 pl-8 text-sm" />
      </label>
      <Segmented
        v-if="providers.length > 1"
        v-model="provider"
        label="按 Agent 过滤"
        class="self-start"
        :options="providerOptions"
      />
    </div>

    <!-- -mx-1 px-1 给整行的焦点环留位置：滚动容器会裁掉画在盒子外面的 outline -->
    <nav aria-label="会话列表" class="min-h-0 flex-1 flex flex-col gap-0.5 overflow-x-hidden overflow-y-auto -mx-1 px-1 pb-2 pt-1">
      <p v-if="sessionsError" class="px-2 py-4 text-xs ink-danger">会话列表加载失败：{{ sessionsError }}</p>
      <p v-else-if="!sessions" class="flex items-center gap-2 px-2 py-4 text-xs ink-soft">
        <AtomSpinner class="h-3.5 w-3.5" /> 加载会话…
      </p>
      <p v-else-if="sessions.length === 0" class="px-2 py-4 text-xs ink-soft">还没有归档任何会话，先跑 <code>harl capture</code>。</p>
      <p v-else-if="groups.length === 0" class="px-2 py-4 text-xs ink-soft">没有匹配的会话。</p>

      <div v-for="(g, gi) in groups" :key="g.key">
        <button
          type="button"
          class="w-full flex items-center gap-2 rounded-full py-1.5 pl-2 pr-2.5 text-left text-sm ink-muted surface-hover kbd-ring"
          :title="g.path || '日志开头没有记录工作目录'"
          :aria-expanded="isOpen(g.key, gi)"
          @click="toggleGroup(g.key)"
        >
          <i class="h-4 w-4 shrink-0 opacity-70" :class="isOpen(g.key, gi) ? 'i-jannchie-chevron-down' : 'i-jannchie-chevron-right'" />
          <i class="i-jannchie-folder h-4 w-4 shrink-0 opacity-70" />
          <span class="min-w-0 flex-1 truncate">{{ g.label }}</span>
          <span class="shrink-0 text-xs text-neutral-500 tabular-nums">{{ g.nodes.length }}</span>
        </button>

        <!-- 竖线在箭头中心下方；会话图标和项目图标落在同一列 -->
        <div v-if="isOpen(g.key, gi)" class="ml-4 flex flex-col gap-0.5 border-l border-neutral-500/15 py-0.5 pl-[7px]">
          <template v-for="n in shown(g)" :key="n.s.id">
            <SessionRow
              :session="n.s"
              :selected="n.s.id === activeId"
              :child-count="n.children.length"
              :open="openParents.has(n.s.id) || (words.length > 0 && n.children.length > 0)"
              @toggle="toggleParent(n.s.id)"
            />
            <div
              v-if="n.children.length && (openParents.has(n.s.id) || words.length > 0)"
              class="ml-4 flex flex-col gap-0.5 border-l border-neutral-500/15 py-0.5 pl-[7px]"
            >
              <SessionRow v-for="c in n.children" :key="c.id" :session="c" :selected="c.id === activeId" child />
            </div>
          </template>
          <button
            v-if="shown(g).length < g.nodes.length"
            type="button"
            class="w-full rounded-full py-1 pl-8 text-left text-xs ink-soft surface-hover kbd-ring"
            @click="fullGroups = new Set(fullGroups).add(g.key)"
          >
            还有 {{ g.nodes.length - shown(g).length }} 个会话
          </button>
        </div>
      </div>
    </nav>

    <div class="flex items-center gap-2 py-2 text-xs ink-soft">
      <span class="min-w-0 flex-1 truncate num" :title="statusLine?.title" role="status" aria-live="polite">
        <template v-if="words.length || provider !== 'all'">匹配 {{ fmtInt(matched) }} 个会话</template>
        <template v-else>{{ statusLine?.text ?? '' }}</template>
      </span>
      <button type="button" class="icon-button h-7! w-7!" title="重新加载会话列表" aria-label="重新加载会话列表" @click="loadSessions(true)">
        <AtomSpinner v-if="loading" class="h-3.5 w-3.5" />
        <i v-else class="i-jannchie-refresh h-3.5 w-3.5" />
      </button>
    </div>
  </aside>
</template>
