<script setup lang="ts">
// Agent 网络：父会话链 + 当前会话 + 所有子 agent（递归）。
// 父子关系来自会话头里的 parent_session；各卡的数字是把那个会话的事件也折一遍算的，
// 协作记录（消息、回报、委派、团队任务）是把家族里每个会话的事件扫一遍抽出来的，见 lib/fleet.ts。
// 选中一个 agent，时间轴、任务、事件检查一起收窄到它。
import { computed, onUnmounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import type { Event, SessionInfo } from '../../api'
import type { CtxFold } from '../../lib/context'
import { fleetCache, fleetStats, type CtxSummary, type FamilyStats, type FleetSlot } from '../../lib/ctxView'
import { buildFleet, cardKey, collabCache, familyCards, loadFamily, type FleetAgent } from '../../lib/fleet'
import { runFleetJob } from '../../lib/fleetWorker'
import { fmtInt } from '../../lib/format'
import { useNow } from '../../lib/now'
import { bySession } from '../../lib/sessions'
import DashCard from '../dash/DashCard.vue'
import AtomSpinner from '../AtomSpinner.vue'
import FleetGraph from './fleet/FleetGraph.vue'
import FleetInspector from './fleet/FleetInspector.vue'
import FleetTasks from './fleet/FleetTasks.vue'
import FleetTimeline from './fleet/FleetTimeline.vue'

const props = defineProps<{
  info: SessionInfo
  fold: CtxFold
  summary: CtxSummary
  /** 当前会话的事件；给了就不再为协作记录重拉一遍 */
  events?: Event[] | null
}>()
const emit = defineEmits<{ family: [cost: number | null]; familyStats: [stats: FamilyStats | null] }>()
const { t } = useI18n()

const cards = computed(() => familyCards(props.info, bySession.value))

let ac: AbortController | null = null
/** 这次挂载里已经重试过的 error 卡，免得每次 cards 变化都再拉一遍 */
const retried = new Set<string>()

watch(
  cards,
  (list) => {
    if (list.length <= 1) return
    ac?.abort()
    ac = new AbortController()
    void loadFamily(list, { signal: ac.signal, retried, currentEvents: props.events, offload: runFleetJob })
  },
  { immediate: true },
)
onUnmounted(() => ac?.abort())

const current = computed(() => fleetStats(props.fold, props.summary))

/** session id → 卡片数字；当前会话直接用 ContextView 折好的 */
const slots = computed(() => {
  const m = new Map<string, FleetSlot | undefined>()
  for (const c of cards.value) m.set(c.info.session, c.role === 'current' ? current.value : fleetCache.get(cardKey(c.info)))
  return m
})

/** 当前会话和它的子孙一起的费用 */
const familyCost = computed(() => {
  const fam = cards.value.filter((c) => c.role !== 'parent').map((c) => slots.value.get(c.info.session))
  if (fam.length < 2 || fam.some((s) => typeof s !== 'object')) return null
  return fam.reduce((a, s) => a + (s as Exclude<FleetSlot, string>).cost, 0)
})
watch(familyCost, (v) => emit('family', v), { immediate: true })

/** 子孙 agent 的合计（不含自己），给概况卡的 agent 家族一格；还在拉的先不给，拉不到的跳过并记成 partial */
const familyStats = computed<FamilyStats | null>(() => {
  const kids = cards.value.filter((c) => c.role === 'child').map((c) => slots.value.get(c.info.session))
  if (!kids.length || kids.some((s) => s === 'loading')) return null
  const out = { agents: kids.length, billed: 0, cost: 0, partial: 0 }
  for (const s of kids) {
    if (typeof s !== 'object') {
      out.partial++
      continue
    }
    out.billed += s.billed
    out.cost += s.cost
  }
  return out.partial < kids.length ? out : null
})
watch(familyStats, (v) => emit('familyStats', v), { immediate: true })
onUnmounted(() => emit('familyStats', null))

// 「进行中」按 mtime 算，每半分钟刷新一次
const now = useNow(30_000)

const model = computed(() => {
  const m = buildFleet(cards.value, (c) => collabCache.get(cardKey(c.info)), now.value)
  // 当前会话的窗口用 ContextView 折出来的（dsh 的 request/context）
  const cur = m.agents.find((a) => a.role === 'current')
  if (cur && cur.window == null) cur.window = props.fold.window
  return m
})

const selected = ref<string | null>(null)
const task = ref<string | null>(null)
const picked = ref<string | null>(null)
const selectedAgent = computed<FleetAgent | null>(() => (selected.value ? (model.value.byId.get(selected.value) ?? null) : null))
// 换了会话，筛选清掉
watch(
  () => props.info.session,
  () => {
    selected.value = null
    task.value = null
    picked.value = null
  },
)

const stats = computed(() => {
  const m = model.value
  const work = m.tasks.filter((x) => x.status !== 'deleted').length
  return [
    { key: 'agents', n: m.agents.length },
    { key: 'running', n: m.agents.filter((a) => a.running).length },
    { key: 'work', n: work },
    { key: 'messages', n: m.records.filter((r) => r.kind !== 'delegation').length },
  ]
})
const hasCollab = computed(() => model.value.records.length > 0 || model.value.tasks.length > 0 || model.value.pending > 0)
</script>

<template>
  <DashCard v-if="cards.length > 1" icon="i-jannchie-network" :title="t('ctx.fleet.title')" :caption="t('ctx.fleet.caption')" :title-hint="t('ctxFleet.summary.coverage')">
    <div class="flex flex-col gap-4">
      <!-- 概要 -->
      <div class="flex flex-wrap items-center gap-2 text-xs">
        <span v-for="s in stats" :key="s.key" class="inline-flex items-baseline gap-1.5 rounded-lg px-2.5 py-1 surface">
          <span class="text-sm font-medium ink-strong num">{{ fmtInt(s.n) }}</span>
          <span class="ink-soft">{{ t(`ctxFleet.summary.${s.key}`) }}</span>
        </span>
        <span v-if="model.pending" class="inline-flex items-center gap-1.5 ink-soft">
          <AtomSpinner class="h-3 w-3" />{{ t('ctxFleet.summary.loading', { n: fmtInt(model.pending) }) }}
        </span>
        <span class="flex-1" />
        <button v-if="selectedAgent" type="button" class="segment border-0 pill-selected" :title="t('ctxFleet.summary.clear')" @click="selected = null">
          {{ t('ctxFleet.summary.filtered', { name: selectedAgent.name }) }}<i class="i-jannchie-x h-3 w-3" aria-hidden="true" />
        </button>
      </div>

      <FleetGraph :agents="model.agents" :slots="slots" :selected="selected" @select="(id) => (selected = id)" />

      <template v-if="hasCollab">
        <FleetTimeline
          :model="model"
          :selected="selected"
          :task="task"
          :picked="picked"
          @pick="(k) => (picked = k)"
          @task="(k) => (task = k)"
          @select="(id) => (selected = id)"
        />
        <FleetTasks :model="model" :selected="selected" :task="task" @task="(k) => (task = k)" @select="(id) => (selected = id)" />
        <FleetInspector :model="model" :selected="selected" :picked="picked" @pick="(k) => (picked = k)" @select="(id) => (selected = id)" />
      </template>
    </div>
  </DashCard>
</template>
