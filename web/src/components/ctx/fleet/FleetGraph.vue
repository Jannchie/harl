<script setup lang="ts">
// 家族图：父会话链 → 当前 → 子孙，树形连线；每个直接子 agent 一个分支色，子孙沿用。
// 悬停一张卡，它的祖先和子孙整条链高亮，其余变淡。卡片内容沿用原来的：组成条、请求数、峰值、费用、时长、进行中。
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { RouterLink } from 'vue-router'
import { useChartTheme } from '../../../lib/chartTheme'
import { ALL_CATS } from '../../../lib/context'
import { catColor, type FleetSlot } from '../../../lib/ctxView'
import { lineageColor, type FleetAgent } from '../../../lib/fleet'
import { fmtBig, fmtMoney, fmtMs, fmtPct, tn } from '../../../lib/format'
import AtomSpinner from '../../AtomSpinner.vue'
import ProviderTag from '../../ProviderTag.vue'

const props = defineProps<{
  agents: FleetAgent[]
  slots: Map<string, FleetSlot | undefined>
  selected: string | null
}>()
const emit = defineEmits<{ select: [id: string | null] }>()
const { t } = useI18n()
const theme = useChartTheme()

const hovered = ref<string | null>(null)

const tree = computed(() => {
  const list = props.agents
  const inList = new Set(list.map((a) => a.id))
  const parentOf = new Map(list.map((a) => [a.id, a.parent && inList.has(a.parent) ? a.parent : null]))
  // 每个上级的最后一个下级：它下面不再延伸竖线
  const lastOf = new Map<string | null, string>()
  for (const a of list) lastOf.set(parentOf.get(a.id) ?? null, a.id)
  const isLast = (id: string) => lastOf.get(parentOf.get(id) ?? null) === id
  return list.map((a) => {
    const chain: string[] = []
    for (let p: string | null = a.id; p; p = parentOf.get(p) ?? null) chain.unshift(p)
    const slot = props.slots.get(a.id)
    const st = typeof slot === 'object' ? slot : null
    return {
      a,
      chain,
      /** 第 k 列：经过的祖先在它之后还有兄弟时画竖线 */
      rails: chain.slice(1, -1).map((id) => !isLast(id)),
      elbow: chain.length > 1,
      last: isLast(a.id),
      slot,
      st,
      color: lineageColor(a.lineage, theme.value),
    }
  })
})

/** 悬停的那张卡的祖先 + 自己 + 子孙 */
const lit = computed<Set<string> | null>(() => {
  const h = hovered.value
  if (!h) return null
  const row = tree.value.find((r) => r.a.id === h)
  if (!row) return null
  const s = new Set(row.chain)
  for (const r of tree.value) if (r.chain.includes(h)) s.add(r.a.id)
  return s
})

function occupancy(r: (typeof tree.value)[number]): number | null {
  if (!r.st || !r.a.window) return null
  return r.st.last / r.a.window
}

const modeLabel = (m: FleetAgent['mode']) => (m === 'one-shot' ? t('ctxFleet.graph.oneShot') : t('ctxFleet.graph.continuable'))
</script>

<template>
  <ul class="m-0 flex flex-col p-0" @mouseleave="hovered = null">
    <li
      v-for="r in tree"
      :key="r.a.info.id"
      class="relative flex list-none items-stretch transition-opacity"
      :class="lit && !lit.has(r.a.id) ? 'opacity-40' : ''"
      @mouseenter="hovered = r.a.id"
      @focusin="hovered = r.a.id"
    >
      <!-- 树形连线：每层一列 20px -->
      <span v-for="(on, k) in r.rails" :key="k" class="relative w-5 shrink-0" aria-hidden="true">
        <span v-if="on" class="absolute inset-y-0 left-2.5 w-px bg-[var(--line)]" />
      </span>
      <span v-if="r.elbow" class="relative w-5 shrink-0" aria-hidden="true">
        <span class="absolute left-2.5 top-0 h-1/2 w-px" :style="{ background: r.color }" />
        <span v-if="!r.last" class="absolute bottom-0 left-2.5 top-1/2 w-px bg-[var(--line)]" />
        <span class="absolute left-2.5 right-0 top-1/2 h-px" :style="{ background: r.color }" />
      </span>
      <div class="min-w-0 flex-1 py-[3px]">
        <component
          :is="r.a.role === 'current' ? 'div' : RouterLink"
          :to="r.a.role === 'current' ? undefined : { name: 'session', params: { id: r.a.info.id }, query: { view: 'context' } }"
          class="flex flex-col gap-1.5 rounded-xl border-l-3 border-solid px-3 py-2 text-xs kbd-ring"
          :class="[r.a.role === 'current' ? 'surface-strong' : 'surface surface-hover', selected === r.a.id ? 'ring-1 ring-[var(--accent)]' : '']"
          :style="{ borderLeftColor: r.color }"
        >
          <div class="flex min-w-0 items-center gap-2">
            <i class="h-3.5 w-3.5 shrink-0" :class="r.a.role === 'child' ? 'i-jannchie-robot' : 'i-jannchie-chat ink-soft'" :style="r.a.role === 'child' ? { color: r.color } : undefined" aria-hidden="true" />
            <span class="min-w-0 flex-1 truncate text-sm ink-strong" :title="r.a.info.title ?? undefined">{{ r.a.role === 'child' ? r.a.name : (r.a.info.title ?? r.a.name) }}</span>
            <span v-if="r.a.role !== 'child'" class="shrink-0 rounded px-1.5 ink-soft surface">{{ r.a.role === 'current' ? t('ctx.fleet.current') : t('ctx.fleet.parent') }}</span>
            <span v-if="r.a.mode" class="shrink-0 rounded px-1.5 ink-soft surface" :title="t('ctxFleet.graph.modeTip')">{{ modeLabel(r.a.mode) }}</span>
            <span v-if="r.a.running" class="inline-flex shrink-0 items-center gap-1 ink-success">
              <span class="h-1.5 w-1.5 animate-pulse rounded-full bg-current" />{{ t('ctx.fleet.running') }}
            </span>
            <ProviderTag :provider="r.a.info.provider" class="shrink-0 ink-soft" />
            <button
              type="button"
              class="shrink-0 rounded border-0 bg-transparent p-0.5 kbd-ring surface-hover"
              :class="selected === r.a.id ? 'ink-accent' : 'ink-soft'"
              :title="selected === r.a.id ? t('ctxFleet.graph.unfocus') : t('ctxFleet.graph.focus')"
              :aria-label="selected === r.a.id ? t('ctxFleet.graph.unfocus') : t('ctxFleet.graph.focus')"
              :aria-pressed="selected === r.a.id"
              @click.prevent.stop="emit('select', selected === r.a.id ? null : r.a.id)"
            >
              <i class="i-jannchie-search block h-3.5 w-3.5" aria-hidden="true" />
            </button>
          </div>
          <template v-if="r.st">
            <!-- 知道窗口时按窗口画（剩下的是空闲），否则按最后一次请求画满 -->
            <div class="flex h-1.5 w-full overflow-hidden rounded-full surface">
              <span
                v-for="cat in ALL_CATS"
                :key="cat"
                :style="{
                  width: `${(r.a.window ? r.st.parts[cat] / Math.max(r.a.window, r.st.last) : r.st.last ? r.st.parts[cat] / r.st.last : 0) * 100}%`,
                  background: catColor(cat, theme),
                }"
              />
            </div>
            <div class="flex flex-wrap gap-x-3 ink-soft num">
              <span>{{ tn('ctx.fleet.steps', r.st.steps) }}</span>
              <span>{{ fmtBig(r.st.last) }} · {{ t('ctx.fleet.peak', { n: fmtBig(r.st.peak) }) }}</span>
              <span
                v-if="occupancy(r) != null"
                :class="occupancy(r)! >= 0.8 ? 'ink-danger' : ''"
                :title="t('ctxFleet.graph.windowTip', { used: fmtBig(r.st.last), total: fmtBig(r.a.window) })"
              >{{ t('ctxFleet.graph.window', { pct: fmtPct(occupancy(r), 0) }) }}</span>
              <span>{{ fmtBig(r.st.billed) }} tok</span>
              <span>{{ fmtMoney(r.st.cost) }}</span>
              <span>{{ fmtMs(r.st.wallMs) }}</span>
            </div>
          </template>
          <span v-else-if="r.slot === 'loading'" class="inline-flex items-center gap-1.5 ink-soft"><AtomSpinner class="h-3 w-3" />{{ t('ctx.fleet.loading') }}</span>
          <span v-else-if="r.slot === 'error'" class="ink-soft">{{ t('ctx.fleet.missing') }}</span>
        </component>
      </div>
    </li>
  </ul>
</template>
