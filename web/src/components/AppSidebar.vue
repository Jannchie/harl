<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { RouterLink, useRoute } from 'vue-router'
import type { SessionInfo } from '../api'
import { groupAgents, isRunning, kindOf, rootOf, sessionTitle, type AgentGroup } from '../lib/agents'
import { fmtBytes, fmtInt, tn } from '../lib/format'
import { projectOf } from '../lib/project'
import { providerDot, providerName } from '../lib/providers'
import { cycleScheme, schemeMode } from '../lib/scheme'
import { bySession, loadSessions, loading, machineName, sessions, sessionsError, status } from '../lib/sessions'
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
/** 根会话下展开的 workflow 运行、点过「再显示」的子 agent 列表；键是 `<根会话 id>|<组键>` */
const openRuns = ref(new Set<string>())
const fullLists = ref(new Set<string>())

/** 「进行中」按 mtime 算，隔一会儿重算一次 */
const now = ref(Date.now())
let ticker: ReturnType<typeof setInterval> | undefined
onMounted(() => (ticker = setInterval(() => (now.value = Date.now()), 30_000)))
onUnmounted(() => clearInterval(ticker))

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
  /** children 按类别 / workflow 运行分的组 */
  agents: AgentGroup[]
}

interface Group {
  key: string
  label: string
  path: string
  nodes: Node[]
}

const byMtime = (a: SessionInfo, b: SessionInfo) => (a.mtime < b.mtime ? 1 : a.mtime > b.mtime ? -1 : 0)

/** 根会话下一组子 agent 的状态键 */
const listKey = (n: Node, g: AgentGroup) => `${n.s.id}|${g.key}`

/** 顶层只放根会话；子 agent 不论隔几层都挂到根会话下面。同一个会话的几份副本（多机器、Claude Desktop 的 ssh 镜像）
 * 只留 bySession 选中的那份；workflow 的 journal 不是对话，不进侧栏。父会话不在列表里的子会话当根会话 */
const tree = computed(() => {
  const pick = bySession.value
  const list = (sessions.value ?? []).filter((s) => kindOf(s) !== 'journal' && pick.get(s.session) === s).sort(byMtime)
  const kids = new Map<string, SessionInfo[]>()
  const top: SessionInfo[] = []
  for (const s of list) {
    const root = rootOf(s, pick)
    if (root === s) top.push(s)
    else kids.set(root.id, [...(kids.get(root.id) ?? []), s])
  }
  return { top, kids }
})

/** 项目 → 根会话 → 子 agent（分组） */
const groups = computed<Group[]>(() => {
  const { top, kids } = tree.value
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
    g.nodes.push({ s, children, agents: groupAgents(children) })
  }
  return [...out.values()]
})

/** 当前会话所在的项目 / 根会话 / 子 agent 组，保证它在侧栏里看得到 */
const activeHome = computed(() => {
  for (const g of groups.value)
    for (const n of g.nodes) {
      if (n.s.id === activeId.value) return { group: g.key, parent: null as string | null, run: null as string | null }
      const ag = n.agents.find((a) => a.items.some((c) => c.id === activeId.value))
      if (ag) return { group: g.key, parent: n.s.id, run: ag.key === 'agents' ? null : listKey(n, ag) }
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

function shown(g: Group): Node[] {
  if (fullGroups.value.has(g.key) || searching.value) return g.nodes
  const head = g.nodes.slice(0, PAGE)
  // 当前会话排在后面时也要露出来
  const active = g.nodes.find((n, i) => i >= PAGE && (n.s.id === activeId.value || n.children.some((c) => c.id === activeId.value)))
  return active ? [...head, active] : head
}

/** 子会话展开：点开过的，或者在搜索（命中可能就在子会话里） */
const childrenOpen = (n: Node) => n.children.length > 0 && (searching.value || openParents.value.has(n.s.id))

const runningIn = (list: SessionInfo[]) => list.reduce((a, s) => a + (isRunning(s, now.value) ? 1 : 0), 0)
/** 普通子 agent 直接列在根会话下面；workflow 运行各自一组，可折叠 */
const plainAgents = (n: Node) => n.agents.find((g) => g.key === 'agents')
const runsOf = (n: Node) => n.agents.filter((g) => g.key !== 'agents')
const runOpen = (n: Node, g: AgentGroup) => searching.value || openRuns.value.has(listKey(n, g))

/** 一串里先给 CHILD_PAGE 个；当前会话排在后面时也露出来 */
function firstOf<T>(key: string, list: T[], isActive: (x: T) => boolean): T[] {
  if (searching.value || fullLists.value.has(key) || list.length <= CHILD_PAGE) return list
  const head = list.slice(0, CHILD_PAGE)
  const active = list.find((x, i) => i >= CHILD_PAGE && isActive(x))
  return active ? [...head, active] : head
}
const shownAgents = (n: Node, g: AgentGroup) => firstOf(listKey(n, g), g.items, (c) => c.id === activeId.value)
const shownRuns = (n: Node) =>
  firstOf(`${n.s.id}|runs`, runsOf(n), (g) => g.items.some((c) => c.id === activeId.value))
function showAll(key: string) {
  fullLists.value = new Set(fullLists.value).add(key)
}
/** workflow 运行的组名：转述的用户请求；没有就退回第一个 agent 的显示名 */
const runName = (g: AgentGroup) => g.title ?? (g.items[0] && sessionTitle(g.items[0])) ?? null

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

      <section v-for="(g, gi) in groups" :key="g.key" class="flex flex-col gap-px">
        <button
          type="button"
          class="w-full flex items-center gap-2 rounded-md py-1 pl-2 pr-2.5 text-left text-xs kbd-ring transition-colors duration-150 surface-hover"
          :title="g.path || t('common.sidebar.noCwd')"
          :aria-expanded="isOpen(g.key, gi)"
          @click="toggleGroup(g.key, gi)"
        >
          <i
            class="i-jannchie-chevron-right h-4 w-4 shrink-0 ink-soft transition-transform duration-150"
            :class="isOpen(g.key, gi) ? 'rotate-90' : ''"
            aria-hidden="true"
          />
          <span class="min-w-0 flex-1 truncate font-semibold ink-strong">{{ g.label }}</span>
          <span class="shrink-0 ink-soft num">{{ g.nodes.length }}</span>
        </button>

        <template v-if="isOpen(g.key, gi)">
          <template v-for="n in shown(g)" :key="n.s.id">
            <SessionRow
              :session="n.s"
              :selected="n.s.id === activeId"
              :child-count="n.children.length"
              :child-running="runningIn(n.children)"
              :open="childrenOpen(n)"
              @toggle="toggleParent(n.s.id)"
            />
            <div v-if="childrenOpen(n)" class="ml-4 flex flex-col gap-px">
              <template v-if="plainAgents(n)">
                <SessionRow
                  v-for="c in shownAgents(n, plainAgents(n)!)"
                  :key="c.id"
                  :session="c"
                  :selected="c.id === activeId"
                  :running="isRunning(c, now)"
                  child
                />
                <button
                  v-if="shownAgents(n, plainAgents(n)!).length < plainAgents(n)!.items.length"
                  type="button"
                  class="w-full rounded-md py-1 pl-8 text-left text-xs ink-soft surface-hover kbd-ring hover:text-ink"
                  @click="showAll(listKey(n, plainAgents(n)!))"
                >
                  {{ t('common.sidebar.showMore', { n: plainAgents(n)!.items.length - shownAgents(n, plainAgents(n)!).length }) }}
                </button>
              </template>
              <template v-for="g in shownRuns(n)" :key="g.key">
                <button
                  type="button"
                  class="w-full flex items-center gap-2 rounded-md py-1 pl-2 pr-2.5 text-left text-xs ink-muted kbd-ring transition-colors duration-150 surface-hover hover:text-ink"
                  :title="runName(g) ?? t('common.sidebar.workflowRun')"
                  :aria-expanded="runOpen(n, g)"
                  @click="toggleRun(listKey(n, g))"
                >
                  <i class="i-jannchie-chevron-right h-4 w-4 shrink-0 ink-soft transition-transform duration-150" :class="runOpen(n, g) ? 'rotate-90' : ''" aria-hidden="true" />
                  <span class="min-w-0 flex-1 truncate">
                    <span class="ink-soft">{{ t('common.sidebar.workflowRun') }}</span><template v-if="runName(g)"> · {{ runName(g) }}</template>
                  </span>
                  <span v-if="runningIn(g.items)" class="h-1.5 w-1.5 shrink-0 animate-pulse rounded-full bg-current ink-success" :title="tn('session.sessionRow.running', runningIn(g.items))" />
                  <span class="shrink-0 ink-soft num" :aria-label="tn('session.sessionRow.agents', g.items.length)">{{ fmtInt(g.items.length) }}</span>
                </button>
                <div v-if="runOpen(n, g)" class="ml-4 flex flex-col gap-px">
                  <SessionRow
                    v-for="c in shownAgents(n, g)"
                    :key="c.id"
                    :session="c"
                    :selected="c.id === activeId"
                    :running="isRunning(c, now)"
                    child
                  />
                  <button
                    v-if="shownAgents(n, g).length < g.items.length"
                    type="button"
                    class="w-full rounded-md py-1 pl-8 text-left text-xs ink-soft surface-hover kbd-ring hover:text-ink"
                    @click="showAll(listKey(n, g))"
                  >
                    {{ t('common.sidebar.showMore', { n: g.items.length - shownAgents(n, g).length }) }}
                  </button>
                </div>
              </template>
              <button
                v-if="shownRuns(n).length < runsOf(n).length"
                type="button"
                class="w-full rounded-md py-1 pl-8 text-left text-xs ink-soft surface-hover kbd-ring hover:text-ink"
                @click="showAll(`${n.s.id}|runs`)"
              >
                {{ t('common.sidebar.showMoreRuns', { n: runsOf(n).length - shownRuns(n).length }) }}
              </button>
            </div>
          </template>
          <button
            v-if="shown(g).length < g.nodes.length"
            type="button"
            class="w-full rounded-md py-1 pl-8 text-left text-xs ink-soft surface-hover kbd-ring hover:text-ink"
            @click="fullGroups = new Set(fullGroups).add(g.key)"
          >
            {{ t('common.sidebar.showMore', { n: g.nodes.length - shown(g).length }) }}
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
