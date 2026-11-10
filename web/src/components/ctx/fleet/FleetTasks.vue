<script setup lang="ts">
// 协作任务：团队任务（Claude Code 的 Task* 工具、dsh 的 team/task 记录）和子 agent 委派（各家都有）。
// 状态筛选、搜索；展开看任务说明、写入范围（dsh）、依赖链、最终回复。
import { computed, nextTick, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useChartTheme } from '../../../lib/chartTheme'
import { lineageColor, taskTouches, type FleetModel, type FleetTask } from '../../../lib/fleet'
import { fmtInt, tn, truncate } from '../../../lib/format'
import Segmented from '../../Segmented.vue'
import TimeStamp from '../../TimeStamp.vue'

const props = defineProps<{ model: FleetModel; selected: string | null; task: string | null }>()
const emit = defineEmits<{ task: [key: string | null]; select: [id: string | null] }>()
const { t, te } = useI18n()
const theme = useChartTheme()

type Bucket = 'all' | FleetTask['bucket']
const bucket = ref<Bucket>('all')
const query = ref('')
const open = ref(new Set<string>())

const OPTIONS = computed(() =>
  (['all', 'in_progress', 'pending', 'completed'] as const).map((v) => ({ value: v, label: t(`ctxFleet.tasks.${v}`) })),
)

const list = computed(() => {
  const q = query.value.trim().toLowerCase()
  return props.model.tasks.filter((x) => {
    if (x.status === 'deleted') return false
    if (props.selected && !taskTouches(x, props.selected)) return false
    if (bucket.value !== 'all' && x.bucket !== bucket.value) return false
    if (q && !`${x.id} ${x.subject} ${x.description} ${x.owner ?? ''}`.toLowerCase().includes(q)) return false
    return true
  })
})
const total = computed(() => props.model.tasks.filter((x) => x.status !== 'deleted').length)
const groups = computed(() =>
  (['team', 'delegation'] as const)
    .map((kind) => ({ kind, items: list.value.filter((x) => x.kind === kind) }))
    .filter((g) => g.items.length),
)

function ownerName(x: FleetTask): string | null {
  if (x.ownerId) return props.model.byId.get(x.ownerId)?.name ?? x.ownerId
  return x.owner
}
const colorOf = (id: string | null) => lineageColor(id ? (props.model.byId.get(id)?.lineage ?? -1) : -1, theme.value)
const tone = (x: FleetTask) =>
  x.status === 'failed' ? 'ink-danger' : x.status === 'stopped' ? 'ink-warning' : x.bucket === 'completed' ? 'ink-success' : x.bucket === 'in_progress' ? 'ink-accent' : 'ink-soft'
const statusLabel = (s: string) => {
  const k = `ctxFleet.tasks.status.${s}`
  return te(k) ? t(k) : s
}

function toggle(k: string) {
  const s = new Set(open.value)
  if (s.has(k)) s.delete(k)
  else s.add(k)
  open.value = s
}

// 时间轴上点了任务：展开并滚到它
const root = ref<HTMLElement | null>(null)
watch(
  () => props.task,
  async (k) => {
    if (!k) return
    if (!open.value.has(k)) toggle(k)
    bucket.value = 'all'
    await nextTick()
    root.value?.querySelector(`[data-task="${CSS.escape(k)}"]`)?.scrollIntoView({ block: 'nearest', behavior: 'smooth' })
  },
)
</script>

<template>
  <section ref="root" class="flex flex-col gap-2">
    <header class="flex flex-wrap items-center gap-2">
      <h3 class="m-0 flex items-center gap-1.5 text-sm font-medium ink-strong">
        <i class="i-jannchie-check-square h-4 w-4 ink-soft" aria-hidden="true" />{{ t('ctxFleet.tasks.title') }}
      </h3>
      <span class="text-xs ink-soft num">{{ t('ctxFleet.tasks.sub', { shown: fmtInt(list.length), total: fmtInt(total) }) }}</span>
      <span class="flex-1" />
      <Segmented v-model="bucket" :options="OPTIONS" :label="t('ctxFleet.tasks.filterLabel')" />
      <input v-model="query" type="search" class="field max-w-52 py-1 text-xs" :placeholder="t('ctxFleet.tasks.search')" :aria-label="t('ctxFleet.tasks.search')" />
    </header>
    <p v-if="!total" class="m-0 py-4 text-center text-xs ink-soft">{{ t('ctxFleet.tasks.none') }}</p>
    <p v-else-if="!list.length" class="m-0 py-4 text-center text-xs ink-soft">{{ t('ctxFleet.tasks.noMatch') }}</p>
    <div v-for="g in groups" :key="g.kind" class="flex flex-col gap-1">
      <h4 class="m-0 text-xs font-medium ink-soft">{{ t(`ctxFleet.tasks.${g.kind}`) }} <span class="num">{{ fmtInt(g.items.length) }}</span></h4>
      <ul class="m-0 flex max-h-[28rem] flex-col gap-1 overflow-y-auto p-0">
        <li
          v-for="x in g.items"
          :key="x.key"
          :data-task="x.key"
          class="list-none rounded-lg text-xs"
          :class="task === x.key ? 'surface-strong ring-1 ring-[var(--accent)]' : 'surface'"
          :style="{ boxShadow: `inset 3px 0 0 ${colorOf(x.ownerId)}` }"
        >
          <div class="flex min-w-0 items-center gap-2 px-2.5 py-1.5">
            <button
              type="button"
              class="flex min-w-0 flex-1 items-center gap-2 border-0 bg-transparent p-0 text-left text-xs kbd-ring"
              :aria-expanded="open.has(x.key)"
              @click="toggle(x.key)"
            >
              <i class="fold-chevron i-jannchie-chevron-right" :class="open.has(x.key) ? 'rotate-90' : ''" aria-hidden="true" />
              <span class="shrink-0 num ink-soft">{{ truncate(x.id, 24) }}</span>
              <span class="min-w-0 flex-1 truncate ink-strong">{{ x.subject }}</span>
            </button>
            <span v-if="x.blocked" class="shrink-0 rounded px-1.5 ink-warning surface">{{ t('ctxFleet.tasks.blocked') }}</span>
            <span v-else-if="x.kind === 'team' && x.bucket === 'pending' && x.blockedBy.length" class="shrink-0 rounded px-1.5 ink-success surface">{{ t('ctxFleet.tasks.ready') }}</span>
            <span class="shrink-0" :class="tone(x)">{{ statusLabel(x.status) }}</span>
            <button
              v-if="x.ownerId"
              type="button"
              class="max-w-36 shrink-0 truncate rounded border-0 px-1.5 text-xs kbd-ring surface-hover"
              :class="selected === x.ownerId ? 'pill-selected' : 'surface ink-muted'"
              :title="selected === x.ownerId ? t('ctxFleet.graph.unfocus') : t('ctxFleet.graph.focus')"
              @click="emit('select', selected === x.ownerId ? null : x.ownerId)"
            >
              {{ ownerName(x) }}
            </button>
            <span v-else class="max-w-36 shrink-0 truncate ink-soft">{{ ownerName(x) ?? t('ctxFleet.tasks.unowned') }}</span>
            <button
              type="button"
              class="shrink-0 rounded border-0 bg-transparent p-0.5 kbd-ring surface-hover"
              :class="task === x.key ? 'ink-accent' : 'ink-soft'"
              :title="task === x.key ? t('ctxFleet.timeline.clearTask') : t('ctxFleet.tasks.timeline')"
              :aria-label="task === x.key ? t('ctxFleet.timeline.clearTask') : t('ctxFleet.tasks.timeline')"
              :aria-pressed="task === x.key"
              @click="emit('task', task === x.key ? null : x.key)"
            >
              <i class="i-jannchie-collaborate block h-3.5 w-3.5" aria-hidden="true" />
            </button>
          </div>
          <div v-if="open.has(x.key)" class="flex flex-col gap-1.5 px-2.5 pb-2 pl-8">
            <div class="flex flex-wrap gap-x-3 gap-y-0.5 ink-soft">
              <span><TimeStamp :ts="new Date(x.ts).toISOString()" mode="full" /></span>
              <span v-if="x.revisions.length > 1" class="num">{{ tn('ctxFleet.tasks.revisions', x.revisions.length) }}</span>
              <span v-if="x.chain.length">{{ t('ctxFleet.tasks.blockedBy') }} <span class="num ink-muted">{{ x.chain.join(' → ') }}</span></span>
              <span v-if="x.writeScopes.length">{{ t('ctxFleet.tasks.scopes') }}: <code class="ink-muted">{{ x.writeScopes.join(' · ') }}</code></span>
              <span v-if="x.delegation?.mode">{{ t('ctxFleet.inspector.mode') }}: {{ x.delegation.mode === 'one-shot' ? t('ctxFleet.graph.oneShot') : x.delegation.mode === 'continuable' ? t('ctxFleet.graph.continuable') : x.delegation.mode === 'external' ? t('ctxFleet.graph.external') : t('ctxFleet.graph.unknownMode') }}</span>
              <span v-if="x.delegation?.settle">{{ t(`ctxFleet.settled.${x.delegation.settle}`) }}</span>
            </div>
            <div v-if="x.description">
              <div class="mb-0.5 ink-soft">{{ x.kind === 'delegation' ? t('ctxFleet.inspector.prompt') : t('ctxFleet.tasks.brief') }}</div>
              <p class="m-0 max-h-48 overflow-y-auto whitespace-pre-wrap break-words rounded-md p-2 surface">{{ x.description }}</p>
            </div>
            <div v-if="x.report">
              <div class="mb-0.5 ink-soft">{{ t('ctxFleet.tasks.report') }}</div>
              <p class="m-0 max-h-48 overflow-y-auto whitespace-pre-wrap break-words rounded-md p-2 surface">{{ x.report }}</p>
            </div>
          </div>
        </li>
      </ul>
    </div>
  </section>
</template>
