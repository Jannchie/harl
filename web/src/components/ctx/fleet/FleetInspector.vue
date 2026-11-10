<script setup lang="ts">
// 事件检查：消息、回报、委派按时间倒序排成一列；选中一条看详情——
// 发送方 → 接收方、记录在谁的日志里和 seq、dsh 结算通知的说明、参与者卡片。
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { RouterLink } from 'vue-router'
import { useChartTheme } from '../../../lib/chartTheme'
import { lineageColor, touches, type FleetAgent, type FleetModel, type FleetRecord, type RecordKind } from '../../../lib/fleet'
import { fmtInt, truncate } from '../../../lib/format'
import MarkdownText from '../../MarkdownText.vue'
import ProviderTag from '../../ProviderTag.vue'
import Segmented from '../../Segmented.vue'
import TimeStamp from '../../TimeStamp.vue'

const props = defineProps<{ model: FleetModel; selected: string | null; picked: string | null }>()
const emit = defineEmits<{ pick: [key: string | null]; select: [id: string | null] }>()
const { t, te } = useI18n()
const theme = useChartTheme()

const kind = ref<'all' | RecordKind>('all')
const query = ref('')
const PAGE = 150
const limit = ref(PAGE)

const OPTIONS = computed(() =>
  (['all', 'message', 'report', 'delegation'] as const).map((v) => ({
    value: v,
    label: v === 'all' ? t('ctxFleet.inspector.all') : t(`ctxFleet.kind.${v}`),
  })),
)

const nameOf = (id: string | null, raw?: string | null) => (id ? (props.model.byId.get(id)?.name ?? id) : (raw ?? t('ctxFleet.inspector.unknown')))
const colorOf = (id: string | null) => lineageColor(id ? (props.model.byId.get(id)?.lineage ?? -1) : -1, theme.value)

const list = computed(() => {
  const q = query.value.trim().toLowerCase()
  const out: FleetRecord[] = []
  for (let i = props.model.records.length - 1; i >= 0; i--) {
    const r = props.model.records[i]!
    if (props.selected && !touches(r, props.selected)) continue
    if (kind.value !== 'all' && r.kind !== kind.value) continue
    if (q && !`${r.text} ${r.summary ?? ''} ${nameOf(r.fromId, r.fromName ?? r.from)} ${nameOf(r.toId, r.to)}`.toLowerCase().includes(q)) continue
    out.push(r)
  }
  return out
})
watch([kind, query, () => props.selected], () => (limit.value = PAGE))

const rec = computed(() => (props.picked ? props.model.records.find((r) => r.key === props.picked) ?? null : null))
// 从时间轴选中的记录不在当前页时，把页放大到能看到它
watch(rec, (r) => {
  if (!r) return
  const i = list.value.indexOf(r)
  if (i >= limit.value) limit.value = i + 1
})

const subLabel = (s: string) => (te(`ctxFleet.sub.${s}`) ? t(`ctxFleet.sub.${s}`) : s)
const preview = (r: FleetRecord) => truncate((r.kind === 'delegation' ? r.summary || r.text : r.text || r.summary || '').replace(/\s+/g, ' '), 140)

interface Party {
  role: 'sender' | 'recipient'
  id: string | null
  raw: string | null
  agent: FleetAgent | null
}
const parties = computed<Party[]>(() => {
  const r = rec.value
  if (!r) return []
  return [
    { role: 'sender', id: r.fromId, raw: r.fromName ?? r.from, agent: r.fromId ? (props.model.byId.get(r.fromId) ?? null) : null },
    { role: 'recipient', id: r.toId, raw: r.to, agent: r.toId ? (props.model.byId.get(r.toId) ?? null) : null },
  ]
})
const sourceAgent = computed(() => (rec.value ? (props.model.byId.get(rec.value.source) ?? null) : null))
const modeLabel = (m: string | null | undefined) =>
  m === 'one-shot' ? t('ctxFleet.graph.oneShot') : m === 'continuable' ? t('ctxFleet.graph.continuable') : m === 'external' ? t('ctxFleet.graph.external') : t('ctxFleet.graph.unknownMode')
</script>

<template>
  <section class="flex flex-col gap-2">
    <header class="flex flex-wrap items-center gap-2">
      <h3 class="m-0 flex items-center gap-1.5 text-sm font-medium ink-strong">
        <i class="i-jannchie-search-list h-4 w-4 ink-soft" aria-hidden="true" />{{ t('ctxFleet.inspector.title') }}
      </h3>
      <span class="flex-1" />
      <Segmented v-model="kind" :options="OPTIONS" :label="t('ctxFleet.inspector.kindGroup')" />
      <input v-model="query" type="search" class="field max-w-52 py-1 text-xs" :placeholder="t('ctxFleet.inspector.search')" :aria-label="t('ctxFleet.inspector.search')" />
    </header>
    <div class="grid min-w-0 gap-3 lg:grid-cols-[minmax(0,2fr)_minmax(0,3fr)]">
      <!-- 事件流 -->
      <div class="min-w-0">
        <p v-if="!list.length" class="m-0 py-4 text-center text-xs ink-soft">{{ t('ctxFleet.inspector.noMatch') }}</p>
        <ul class="m-0 flex max-h-[30rem] flex-col gap-px overflow-y-auto p-0">
          <li v-for="r in list.slice(0, limit)" :key="r.key" class="list-none">
            <button
              type="button"
              class="w-full flex flex-col gap-0.5 rounded-lg border-0 px-2 py-1 text-left text-xs kbd-ring"
              :class="picked === r.key ? 'pill-selected' : 'bg-transparent surface-hover'"
              @click="emit('pick', picked === r.key ? null : r.key)"
            >
              <span class="flex min-w-0 items-center gap-1.5">
                <span class="h-2 w-2 shrink-0 rounded-full" :style="{ background: colorOf(r.fromId ?? r.lane) }" />
                <span class="min-w-0 truncate ink-strong">{{ nameOf(r.fromId, r.fromName ?? r.from) }}</span>
                <i class="i-jannchie-arrow-up h-3 w-3 shrink-0 rotate-90 ink-soft" aria-hidden="true" />
                <span class="min-w-0 truncate ink-strong">{{ nameOf(r.toId, r.to) }}</span>
                <span class="flex-1" />
                <span class="shrink-0 ink-soft">{{ t(`ctxFleet.kind.${r.kind}`) }}</span>
                <TimeStamp :ts="new Date(r.ts).toISOString()" class="w-16 shrink-0 text-right ink-soft" />
              </span>
              <span v-if="preview(r)" class="truncate ink-soft">{{ preview(r) }}</span>
            </button>
          </li>
        </ul>
        <button
          v-if="list.length > limit"
          type="button"
          class="mt-1 rounded-md border-0 px-2 py-1 text-xs surface surface-hover kbd-ring"
          @click="limit += PAGE"
        >
          {{ t('ctxFleet.inspector.more', { n: fmtInt(Math.min(PAGE, list.length - limit)) }) }}
        </button>
      </div>

      <!-- 详情 -->
      <div class="min-w-0 rounded-xl p-3 text-xs surface">
        <p v-if="!rec" class="m-0 py-6 text-center ink-soft">{{ t('ctxFleet.inspector.pick') }}</p>
        <div v-else class="flex flex-col gap-2.5">
          <div class="flex flex-wrap items-center gap-2">
            <span class="rounded px-1.5 surface-strong ink-strong">{{ t(`ctxFleet.kind.${rec.kind}`) }}</span>
            <span v-if="rec.kind !== 'delegation'" class="ink-soft">{{ subLabel(rec.sub) }}</span>
            <TimeStamp :ts="new Date(rec.ts).toISOString()" mode="full" class="ink-soft" />
            <span class="flex-1" />
            <button type="button" class="rounded border-0 bg-transparent p-0.5 ink-soft kbd-ring surface-hover" :aria-label="t('ctxFleet.inspector.close')" :title="t('ctxFleet.inspector.close')" @click="emit('pick', null)">
              <i class="i-jannchie-x block h-3.5 w-3.5" aria-hidden="true" />
            </button>
          </div>
          <p class="m-0 ink-soft" :title="t('ctxFleet.inspector.originTip')">
            {{ t('ctxFleet.inspector.log', { name: sourceAgent?.name ?? rec.source }) }} ·
            <RouterLink
              v-if="sourceAgent"
              :to="{ name: 'session', params: { id: sourceAgent.info.id }, query: { view: 'events' } }"
              class="num ink-muted kbd-ring"
            >{{ t('ctxFleet.inspector.seq', { n: rec.seq }) }}</RouterLink>
            <span v-else class="num">{{ t('ctxFleet.inspector.seq', { n: rec.seq }) }}</span>
          </p>
          <p v-if="rec.sub === 'settled'" class="m-0 rounded-md px-2 py-1 ink-warning surface-strong">{{ t('ctxFleet.inspector.settledNote') }}</p>
          <p v-if="rec.settle" class="m-0 font-medium" :class="rec.settle === 'failed' ? 'ink-danger' : rec.settle === 'finished' || rec.settle === 'finishedFinal' ? 'ink-success' : 'ink-warning'">
            {{ t(`ctxFleet.settled.${rec.settle}`) }}
          </p>
          <template v-if="rec.kind === 'delegation' && rec.delegation">
            <div class="flex flex-wrap gap-x-3 gap-y-0.5 ink-soft">
              <span v-if="rec.delegation.label" class="ink-strong">{{ rec.delegation.label }}</span>
              <span>{{ t('ctxFleet.inspector.tool') }}: <code>{{ rec.delegation.tool }}</code></span>
              <span v-if="rec.delegation.mode">{{ t('ctxFleet.inspector.mode') }}: {{ modeLabel(rec.delegation.mode) }}</span>
              <span>{{ t(`ctxFleet.tasks.status.${rec.delegation.status}`) }}</span>
            </div>
            <div class="ink-soft">{{ t('ctxFleet.inspector.prompt') }}</div>
          </template>
          <p v-else-if="rec.summary && rec.summary !== rec.text" class="m-0 ink-muted"><span class="ink-soft">{{ t('ctxFleet.inspector.summary') }}:</span> {{ rec.summary }}</p>
          <p v-if="rec.encrypted" class="m-0 ink-soft">{{ t('ctxFleet.inspector.encrypted') }}</p>
          <MarkdownText v-else-if="rec.text" :text="rec.text" class="max-h-[24rem] overflow-auto rounded-lg p-2.5 surface" />
          <p v-else class="m-0 ink-soft">{{ rec.sub === 'settled' ? t('ctxFleet.inspector.noReport') : t('ctxFleet.inspector.noBody') }}</p>
          <div v-if="rec.structured">
            <div class="mb-0.5 ink-soft">{{ t('ctxFleet.inspector.structured') }}</div>
            <pre class="m-0 max-h-48 overflow-auto whitespace-pre-wrap break-words rounded-md p-2 font-mono surface">{{ rec.structured }}</pre>
          </div>

          <div class="ink-soft">{{ t('ctxFleet.inspector.participants') }}</div>
          <div class="grid gap-2 sm:grid-cols-2">
            <div
              v-for="p in parties"
              :key="p.role"
              class="flex min-w-0 flex-col gap-1 rounded-lg p-2 surface-strong"
              :style="{ boxShadow: `inset 3px 0 0 ${colorOf(p.id)}` }"
            >
              <span class="ink-soft">{{ t(`ctxFleet.inspector.${p.role}`) }}</span>
              <span class="truncate text-sm ink-strong">{{ nameOf(p.id, p.raw) }}</span>
              <span v-if="p.agent" class="flex flex-wrap items-center gap-x-2 ink-soft">
                <ProviderTag :provider="p.agent.info.provider" />
                <span v-if="p.agent.role !== 'child'">{{ p.agent.role === 'current' ? t('ctx.fleet.current') : t('ctx.fleet.parent') }}</span>
                <span v-if="p.agent.running" class="ink-success">{{ t('ctx.fleet.running') }}</span>
              </span>
              <span v-if="p.agent" class="flex flex-wrap gap-1.5">
                <button type="button" class="rounded border-0 px-1.5 text-xs kbd-ring surface surface-hover" @click="emit('select', selected === p.id ? null : p.id)">
                  {{ selected === p.id ? t('ctxFleet.inspector.unfilter') : t('ctxFleet.inspector.filter') }}
                </button>
                <RouterLink
                  v-if="p.agent.role !== 'current'"
                  :to="{ name: 'session', params: { id: p.agent.info.id }, query: { view: 'context' } }"
                  class="rounded px-1.5 ink-muted kbd-ring surface surface-hover"
                >{{ t('ctxFleet.inspector.open') }}</RouterLink>
              </span>
            </div>
          </div>
        </div>
      </div>
    </div>
  </section>
</template>
