<script setup lang="ts">
// 跨 agent 协作时间轴：每个 agent 一条泳道，消息、回报、委派、任务标记画在同一条时间轴上。
// 记录画在发送方的泳道上，竖线连到接收方；没有记录的泳道算安静，默认收起。
// 点任务标记按任务筛选；点记录在事件检查里打开。
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useChartTheme } from '../../../lib/chartTheme'
import { lineageColor, touches, type FleetModel, type FleetRecord, type TaskMark } from '../../../lib/fleet'
import { fmtDateTime, fmtInt } from '../../../lib/format'
import { usePref } from '../../../lib/ctxView'

const props = defineProps<{
  model: FleetModel
  selected: string | null
  task: string | null
  picked: string | null
}>()
const emit = defineEmits<{ pick: [key: string | null]; task: [key: string | null]; select: [id: string | null] }>()
const { t } = useI18n()
const theme = useChartTheme()

const query = ref('')
const showQuiet = usePref<boolean>('fleet.showQuiet', false)
const LANE_H = 28
/** 画太多节点会拖慢页面：只画最近的这么多 */
const CAP = 2000

const nameOf = (id: string | null, raw?: string | null) => (id ? (props.model.byId.get(id)?.name ?? id) : (raw ?? t('ctxFleet.inspector.unknown')))

const taskOf = computed(() => (props.task ? props.model.tasks.find((x) => x.key === props.task) ?? null : null))

const filtered = computed(() => {
  const q = query.value.trim().toLowerCase()
  return props.model.records.filter((r) => {
    if (props.selected && !touches(r, props.selected)) return false
    if (props.task && r.task !== props.task) return false
    if (q) {
      const hay = `${r.text} ${r.summary ?? ''} ${nameOf(r.fromId, r.fromName ?? r.from)} ${nameOf(r.toId, r.to)}`.toLowerCase()
      if (!hay.includes(q)) return false
    }
    return true
  })
})
const shown = computed(() => (filtered.value.length > CAP ? filtered.value.slice(-CAP) : filtered.value))

const marks = computed<TaskMark[]>(() => {
  if (query.value.trim()) return []
  return props.model.marks.filter((m) => (!props.task || m.task === props.task) && (!props.selected || m.lane === props.selected))
})

const busy = computed(() => {
  const s = new Set<string>()
  for (const r of shown.value) {
    s.add(r.lane)
    if (r.toId) s.add(r.toId)
  }
  for (const m of marks.value) s.add(m.lane)
  return s
})
const quiet = computed(() => props.model.agents.filter((a) => !busy.value.has(a.id)))
const lanes = computed(() => props.model.agents.filter((a) => showQuiet.value || busy.value.has(a.id)))
const laneY = computed(() => new Map(lanes.value.map((a, i) => [a.id, i * LANE_H + LANE_H / 2])))
const laneCount = computed(() => {
  const m = new Map<string, number>()
  for (const r of shown.value) m.set(r.lane, (m.get(r.lane) ?? 0) + 1)
  return m
})

const range = computed<[number, number] | null>(() => {
  let lo = Infinity
  let hi = -Infinity
  for (const r of shown.value) {
    lo = Math.min(lo, r.ts)
    hi = Math.max(hi, r.ts)
  }
  for (const m of marks.value) {
    lo = Math.min(lo, m.ts)
    hi = Math.max(hi, m.ts)
  }
  if (!Number.isFinite(lo)) return props.model.range
  // 两头留 2% 的边，单点时撑开一分钟
  const span = Math.max(hi - lo, 60_000)
  return [lo - span * 0.02, lo + span * 1.02]
})
const xOf = (ts: number) => {
  const r = range.value
  if (!r) return 0
  return Math.min(100, Math.max(0, ((ts - r[0]) / (r[1] - r[0])) * 100))
}
const ticks = computed(() => {
  const r = range.value
  if (!r) return []
  return [0, 0.25, 0.5, 0.75, 1].map((f) => ({ f, ts: new Date(r[0] + (r[1] - r[0]) * f).toISOString() }))
})

const colorOf = (id: string) => lineageColor(props.model.byId.get(id)?.lineage ?? -1, theme.value)

const hoverKey = ref<string | null>(null)
const SHAPE: Record<FleetRecord['kind'], string> = {
  message: 'rounded-full',
  report: 'rounded-[2px]',
  delegation: 'rotate-45 rounded-[1px]',
}

function label(r: FleetRecord): string {
  return t('ctxFleet.timeline.label', {
    time: fmtDateTime(new Date(r.ts).toISOString()),
    from: nameOf(r.fromId, r.fromName ?? r.from),
    to: nameOf(r.toId, r.to),
    kind: t(`ctxFleet.kind.${r.kind}`),
  })
}
function markTip(m: TaskMark): string {
  const tk = props.model.tasks.find((x) => x.key === m.task)
  return t('ctxFleet.timeline.taskTip', { id: tk?.id ?? m.label, subject: tk?.subject ?? '' })
}
</script>

<template>
  <section class="flex flex-col gap-2">
    <header class="flex flex-wrap items-center gap-2">
      <h3 class="m-0 flex items-center gap-1.5 text-sm font-medium ink-strong">
        <i class="i-jannchie-collaborate h-4 w-4 ink-soft" aria-hidden="true" />{{ t('ctxFleet.timeline.title') }}
      </h3>
      <span class="text-xs ink-soft num">{{ t('ctxFleet.timeline.sub', { shown: fmtInt(shown.length), total: fmtInt(model.records.length), lanes: fmtInt(lanes.length) }) }}</span>
      <span class="flex-1" />
      <button
        v-if="taskOf"
        type="button"
        class="segment border-0 pill-selected"
        :title="t('ctxFleet.timeline.clearTask')"
        @click="emit('task', null)"
      >
        {{ t('ctxFleet.timeline.taskFilter', { id: taskOf.id }) }}<i class="i-jannchie-x h-3 w-3" aria-hidden="true" />
      </button>
      <button type="button" class="segment border-0 surface ink-muted" :disabled="!shown.length" @click="emit('pick', shown[0]?.key ?? null)">
        <i class="i-jannchie-arrow-up h-3 w-3 -rotate-90" aria-hidden="true" />{{ t('ctxFleet.timeline.earliest') }}
      </button>
      <button type="button" class="segment border-0 surface ink-muted" :disabled="!shown.length" @click="emit('pick', shown.at(-1)?.key ?? null)">
        {{ t('ctxFleet.timeline.latest') }}<i class="i-jannchie-arrow-up h-3 w-3 rotate-90" aria-hidden="true" />
      </button>
      <input v-model="query" type="search" class="field max-w-52 py-1 text-xs" :placeholder="t('ctxFleet.timeline.search')" :aria-label="t('ctxFleet.timeline.search')" />
    </header>

    <p v-if="!model.records.length && !model.marks.length" class="m-0 py-4 text-center text-xs ink-soft">
      {{ model.pending ? t('ctxFleet.timeline.loading') : t('ctxFleet.timeline.none') }}
    </p>
    <p v-else-if="!shown.length && !marks.length" class="m-0 py-4 text-center text-xs ink-soft">{{ t('ctxFleet.timeline.empty') }}</p>
    <div v-else class="flex min-w-0 text-xs">
      <!-- 泳道名 -->
      <ul class="m-0 w-40 shrink-0 p-0 pr-2">
        <li v-for="a in lanes" :key="a.id" class="flex list-none items-center gap-1.5" :style="{ height: `${LANE_H}px` }">
          <span class="h-2 w-2 shrink-0 rounded-full" :style="{ background: colorOf(a.id) }" />
          <button
            type="button"
            class="min-w-0 flex-1 truncate border-0 bg-transparent p-0 text-left text-xs kbd-ring"
            :class="selected === a.id ? 'ink-accent' : 'ink-strong'"
            :title="selected === a.id ? t('ctxFleet.graph.unfocus') : t('ctxFleet.graph.focus')"
            @click="emit('select', selected === a.id ? null : a.id)"
          >
            {{ a.name }}
          </button>
          <span class="shrink-0 ink-soft num" :title="t('ctxFleet.timeline.traffic', { n: fmtInt(laneCount.get(a.id) ?? 0) })">{{ fmtInt(laneCount.get(a.id) ?? 0) }}</span>
        </li>
      </ul>
      <!-- 轨道 -->
      <div class="relative min-w-0 flex-1">
        <div class="relative" :style="{ height: `${lanes.length * LANE_H}px` }">
          <template v-for="a in lanes" :key="a.id">
            <div class="absolute inset-x-0 h-px bg-[var(--line)] opacity-60" :style="{ top: `${laneY.get(a.id)}px` }" />
            <!-- 这个 agent 日志里第一条到最后一条事件：活跃区间 -->
            <div
              v-if="a.firstTs != null && a.lastTs != null"
              class="absolute h-1 rounded-full opacity-35"
              :style="{
                top: `${laneY.get(a.id)! - 2}px`,
                left: `${xOf(a.firstTs)}%`,
                width: `${Math.max(0.4, xOf(a.lastTs) - xOf(a.firstTs))}%`,
                background: colorOf(a.id),
              }"
            />
          </template>
          <!-- 连线：x 用百分比，y 用像素；不随拉伸变粗 -->
          <svg class="pointer-events-none absolute inset-0 h-full w-full overflow-visible" :viewBox="`0 0 1000 ${Math.max(1, lanes.length * LANE_H)}`" preserveAspectRatio="none" aria-hidden="true">
            <template v-for="r in shown" :key="r.key">
              <line
                v-if="r.toId && r.toId !== r.lane && laneY.has(r.toId) && laneY.has(r.lane)"
                :x1="xOf(r.ts) * 10"
                :x2="xOf(r.ts) * 10"
                :y1="laneY.get(r.lane)"
                :y2="laneY.get(r.toId)"
                :stroke="colorOf(r.lane)"
                :stroke-width="hoverKey === r.key || picked === r.key ? 2 : 1"
                :stroke-opacity="hoverKey === r.key || picked === r.key ? 1 : 0.45"
                :stroke-dasharray="r.kind === 'delegation' ? '3 2' : undefined"
                vector-effect="non-scaling-stroke"
              />
            </template>
          </svg>
          <template v-for="r in shown" :key="r.key">
            <span
              v-if="r.toId && r.toId !== r.lane && laneY.has(r.toId)"
              class="pointer-events-none absolute h-1.5 w-1.5 -translate-x-1/2 -translate-y-1/2 rounded-full"
              :style="{ left: `${xOf(r.ts)}%`, top: `${laneY.get(r.toId)}px`, background: colorOf(r.lane) }"
            />
            <button
              v-if="laneY.has(r.lane)"
              type="button"
              class="absolute h-2.5 w-2.5 -translate-x-1/2 -translate-y-1/2 border-0 p-0 kbd-ring"
              :class="[SHAPE[r.kind], picked === r.key ? 'ring-2 ring-[var(--fg)] z-2' : '']"
              :style="{ left: `${xOf(r.ts)}%`, top: `${laneY.get(r.lane)}px`, background: r.kind === 'report' ? 'var(--bg)' : colorOf(r.lane), boxShadow: r.kind === 'report' ? `inset 0 0 0 2px ${colorOf(r.lane)}` : undefined }"
              :title="label(r)"
              :aria-label="label(r)"
              @mouseenter="hoverKey = r.key"
              @mouseleave="hoverKey = null"
              @click="emit('pick', picked === r.key ? null : r.key)"
            />
          </template>
          <button
            v-for="m in marks"
            v-show="laneY.has(m.lane)"
            :key="m.key"
            type="button"
            class="absolute z-1 -translate-x-1/2 rounded border-0 px-1 py-0 text-[10px] leading-[14px] num kbd-ring"
            :class="task === m.task ? 'pill-selected' : 'surface-strong ink-strong'"
            :style="{ left: `${xOf(m.ts)}%`, top: `${(laneY.get(m.lane) ?? 0) - 15}px`, boxShadow: `inset 0 -2px 0 ${colorOf(m.lane)}` }"
            :title="markTip(m)"
            :aria-label="markTip(m)"
            @click="emit('task', task === m.task ? null : m.task)"
          >
            {{ m.label }}
          </button>
        </div>
        <!-- 时间轴 -->
        <div class="relative mt-1 h-4 ink-soft num">
          <span
            v-for="tk in ticks"
            :key="tk.f"
            class="absolute whitespace-nowrap text-[10px]"
            :class="tk.f === 0 ? '' : tk.f === 1 ? '-translate-x-full' : '-translate-x-1/2'"
            :style="{ left: `${tk.f * 100}%` }"
          >{{ fmtDateTime(tk.ts) }}</span>
        </div>
      </div>
    </div>
    <div class="flex flex-wrap items-center gap-2 text-xs ink-soft">
      <button v-if="quiet.length" type="button" class="segment border-0 surface ink-muted" @click="showQuiet = !showQuiet">
        {{ showQuiet ? t('ctxFleet.timeline.hideQuiet') : t('ctxFleet.timeline.showQuiet', { n: fmtInt(quiet.length) }) }}
      </button>
      <span v-if="filtered.length > CAP">{{ t('ctxFleet.timeline.capped', { n: fmtInt(CAP) }) }}</span>
      <span class="flex-1" />
      <span class="inline-flex items-center gap-1"><span class="h-2 w-2 rounded-full bg-[var(--fg-2)]" />{{ t('ctxFleet.kind.message') }}</span>
      <span class="inline-flex items-center gap-1"><span class="h-2 w-2 rounded-[2px] shadow-[inset_0_0_0_2px_var(--fg-2)]" />{{ t('ctxFleet.kind.report') }}</span>
      <span class="inline-flex items-center gap-1"><span class="h-2 w-2 rotate-45 bg-[var(--fg-2)]" />{{ t('ctxFleet.kind.delegation') }}</span>
      <span class="inline-flex items-center gap-1"><span class="rounded px-1 text-[10px] surface-strong">1</span>{{ t('ctxFleet.kind.task') }}</span>
    </div>
  </section>
</template>
