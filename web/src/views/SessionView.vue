<script setup lang="ts">
import { computed, defineAsyncComponent, markRaw, nextTick, onMounted, onUnmounted, provide, ref, shallowRef, toRef, watch } from 'vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { api, type Event } from '../api'
import { sessionTitle, usePageTitle } from '../lib/title'
import { SNAPSHOT_ID } from '../lib/elided'
import { groupEvents, type Grouped } from '../lib/group'
import { fmtBytes, fmtDuration, tn, truncate } from '../lib/format'
import { projectOf } from '../lib/project'
import { bySession, describe, loadSessions, machineName, sessions, sessionsState } from '../lib/sessions'
import EventRow from '../components/EventRow.vue'
import ItemView from '../components/ItemView.vue'
import ProviderTag from '../components/ProviderTag.vue'
import TimeStamp from '../components/TimeStamp.vue'
import TurnBlock from '../components/TurnBlock.vue'
import AtomSpinner from '../components/AtomSpinner.vue'
import Segmented from '../components/Segmented.vue'

// 上下文视图带 ECharts，单独切 chunk：只看对话不用下它
const ContextView = defineAsyncComponent(() => import('../components/ctx/ContextView.vue'))

const props = defineProps<{ id: string }>()
// JsonBlock 按它取 slim 事件里省略掉的 base64
provide(SNAPSHOT_ID, toRef(props, 'id'))
const route = useRoute()
const router = useRouter()
const { t } = useI18n()

const events = shallowRef<Event[] | null>(null)
const error = ref<string | null>(null)

onMounted(() => loadSessions())

const info = computed(() => sessions.value?.find((s) => s.id === props.id) ?? null)
const parent = computed(() => (info.value?.parent_session ? (bySession.value.get(info.value.parent_session) ?? null) : null))
const children = computed(() =>
  info.value ? (sessions.value ?? []).filter((s) => s.parent_session === info.value!.session && s.id !== props.id) : [],
)

// 路由 id 是快照哈希，每次 capture 都会换：列表是 capture 之前拉的就找不到当前 id。
// 这种时候强制重拉一次；每个 id 只拉一次，真不在列表里（比如旧快照）也不会来回刷
let refetchedFor: string | null = null
watch(
  [() => props.id, () => sessionsState.value.kind, info],
  ([id, kind, i]) => {
    if (kind !== 'ready' || i || refetchedFor === id) return
    refetchedFor = id
    void loadSessions(true)
  },
  { immediate: true },
)

let ac: AbortController | null = null

async function load(id: string) {
  ac?.abort()
  ac = new AbortController()
  const signal = ac.signal
  events.value = null
  error.value = null
  try {
    const evs = await api.events(id, signal)
    // JSON.parse 刚占了一个任务；分组、渲染另起一个，中间让浏览器喘口气
    await new Promise((r) => setTimeout(r))
    if (signal.aborted) return
    // 几万个事件、几十 MB：不要让 Vue 去代理它
    events.value = markRaw(evs)
    // 从别处链过来带着 #turn-…：内容这时才渲染出来，路由那边滚不到，下面 revealHash 补一次
  } catch (e) {
    if (!signal.aborted) error.value = describe(e)
  }
}

watch(() => props.id, load, { immediate: true })
onUnmounted(() => ac?.abort())

const VIEW_OPTIONS = computed(() => [
  { value: 'chat' as const, label: t('session.sessionView.viewChat'), icon: 'i-jannchie-chat', compact: true },
  { value: 'context' as const, label: t('ctx.view'), icon: 'i-jannchie-layers', compact: true },
  { value: 'events' as const, label: t('session.sessionView.events'), icon: 'i-jannchie-list', compact: true },
])

type View = 'chat' | 'context' | 'events'

/** 对话 / 上下文 / 事件，记在 query 里，刷新和分享都保留 */
const view = computed({
  get: (): View => (route.query.view === 'events' || route.query.view === 'context' ? route.query.view : 'chat'),
  set: (v: View) => {
    const q = { ...route.query }
    if (v !== 'chat') q.view = v
    else delete q.view
    // 「在上下文视图里看这一轮」带的定位只用一次：自己切视图就不再带着
    delete q.ctxTurn
    void router.replace({ query: q })
  },
})

/** 从对话里某一轮跳过来时带的 turn id：上下文视图选中那一轮的最后一次请求 */
const ctxTurn = computed(() => (typeof route.query.ctxTurn === 'string' ? route.query.ctxTurn : null))

const grouped = computed<Grouped | null>(() => (events.value ? groupEvents(events.value) : null))

// ---- 分批渲染 ----
// 大会话一口气渲染完（几千段 Markdown）要好几秒，主线程一直卡着。对话视图先渲染开头几块，
// 其余每个任务补一批，批大小按上一批实际花的时间调到大约 FILL_BUDGET_MS；批与批之间浏览器照常绘制、响应输入。
// 渲染完的 DOM 和一次渲染出来的一样

const FILL_BUDGET_MS = 24

/** 对话视图要渲染的块数 */
const fillTotal = computed(() => (view.value === 'chat' ? (grouped.value?.blocks.length ?? 0) : 0))
/** 已经渲染的块数 */
const filled = ref(0)
const shownBlocks = computed(() => grouped.value?.blocks.slice(0, filled.value) ?? [])

let fillGen = 0
function startFill() {
  const gen = ++fillGen
  filled.value = 0
  let batch = 2
  const step = async () => {
    if (gen !== fillGen || filled.value >= fillTotal.value) return
    const t0 = performance.now()
    filled.value = Math.min(fillTotal.value, filled.value + batch)
    await nextTick()
    if (gen !== fillGen) return
    // 读一次布局，把样式、排版也算进这一批的花费，不然批会越调越大、卡在之后的绘制上
    void document.body.offsetHeight
    const dt = performance.now() - t0
    batch = Math.max(1, Math.round(batch * Math.min(4, FILL_BUDGET_MS / Math.max(dt, 1))))
    setTimeout(step)
  }
  void step()
}
watch([grouped, view], startFill, { immediate: true })
onUnmounted(() => fillGen++)

// ---- 事件视图：按组懒挂载 ----
// 几万行全挂上去（每行一个组件）光堆就几百 MB，之后每次 GC 都要停一两秒。按 EVENT_CHUNK 行分组，
// 组滚到视口附近才挂行，没挂的组用估计的高度占位；挂过的留着（展开的行、滚回来都不丢）

const EVENT_CHUNK = 100
/** 一行（含间隙）大约的高度，占位用 */
const EVENT_ROW_PX = 25

const eventChunks = computed(() => {
  const evs = events.value ?? []
  const out: { start: number; rows: Event[] }[] = []
  for (let i = 0; i < evs.length; i += EVENT_CHUNK) out.push({ start: i, rows: evs.slice(i, i + EVENT_CHUNK) })
  return out
})
const liveChunks = shallowRef(new Set<number>())
watch(events, () => (liveChunks.value = new Set()))

let chunkIo: IntersectionObserver | null = null
function chunkObserver(): IntersectionObserver {
  chunkIo ??= new IntersectionObserver(
    (entries) => {
      const hit = entries.filter((e) => e.isIntersecting)
      if (!hit.length) return
      // 挂上就一直挂着，不用再看
      for (const e of hit) chunkIo?.unobserve(e.target)
      const add = hit.map((e) => Number((e.target as HTMLElement).dataset.chunk)).filter((i) => !liveChunks.value.has(i))
      if (!add.length) return
      const next = new Set(liveChunks.value)
      for (const i of add) next.add(i)
      liveChunks.value = next
    },
    // 滚动的是 <main>；提前一屏多挂上
    { root: document.querySelector('[data-scroll-root]'), rootMargin: '1500px 0px' },
  )
  return chunkIo
}
function observeChunk(el: unknown) {
  if (el instanceof HTMLElement && !liveChunks.value.has(Number(el.dataset.chunk))) chunkObserver().observe(el)
}
onUnmounted(() => chunkIo?.disconnect())

/** 定位到 hash 指的元素：还没渲染到就先把它之前的都渲染出来 */
async function revealHash(hash: string) {
  if (!hash || !grouped.value) return
  const id = decodeURIComponent(hash.slice(1))
  if (!document.getElementById(id) && filled.value < fillTotal.value) {
    const blocks = grouped.value.blocks
    const at = view.value === 'chat' && id.startsWith('turn-') ? blocks.findIndex((b) => b.type === 'turn' && `turn-${b.turn.key}` === id) : -1
    filled.value = at >= 0 ? Math.max(filled.value, at + 1) : fillTotal.value
    await nextTick()
  }
  document.getElementById(id)?.scrollIntoView({ block: 'start' })
}
watch(
  () => [route.hash, grouped.value] as const,
  ([hash, g]) => {
    if (g) void revealHash(hash)
  },
  { flush: 'post' },
)

const stats = computed(() => {
  const evs = events.value ?? []
  let turns = 0
  let steps = 0
  let tools = 0
  for (const b of grouped.value?.blocks ?? [])
    if (b.type === 'turn') {
      turns++
      steps += b.turn.stepCount
      tools += b.turn.toolCount
    }
  const first = evs.find((e) => e.ts)?.ts
  const last = [...evs].reverse().find((e) => e.ts)?.ts
  return { events: evs.length, turns, steps, tools, first, last, span: fmtDuration(first, last) }
})

const outline = computed(() =>
  (grouped.value?.blocks ?? []).flatMap((b) =>
    b.type === 'turn'
      ? [{ key: b.turn.key, n: b.turn.n, text: truncate((b.turn.userText ?? '').replace(/\s+/g, ' ').trim() || t('session.sessionView.noText'), 48) }]
      : [],
  ),
)

const title = computed(() => (info.value && sessionTitle(info.value)) ?? info.value?.session ?? events.value?.[0]?.session ?? props.id)
/** 显示名是 agent 的 label 时，鼠标停上去看完整的第一句提示 */
const fullTitle = computed(() => info.value?.title ?? title.value)
usePageTitle(() => title.value)
const provider = computed(() => info.value?.provider ?? events.value?.[0]?.provider ?? null)
const project = computed(() => (info.value ? projectOf(info.value) : null))
const metaOpen = ref(false)
</script>

<template>
  <!-- 头部高度 52px 就是展开行钉住时要让出的位置；折叠组里再往下让一行（TurnBlock 的 .steps-body） -->
  <div class="min-h-full" style="--pin-base: 52px; --pin-top: var(--pin-base)">
    <header class="sticky top-0 z-10 page-ground">
      <div class="h-13 flex items-center gap-2 pl-12 pr-3 md:pl-6 md:pr-6">
        <template v-if="parent || info?.parent_session">
          <RouterLink
            v-if="parent"
            :to="{ name: 'session', params: { id: parent.id } }"
            class="flex shrink-0 items-center gap-1 rounded-lg px-2 py-1 text-xs ink-muted surface surface-hover kbd-ring"
            :title="sessionTitle(parent) ?? parent.session"
          >
            <i class="i-jannchie-arrow-left h-3.5 w-3.5" />
            {{ t('session.sessionView.parent') }}
          </RouterLink>
          <span v-else class="shrink-0 text-xs ink-soft" :title="t('session.sessionView.parentMissing')">{{ t('session.sessionView.parentMissing') }}</span>
          <i class="i-jannchie-chevron-right h-3.5 w-3.5 shrink-0 ink-muted" />
          <i class="i-jannchie-robot h-3.5 w-3.5 shrink-0 ink-accent" />
        </template>
        <h1 class="m-0 min-w-0 flex-1 truncate text-sm font-medium" :title="fullTitle">{{ title }}</h1>

        <Segmented
          v-model="view"
          :label="t('session.sessionView.viewLabel')"
          class="shrink-0"
          :options="VIEW_OPTIONS"
        />
        <button
          type="button"
          class="icon-button shrink-0"
          :class="metaOpen ? 'surface-strong' : ''"
          :title="t('session.sessionView.info')"
          :aria-label="t('session.sessionView.info')"
          :aria-expanded="metaOpen"
          @click="metaOpen = !metaOpen"
        >
          <i class="i-jannchie-info h-4 w-4" />
        </button>
        <a :href="api.rawUrl(id)" target="_blank" rel="noopener" class="icon-button shrink-0" :title="t('session.sessionView.rawJsonl')" :aria-label="t('session.sessionView.rawJsonl')">
          <i class="i-jannchie-file-text h-4 w-4" />
        </a>
      </div>
    </header>

    <!-- 上下文视图是一屏卡片，不要两侧留给目录的栏 -->
    <div class="grid grid-cols-[minmax(0,1fr)] px-4 md:px-6" :class="view === 'context' ? '' : '2xl:grid-cols-[minmax(0,1fr)_minmax(0,760px)_minmax(0,1fr)]'">
      <div v-if="view !== 'context'" class="max-2xl:hidden" />

      <div class="mx-auto w-full min-w-0 pb-24" :class="view === 'events' ? 'max-w-1000px 2xl:max-w-none' : view === 'context' ? 'max-w-1280px' : 'max-w-chat'">
        <!-- 会话概况：一行最要紧的，其余点「信息」再看 -->
        <div class="flex flex-wrap items-center gap-x-3 gap-y-1 pb-2 pt-1 text-xs ink-soft">
          <ProviderTag v-if="provider" :provider="provider" class="ink-muted" />
          <span v-if="project?.path" class="inline-flex min-w-0 items-center gap-1" :title="project.path">
            <i class="i-jannchie-folder h-3.5 w-3.5" />{{ project.label }}
          </span>
          <TimeStamp v-if="stats.first" :ts="stats.first" mode="full" />
          <span v-if="stats.span" class="num">{{ stats.span }}</span>
          <span v-if="events" class="num">{{ tn('common.metric.turnsN', stats.turns) }} · {{ tn('common.metric.stepsN', stats.steps) }} · {{ tn('common.metric.toolCallsN', stats.tools) }}</span>
        </div>

        <div v-if="metaOpen" class="mb-3 grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-1.5 rounded-2xl px-4 py-3 text-xs surface">
          <template v-if="info">
            <span class="ink-soft">{{ t('common.metric.session') }}</span><span class="font-mono break-all">{{ info.session }}</span>
            <span class="ink-soft">{{ t('session.sessionView.source') }}</span><span class="font-mono break-all">{{ info.source }}</span>
            <template v-if="project?.path"><span class="ink-soft">{{ t('session.sessionView.cwd') }}</span><span class="font-mono break-all">{{ project.path }}</span></template>
            <span class="ink-soft">{{ t('session.sessionView.machine') }}</span><span class="num" :title="info.machine">{{ machineName(info.machine) }}</span>
            <span class="ink-soft">{{ t('session.sessionView.size') }}</span><span class="num">{{ tn('session.sessionView.lines', info.lines) }} · {{ fmtBytes(info.size) }}</span>
            <span class="ink-soft">{{ t('session.sessionView.modified') }}</span><TimeStamp :ts="info.mtime" mode="full" />
            <span class="ink-soft">{{ t('session.sessionView.captured') }}</span><TimeStamp :ts="info.captured_at" mode="full" />
          </template>
          <span class="ink-soft">{{ t('session.sessionView.snapshot') }}</span><span class="font-mono break-all">{{ id }}</span>
          <template v-if="events">
            <span class="ink-soft">{{ t('session.sessionView.events') }}</span>
            <span class="num">{{ tn('session.sessionView.eventCount', stats.events) }}<template v-if="grouped?.hidden">{{ tn('session.sessionView.eventsHidden', grouped.hidden) }}</template></span>
          </template>
          <template v-if="children.length">
            <span class="ink-soft">{{ t('session.sessionView.children') }}</span>
            <span class="flex flex-col gap-0.5">
              <RouterLink
                v-for="c in children"
                :key="c.id"
                :to="{ name: 'session', params: { id: c.id } }"
                class="inline-flex items-center gap-1 ink-accent hover:underline"
              >
                <i class="i-jannchie-robot h-3.5 w-3.5 shrink-0" /><span class="truncate">{{ sessionTitle(c) ?? c.session }}</span>
              </RouterLink>
            </span>
          </template>
        </div>

        <p v-if="error" class="py-12 text-center text-sm ink-danger">{{ t('common.loadFailed', { error }) }}</p>
        <p v-else-if="!events" class="flex items-center justify-center gap-2 py-12 text-sm ink-soft">
          <AtomSpinner class="h-4 w-4" /> {{ t('session.sessionView.loading') }}
        </p>
        <p v-else-if="events.length === 0" class="py-12 text-center text-sm ink-soft">{{ t('session.sessionView.empty') }}</p>

        <ContextView v-else-if="view === 'context'" :events="events" :info="info" :turn="ctxTurn" />

        <div v-else-if="view === 'events'" class="flex flex-col gap-px pt-2">
          <div
            v-for="ch in eventChunks"
            :key="ch.start"
            :ref="observeChunk"
            v-memo="[ch, liveChunks.has(ch.start)]"
            :data-chunk="ch.start"
            class="event-chunk flex flex-col gap-px"
            :style="liveChunks.has(ch.start) ? undefined : { height: `${ch.rows.length * EVENT_ROW_PX}px` }"
          >
            <template v-if="liveChunks.has(ch.start)">
              <EventRow v-for="e in ch.rows" :key="e.seq" :ev="e" />
            </template>
          </div>
        </div>

        <template v-else-if="grouped">
          <template v-for="b in shownBlocks" :key="b.type === 'turn' ? b.turn.key : `loose:${b.firstSeq}`">
            <TurnBlock v-if="b.type === 'turn'" :turn="b.turn" />
            <div v-else class="flex flex-col gap-1 py-2">
              <ItemView v-for="it in b.items" :key="it.ev.seq" :item="it" />
            </div>
          </template>
        </template>
      </div>

      <!-- 轮次目录：只在够宽的屏幕上出现，跟着滚动 -->
      <nav v-if="view === 'chat' && outline.length > 1" :aria-label="t('session.sessionView.outline')" class="max-2xl:hidden sticky top-16 self-start pl-8">
        <div class="max-h-[calc(100dvh-5rem)] max-w-64 flex flex-col gap-px overflow-y-auto text-xs">
          <a
            v-for="o in outline"
            :key="o.key"
            :href="`#turn-${o.key}`"
            class="flex gap-2 rounded-md px-2 py-1 ink-soft surface-hover hover:ink-strong kbd-ring"
            :title="o.text"
          >
            <span class="w-6 shrink-0 text-right num ink-soft">{{ o.n ?? '?' }}</span>
            <span class="min-w-0 truncate">{{ o.text }}</span>
          </a>
        </div>
      </nav>
    </div>
  </div>
</template>

<style scoped>
/* 挂上了但在屏幕外的组跳过排版和绘制 */
.event-chunk {
  content-visibility: auto;
  contain-intrinsic-size: auto 2500px;
}
</style>
