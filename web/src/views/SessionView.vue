<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, shallowRef, watch } from 'vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { api, type Event } from '../api'
import { usePageTitle } from '../lib/title'
import { groupEvents, type Grouped } from '../lib/group'
import { fmtBytes, fmtDuration, tn, truncate } from '../lib/format'
import { projectOf } from '../lib/project'
import { bySession, describe, loadSessions, machineName, sessions } from '../lib/sessions'
import EventRow from '../components/EventRow.vue'
import ItemView from '../components/ItemView.vue'
import ProviderTag from '../components/ProviderTag.vue'
import TimeStamp from '../components/TimeStamp.vue'
import TurnBlock from '../components/TurnBlock.vue'
import AtomSpinner from '../components/AtomSpinner.vue'
import Segmented from '../components/Segmented.vue'

const props = defineProps<{ id: string }>()
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

let ac: AbortController | null = null

async function load(id: string) {
  ac?.abort()
  ac = new AbortController()
  const signal = ac.signal
  events.value = null
  error.value = null
  try {
    const evs = await api.events(id, signal)
    if (signal.aborted) return
    events.value = evs
    // 从别处链过来带着 #turn-…：内容这时才渲染出来，路由那边滚不到，这里补一次
    if (route.hash) {
      await nextTick()
      document.getElementById(decodeURIComponent(route.hash.slice(1)))?.scrollIntoView({ block: 'start' })
    }
  } catch (e) {
    if (!signal.aborted) error.value = describe(e)
  }
}

watch(() => props.id, load, { immediate: true })
onUnmounted(() => ac?.abort())

const VIEW_OPTIONS = computed(() => [
  { value: 'chat' as const, label: t('session.sessionView.viewChat'), icon: 'i-jannchie-chat', compact: true },
  { value: 'events' as const, label: t('session.sessionView.events'), icon: 'i-jannchie-list', compact: true },
])

/** 对话 / 事件，记在 query 里，刷新和分享都保留 */
const view = computed({
  get: () => (route.query.view === 'events' ? 'events' : 'chat'),
  set: (v: 'chat' | 'events') => {
    const q = { ...route.query }
    if (v === 'events') q.view = 'events'
    else delete q.view
    void router.replace({ query: q })
  },
})

const grouped = computed<Grouped | null>(() => (events.value ? groupEvents(events.value) : null))

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

const title = computed(() => info.value?.title ?? info.value?.session ?? events.value?.[0]?.session ?? props.id)
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
            :title="parent.title ?? parent.session"
          >
            <i class="i-jannchie-arrow-left h-3.5 w-3.5" />
            {{ t('session.sessionView.parent') }}
          </RouterLink>
          <span v-else class="shrink-0 text-xs ink-soft" :title="t('session.sessionView.parentMissing')">{{ t('session.sessionView.parentMissing') }}</span>
          <i class="i-jannchie-chevron-right h-3.5 w-3.5 shrink-0 ink-muted" />
          <i class="i-jannchie-robot h-3.5 w-3.5 shrink-0 ink-accent" />
        </template>
        <h1 class="m-0 min-w-0 flex-1 truncate text-sm font-medium" :title="title">{{ title }}</h1>

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

    <div class="grid grid-cols-[minmax(0,1fr)] px-4 md:px-6 2xl:grid-cols-[minmax(0,1fr)_minmax(0,760px)_minmax(0,1fr)]">
      <div class="max-2xl:hidden" />

      <div class="mx-auto w-full min-w-0 pb-24" :class="view === 'events' ? 'max-w-1000px 2xl:max-w-none' : 'max-w-chat'">
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
                <i class="i-jannchie-robot h-3.5 w-3.5 shrink-0" /><span class="truncate">{{ c.title ?? c.session }}</span>
              </RouterLink>
            </span>
          </template>
        </div>

        <p v-if="error" class="py-12 text-center text-sm ink-danger">{{ t('common.loadFailed', { error }) }}</p>
        <p v-else-if="!events" class="flex items-center justify-center gap-2 py-12 text-sm ink-soft">
          <AtomSpinner class="h-4 w-4" /> {{ t('session.sessionView.loading') }}
        </p>
        <p v-else-if="events.length === 0" class="py-12 text-center text-sm ink-soft">{{ t('session.sessionView.empty') }}</p>

        <div v-else-if="view === 'events'" class="flex flex-col gap-px pt-2">
          <EventRow v-for="e in events" :key="e.seq" :ev="e" />
        </div>

        <template v-else-if="grouped">
          <template v-for="b in grouped.blocks" :key="b.type === 'turn' ? b.turn.key : `loose:${b.firstSeq}`">
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
