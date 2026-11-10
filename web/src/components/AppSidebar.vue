<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { RouterLink, useRoute } from 'vue-router'
import type { SessionInfo } from '../api'
import { groupAgents, newestFirst, rootOf, RUNNING_MS, sessionTitle, type AgentGroup } from '../lib/agents'
import { fmtBytes, fmtInt, tn } from '../lib/format'
import { useNow } from '../lib/now'
import { projectOf } from '../lib/project'
import { providerDot, providerName } from '../lib/providers'
import { cycleScheme, schemeMode } from '../lib/scheme'
import { bySession, loadSessions, loading, machineName, sessions, sessionsError, status } from '../lib/sessions'
import AgentList from './AgentList.vue'
import SessionRow from './SessionRow.vue'
import MultiSelect from './MultiSelect.vue'
import AtomSpinner from './AtomSpinner.vue'
import UpdateBadge from './UpdateBadge.vue'
import LocaleMenu from './LocaleMenu.vue'

const { t } = useI18n()
const route = useRoute()
onMounted(() => loadSessions())

const query = ref('')
/** 选中的 provider；空是全部 */
const providerSel = ref<string[]>([])
/** 默认展开最近活跃的这几个项目，其余收起；点过的按点的来 */
const OPEN_BY_DEFAULT = 4
/** 一个项目先显示这么多会话 */
const PAGE = 10
/** 根会话下面一组子 agent、一串 workflow 运行，各先显示这么多 */
const CHILD_PAGE = 8

/** 用户手动开 / 关过的项目；按 key 记，不随排序或筛选后的位置变 */
const groupOverride = ref(new Map<string, boolean>())
const openParents = ref(new Set<string>())
const fullGroups = ref(new Set<string>())
/** 根会话下展开的 workflow 运行、点过「再显示」的子 agent 列表；键是 `<根会话 id>|<组键>`（见 listKey） */
const openRuns = ref(new Set<string>())
const fullLists = ref(new Set<string>())

/** 「进行中」按 mtime 算，隔一会儿重算一次 */
const now = useNow(30_000)

const activeId = computed(() => (route.name === 'session' ? String(route.params.id) : null))

const providers = computed(() => [...new Set((sessions.value ?? []).map((s) => s.provider))].sort())
const providerOptions = computed(() => providers.value.map((p) => ({ id: p, label: providerName(p), dot: providerDot(p) })))
const searching = computed(() => words.value.length > 0)
const filtering = computed(() => searching.value || providerSel.value.length > 0)

const NAV = [
  { name: 'dashboard', label: 'common.nav.dashboard', icon: 'i-jannchie-chart-column' },
  { name: 'sessions', label: 'common.nav.archive', icon: 'i-jannchie-archive' },
] as const

const words = computed(() => query.value.trim().toLowerCase().split(/\s+/).filter(Boolean))

function hit(s: SessionInfo): boolean {
  if (providerSel.value.length && !providerSel.value.includes(s.provider)) return false
  if (words.value.length === 0) return true
  const hay = `${s.label ?? ''}\n${s.title ?? ''}\n${s.run_title ?? ''}\n${s.session}\n${s.cwd ?? ''}\n${s.source}`.toLowerCase()
  return words.value.every((w) => hay.includes(w))
}

interface Node {
  s: SessionInfo
  /** 根会话下面全部后代（父链拉平），搜索时只留命中的 */
  children: SessionInfo[]
  /** children 里不属于任何编排的子 agent，直接列在根会话下面 */
  plain: SessionInfo[]
  /** children 里的 workflow 运行，各自一组，可折叠 */
  runs: AgentGroup[]
}

interface Group {
  key: string
  label: string
  path: string
  nodes: Node[]
}

/** 根会话下一串东西的状态键：组键是 `agents`（普通子 agent）、`runs`（运行列表本身）或 AgentGroup.key */
const listKey = (rootId: string, key: string) => `${rootId}|${key}`

const NO_AGENTS: ReturnType<typeof groupAgents> = { plain: [], runs: [] }

/** 顶层只放根会话；子 agent 不论隔几层都挂到根会话下面。同一个会话的几份副本（多机器、Claude Desktop 的 ssh 镜像）
 * 只留 bySession 选中的那份。父会话不在列表里的子会话当根会话。
 * 不搜索时的分组和解析好的 mtime 也在这里算，只在会话列表刷新时重算 */
const tree = computed(() => {
  const pick = bySession.value
  const list = (sessions.value ?? []).filter((s) => pick.get(s.session) === s).sort(newestFirst)
  const kids = new Map<string, SessionInfo[]>()
  const top: SessionInfo[] = []
  for (const s of list) {
    const root = rootOf(s, pick)
    if (root === s) top.push(s)
    else {
      const k = kids.get(root.id)
      if (k) k.push(s)
      else kids.set(root.id, [s])
    }
  }
  const grouped = new Map([...kids].map(([id, k]) => [id, groupAgents(k)] as const))
  const stamps = list.map((s) => [s.id, Date.parse(s.mtime)] as const)
  return { top, kids, grouped, stamps }
})

/** 项目 → 根会话 → 子 agent（分组）；搜索时只把命中的子 agent 重新分组 */
const groups = computed<Group[]>(() => {
  const { top, kids, grouped } = tree.value
  const out = new Map<string, Group>()
  for (const s of top) {
    const all = kids.get(s.id) ?? []
    const children = searching.value ? all.filter(hit) : all
    if (!hit(s) && children.length === 0) continue
    const p = projectOf(s)
    let g = out.get(p.key)
    if (!g) {
      g = { key: p.key, label: p.label, path: p.path, nodes: [] }
      out.set(p.key, g)
    }
    const { plain, runs } = searching.value ? groupAgents(children) : (grouped.get(s.id) ?? NO_AGENTS)
    g.nodes.push({ s, children, plain, runs })
  }
  return [...out.values()]
})

const isActive = (c: SessionInfo) => c.id === activeId.value

/** 当前会话所在的项目 / 根会话 / workflow 运行，保证它在侧栏里看得到 */
const activeHome = computed(() => {
  for (const g of groups.value)
    for (const n of g.nodes) {
      if (n.s.id === activeId.value) return { group: g.key, parent: null as string | null, run: null as string | null }
      if (n.plain.some(isActive)) return { group: g.key, parent: n.s.id, run: null }
      const r = n.runs.find((r) => r.items.some(isActive))
      if (r) return { group: g.key, parent: n.s.id, run: listKey(n.s.id, r.key) }
    }
  return null
})

watch(
  activeHome,
  (h) => {
    if (!h) return
    if (!isOpen(h.group, groups.value.findIndex((g) => g.key === h.group))) setGroup(h.group, true)
    if (h.parent && !openParents.value.has(h.parent)) toggleParent(h.parent)
    if (h.run && !openRuns.value.has(h.run)) flip(openRuns, h.run)
  },
  { immediate: true },
)

function isOpen(key: string, index: number): boolean {
  if (searching.value) return true
  return groupOverride.value.get(key) ?? index < OPEN_BY_DEFAULT
}

function setGroup(key: string, open: boolean) {
  groupOverride.value = new Map(groupOverride.value).set(key, open)
}

function flip(set: typeof openParents, key: string) {
  const next = new Set(set.value)
  if (next.has(key)) next.delete(key)
  else next.add(key)
  set.value = next
}
function toggleGroup(key: string, index: number) {
  setGroup(key, !isOpen(key, index))
}
function toggleParent(id: string) {
  flip(openParents, id)
}
function toggleRun(key: string) {
  flip(openRuns, key)
}
/** 点过「再显示」：子 agent / 运行列表按 listKey 记，项目按项目 key 记 */
function showAll(key: string) {
  fullLists.value = new Set(fullLists.value).add(key)
}
function showAllNodes(key: string) {
  fullGroups.value = new Set(fullGroups.value).add(key)
}

/** 子会话展开：点开过的，或者在搜索（命中可能就在子会话里） */
const childrenOpen = (n: Node) => n.children.length > 0 && (searching.value || openParents.value.has(n.s.id))

/** 一串里先给 size 个；当前会话排在后面时也露出来。搜索时、点过「再显示」的全给 */
function firstOf<T>(list: T[], size: number, full: boolean, hasActive: (x: T) => boolean): T[] {
  if (full || searching.value || list.length <= size) return list
  const head = list.slice(0, size)
  const active = list.find((x, i) => i >= size && hasActive(x))
  return active ? [...head, active] : head
}

/** workflow 运行的组名：转述的用户请求；没有就退回第一个 agent 的显示名 */
const runName = (g: AgentGroup) => g.title ?? sessionTitle(g.items[0]!) ?? null

interface ListView {
  key: string
  /** 要显示的行 */
  items: SessionInfo[]
  /** 收起来没显示的个数 */
  more: number
}
interface RunView {
  key: string
  name: string | null
  size: number
  open: boolean
  /** 展开时才有 */
  list: ListView | null
}
interface NodeView {
  node: Node
  open: boolean
  plain: ListView | null
  runs: RunView[]
  runsKey: string
  moreRuns: number
}
interface GroupView {
  group: Group
  open: boolean
  nodes: NodeView[]
  more: number
}

function agentList(key: string, all: SessionInfo[]): ListView {
  const items = firstOf(all, CHILD_PAGE, fullLists.value.has(key), isActive)
  return { key, items, more: all.length - items.length }
}

function nodeView(n: Node): NodeView {
  const runsKey = listKey(n.s.id, 'runs')
  if (!childrenOpen(n)) return { node: n, open: false, plain: null, runs: [], runsKey, moreRuns: 0 }
  const runs = firstOf(n.runs, CHILD_PAGE, fullLists.value.has(runsKey), (g) => g.items.some(isActive))
  return {
    node: n,
    open: true,
    plain: n.plain.length ? agentList(listKey(n.s.id, 'agents'), n.plain) : null,
    runs: runs.map((g) => {
      const key = listKey(n.s.id, g.key)
      const open = searching.value || openRuns.value.has(key)
      return { key, name: runName(g), size: g.items.length, open, list: open ? agentList(key, g.items) : null }
    }),
    runsKey,
    moreRuns: n.runs.length - runs.length,
  }
}

/** 模板直接渲染的结构：开合、分页、组名都在这里算好。「进行中」另算（runningIds / runningCounts），不带着它重算 */
const view = computed<GroupView[]>(() =>
  groups.value.map((g, gi) => {
    if (!isOpen(g.key, gi)) return { group: g, open: false, nodes: [], more: 0 }
    const nodes = firstOf(g.nodes, PAGE, fullGroups.value.has(g.key), (n) => isActive(n.s) || n.children.some(isActive))
    return { group: g, open: true, nodes: nodes.map(nodeView), more: g.nodes.length - nodes.length }
  }),
)

/** 还在写的会话 id；只有它跟着 now 走。集合没变就沿用上一个，下游不重算 */
const runningIds = computed<ReadonlySet<string>>((prev) => {
  const at = now.value
  const next = new Set<string>()
  for (const [id, ms] of tree.value.stamps) if (at - ms < RUNNING_MS) next.add(id)
  return prev && prev.size === next.size && [...next].every((id) => prev.has(id)) ? prev : next
})

/** 根会话（键是会话 id）和 workflow 运行（键是 listKey）下面还在写的个数 */
const runningCounts = computed(() => {
  const ids = runningIds.value
  const out = new Map<string, number>()
  if (ids.size === 0) return out
  const count = (list: SessionInfo[]) => list.reduce((a, s) => a + (ids.has(s.id) ? 1 : 0), 0)
  for (const g of groups.value)
    for (const n of g.nodes) {
      out.set(n.s.id, count(n.children))
      for (const r of n.runs) out.set(listKey(n.s.id, r.key), count(r.items))
    }
  return out
})

const matched = computed(() => groups.value.reduce((a, g) => a + g.nodes.length, 0))

const schemeIcon = computed(() =>
  schemeMode.value === 'light' ? 'i-jannchie-sun' : schemeMode.value === 'dark' ? 'i-jannchie-moon' : 'i-jannchie-monitor',
)
const schemeLabel = computed(() =>
  t(`common.scheme.${schemeMode.value}`),
)

const statusLine = computed(() => {
  const s = status.value
  if (!s) return null
  return {
    text: t('common.sidebar.statusText', { files: fmtInt(s.sources), size: fmtBytes(s.logical_bytes) }, s.sources),
    title: t('common.sidebar.statusTitle', {
      dir: s.data_dir,
      name: machineName(s.machine),
      id: s.machine,
      records: fmtInt(s.records),
      packs: fmtInt(s.packs),
      size: fmtBytes(s.pack_bytes),
    }),
  }
})
</script>

<template>
  <!-- 比正文深一档的底色把侧栏和正文分开，不画分隔线 -->
  <aside class="h-full w-72 shrink-0 flex flex-col bg-rail">
    <div class="flex h-13 shrink-0 items-center px-4">
      <RouterLink to="/" class="-ml-1 flex items-center gap-2 rounded-md px-1 py-0.5 kbd-ring">
        <span class="h-6 w-6 flex items-center justify-center rounded-md bg-accent text-white dark:text-rail" aria-hidden="true">
          <i class="i-jannchie-database h-3.5 w-3.5" />
        </span>
        <span class="text-[15px] font-semibold tracking-tight">harl</span>
      </RouterLink>
    </div>

    <nav :aria-label="t('common.nav.label')" class="flex flex-col gap-px px-2 pb-4">
      <RouterLink
        v-for="item in NAV"
        :key="item.name"
        :to="{ name: item.name }"
        class="rail-item gap-2.5 px-2 text-sm"
        :class="route.name === item.name ? 'row-selected' : 'rail-item-idle'"
        :aria-current="route.name === item.name ? 'page' : undefined"
      >
        <i class="h-4 w-4" :class="[item.icon, route.name === item.name ? 'text-accent' : '']" aria-hidden="true" />
        {{ t(item.label) }}
      </RouterLink>
    </nav>

    <div class="flex flex-col gap-2 px-2 pb-2">
      <div class="flex items-center justify-between px-2">
        <h2 class="text-xs font-medium ink-soft">{{ t('common.metric.sessions') }}</h2>
        <MultiSelect v-if="providers.length > 1" v-model="providerSel" label="Agent" :options="providerOptions" align="right" />
      </div>
      <label class="relative block">
        <i class="i-jannchie-search pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 ink-soft" aria-hidden="true" />
        <input v-model="query" type="search" :placeholder="t('common.sidebar.searchPlaceholder')" :aria-label="t('common.sidebar.searchLabel')" class="field py-1.5 pl-8 text-[13px]" />
      </label>
    </div>

    <!-- px-2 也给整行的焦点环留了位置：滚动容器会裁掉画在盒子外面的 outline -->
    <nav :aria-label="t('common.sidebar.listLabel')" class="min-h-0 flex-1 flex flex-col gap-3 overflow-x-hidden overflow-y-auto px-2 pb-3 pt-1">
      <p v-if="sessionsError" class="px-2 py-4 text-xs ink-danger">{{ t('common.sidebar.loadFailed', { error: sessionsError }) }}</p>
      <p v-else-if="!sessions" class="flex items-center gap-2 px-2 py-4 text-xs ink-soft">
        <AtomSpinner class="h-3.5 w-3.5" /> {{ t('common.sidebar.loading') }}
      </p>
      <i18n-t v-else-if="sessions.length === 0" keypath="common.sidebar.empty" scope="global" tag="p" class="px-2 py-4 text-xs ink-soft">
        <template #cmd><code>harl capture</code></template>
      </i18n-t>
      <p v-else-if="groups.length === 0" class="px-2 py-4 text-xs ink-soft">{{ t('common.sidebar.noMatch') }}</p>

      <section v-for="(gv, gi) in view" :key="gv.group.key" class="flex flex-col gap-px">
        <button
          type="button"
          class="w-full flex items-center gap-2 rounded-md py-1 pl-2 pr-2.5 text-left text-xs kbd-ring transition-colors duration-150 surface-hover"
          :title="gv.group.path || t('common.sidebar.noCwd')"
          :aria-expanded="gv.open"
          @click="toggleGroup(gv.group.key, gi)"
        >
          <i
            class="i-jannchie-chevron-right h-4 w-4 shrink-0 ink-soft transition-transform duration-150"
            :class="gv.open ? 'rotate-90' : ''"
            aria-hidden="true"
          />
          <span class="min-w-0 flex-1 truncate font-semibold ink-strong">{{ gv.group.label }}</span>
          <span class="shrink-0 ink-soft num">{{ gv.group.nodes.length }}</span>
        </button>

        <template v-if="gv.open">
          <template v-for="nv in gv.nodes" :key="nv.node.s.id">
            <SessionRow
              :session="nv.node.s"
              :selected="nv.node.s.id === activeId"
              :child-count="nv.node.children.length"
              :child-running="runningCounts.get(nv.node.s.id) ?? 0"
              :open="nv.open"
              @toggle="toggleParent(nv.node.s.id)"
            />
            <div v-if="nv.open" class="ml-4 flex flex-col gap-px">
              <AgentList
                v-if="nv.plain"
                :items="nv.plain.items"
                :more="nv.plain.more"
                :active-id="activeId"
                :running="runningIds"
                @more="showAll(nv.plain.key)"
              />
              <template v-for="r in nv.runs" :key="r.key">
                <button
                  type="button"
                  class="w-full flex items-center gap-2 rounded-md py-1 pl-2 pr-2.5 text-left text-xs ink-muted kbd-ring transition-colors duration-150 surface-hover hover:text-ink"
                  :title="r.name ?? t('common.sidebar.workflowRun')"
                  :aria-expanded="r.open"
                  @click="toggleRun(r.key)"
                >
                  <i class="i-jannchie-chevron-right h-4 w-4 shrink-0 ink-soft transition-transform duration-150" :class="r.open ? 'rotate-90' : ''" aria-hidden="true" />
                  <span class="min-w-0 flex-1 truncate">
                    <span class="ink-soft">{{ t('common.sidebar.workflowRun') }}</span><template v-if="r.name"> · {{ r.name }}</template>
                  </span>
                  <span v-if="runningCounts.get(r.key)" class="h-1.5 w-1.5 shrink-0 animate-pulse rounded-full bg-current ink-success" :title="tn('session.sessionRow.running', runningCounts.get(r.key)!)" />
                  <span class="shrink-0 ink-soft num" :aria-label="tn('session.sessionRow.agents', r.size)">{{ fmtInt(r.size) }}</span>
                </button>
                <div v-if="r.list" class="ml-4 flex flex-col gap-px">
                  <AgentList :items="r.list.items" :more="r.list.more" :active-id="activeId" :running="runningIds" @more="showAll(r.key)" />
                </div>
              </template>
              <button v-if="nv.moreRuns > 0" type="button" class="rail-more" @click="showAll(nv.runsKey)">
                {{ t('common.sidebar.showMoreRuns', { n: nv.moreRuns }) }}
              </button>
            </div>
          </template>
          <button v-if="gv.more > 0" type="button" class="rail-more" @click="showAllNodes(gv.group.key)">
            {{ t('common.sidebar.showMore', { n: gv.more }) }}
          </button>
        </template>
      </section>
    </nav>

    <div class="shrink-0 px-2 pb-2">
      <UpdateBadge />
      <div class="flex items-center gap-1 border-t border-line pt-2 pl-2 text-xs ink-soft">
        <span class="min-w-0 flex-1 truncate num" :title="statusLine?.title" role="status" aria-live="polite">
          <template v-if="filtering">{{ tn('common.sidebar.matched', matched) }}</template>
          <template v-else>{{ statusLine?.text ?? '' }}</template>
        </span>
        <button type="button" class="icon-button-sm" :title="t('common.sidebar.reload')" :aria-label="t('common.sidebar.reload')" @click="loadSessions(true)">
          <AtomSpinner v-if="loading" class="h-3.5 w-3.5" />
          <i v-else class="i-jannchie-refresh h-3.5 w-3.5" aria-hidden="true" />
        </button>
        <button type="button" class="icon-button-sm" :title="t('common.scheme.label', { mode: schemeLabel })" :aria-label="t('common.scheme.label', { mode: schemeLabel })" @click="cycleScheme">
          <i class="h-3.5 w-3.5" :class="schemeIcon" aria-hidden="true" />
        </button>
        <LocaleMenu />
      </div>
    </div>
  </aside>
</template>
