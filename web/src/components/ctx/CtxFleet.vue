<script setup lang="ts">
// Agent 网络：父会话链 + 当前会话 + 所有子 agent（递归），每个一张卡。
// 父子关系来自会话头里的 parent_session，各卡的数字是把那个会话的事件也折一遍算的。
import { computed, onUnmounted, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { RouterLink } from 'vue-router'
import { api, type SessionInfo } from '../../api'
import { useChartTheme } from '../../lib/chartTheme'
import { ALL_CATS, foldContext, type CtxFold } from '../../lib/context'
import { catColor, fleetCache as cache, fleetStats, summarize, type CtxSummary, type FleetSlot } from '../../lib/ctxView'
import { fmtBig, fmtMoney, fmtMs, tn } from '../../lib/format'
import { ensurePricing } from '../../lib/pricing'
import { bySession } from '../../lib/sessions'
import DashCard from '../dash/DashCard.vue'
import ProviderTag from '../ProviderTag.vue'
import AtomSpinner from '../AtomSpinner.vue'

const props = defineProps<{ info: SessionInfo; fold: CtxFold; summary: CtxSummary }>()
const emit = defineEmits<{ family: [cost: number | null] }>()
const { t } = useI18n()
const theme = useChartTheme()

interface Card {
  info: SessionInfo
  depth: number
  role: 'parent' | 'current' | 'child'
}

const cards = computed<Card[]>(() => {
  const out: Card[] = []
  const parents: SessionInfo[] = []
  let p = props.info.parent_session
  const seen = new Set<string>([props.info.session])
  while (p && !seen.has(p)) {
    seen.add(p)
    const s = bySession.value.get(p)
    if (!s) break
    parents.unshift(s)
    p = s.parent_session
  }
  parents.forEach((s, i) => out.push({ info: s, depth: i, role: 'parent' }))
  out.push({ info: props.info, depth: parents.length, role: 'current' })
  const kids = new Map<string, SessionInfo[]>()
  for (const s of bySession.value.values()) {
    if (!s.parent_session) continue
    const list = kids.get(s.parent_session)
    if (list) list.push(s)
    else kids.set(s.parent_session, [s])
  }
  const walk = (id: string, depth: number) => {
    for (const c of (kids.get(id) ?? []).sort((a, b) => a.mtime.localeCompare(b.mtime))) {
      if (seen.has(c.session)) continue
      seen.add(c.session)
      out.push({ info: c, depth, role: 'child' })
      walk(c.session, depth + 1)
    }
  }
  walk(props.info.session, parents.length + 1)
  return out
})

const keyOf = (s: SessionInfo) => `${s.id}@${s.mtime}`
let ac: AbortController | null = null
/** 这次挂载里已经重试过的 error 卡，免得每次 cards 变化都再拉一遍 */
const retried = new Set<string>()

/** 每次最多同时拉 4 个；当前会话直接用 ContextView 折好的 */
async function loadAll(list: Card[]) {
  ac?.abort()
  ac = new AbortController()
  const signal = ac.signal
  // 还是 loading 的是上一轮被取消的，重新排上；error 的每次挂载重试一回（可能只是后端刚好在重启）
  const queue = list.filter((c) => {
    if (c.role === 'current') return false
    const k = keyOf(c.info)
    const slot = cache.get(k) ?? 'loading'
    if (slot === 'loading') return true
    if (slot !== 'error' || retried.has(k)) return false
    retried.add(k)
    return true
  })
  for (const c of queue) cache.set(keyOf(c.info), 'loading')
  // 费用要用联网的价格目录算（内置快照可能没有新模型），和拉事件并行
  const pricing = ensurePricing()
  const worker = async () => {
    while (queue.length && !signal.aborted) {
      const c = queue.shift()!
      try {
        const evs = await api.events(c.info.id, signal)
        await pricing
        const f = foldContext(evs)
        cache.set(keyOf(c.info), fleetStats(f, summarize(f)))
      } catch {
        if (!signal.aborted) cache.set(keyOf(c.info), 'error')
      }
    }
  }
  await Promise.all([worker(), worker(), worker(), worker()])
}

watch(
  cards,
  (list) => {
    if (list.length > 1) void loadAll(list)
  },
  { immediate: true },
)
onUnmounted(() => ac?.abort())

const current = computed(() => fleetStats(props.fold, props.summary))

const rows = computed(() =>
  cards.value.map((c) => {
    const slot: FleetSlot | undefined = c.role === 'current' ? current.value : cache.get(keyOf(c.info))
    return { ...c, slot, st: typeof slot === 'object' ? slot : null }
  }),
)

/** 当前会话和它的子孙一起的费用 */
const familyCost = computed(() => {
  const fam = rows.value.filter((r) => r.role !== 'parent')
  if (fam.length < 2 || fam.some((r) => !r.st)) return null
  return fam.reduce((a, r) => a + r.st!.cost, 0)
})
watch(familyCost, (v) => emit('family', v), { immediate: true })

/** 两分钟内还在写的算进行中 */
function running(s: SessionInfo): boolean {
  return Date.now() - Date.parse(s.mtime) < 120_000
}
</script>

<template>
  <DashCard v-if="cards.length > 1" icon="i-jannchie-network" :title="t('ctx.fleet.title')" :caption="t('ctx.fleet.caption')">
    <ul class="m-0 flex flex-col gap-1.5 p-0">
      <li v-for="c in rows" :key="c.info.id" class="list-none" :style="{ paddingLeft: `${Math.min(c.depth, 6) * 20}px` }">
        <component
          :is="c.role === 'current' ? 'div' : RouterLink"
          :to="c.role === 'current' ? undefined : { name: 'session', params: { id: c.info.id }, query: { view: 'context' } }"
          class="flex flex-col gap-1.5 rounded-xl px-3 py-2 text-xs kbd-ring"
          :class="c.role === 'current' ? 'surface-strong' : 'surface surface-hover'"
        >
          <div class="flex min-w-0 items-center gap-2">
            <i class="h-3.5 w-3.5 shrink-0" :class="c.role === 'child' ? 'i-jannchie-robot ink-accent' : 'i-jannchie-chat ink-soft'" aria-hidden="true" />
            <span class="min-w-0 flex-1 truncate text-sm ink-strong">{{ c.info.title ?? c.info.session }}</span>
            <span v-if="c.role !== 'child'" class="shrink-0 rounded px-1.5 ink-soft surface">{{ c.role === 'current' ? t('ctx.fleet.current') : t('ctx.fleet.parent') }}</span>
            <span v-if="running(c.info)" class="inline-flex shrink-0 items-center gap-1 ink-success">
              <span class="h-1.5 w-1.5 animate-pulse rounded-full bg-current" />{{ t('ctx.fleet.running') }}
            </span>
            <ProviderTag :provider="c.info.provider" class="shrink-0 ink-soft" />
          </div>
          <template v-if="c.st">
            <div class="flex h-1.5 w-full overflow-hidden rounded-full surface">
              <span
                v-for="cat in ALL_CATS"
                :key="cat"
                :style="{ width: `${c.st.last ? (c.st.parts[cat] / c.st.last) * 100 : 0}%`, background: catColor(cat, theme) }"
              />
            </div>
            <div class="flex flex-wrap gap-x-3 ink-soft num">
              <span>{{ tn('ctx.fleet.steps', c.st.steps) }}</span>
              <span>{{ fmtBig(c.st.last) }} · {{ t('ctx.fleet.peak', { n: fmtBig(c.st.peak) }) }}</span>
              <span>{{ fmtBig(c.st.billed) }} tok</span>
              <span>{{ fmtMoney(c.st.cost) }}</span>
              <span>{{ fmtMs(c.st.wallMs) }}</span>
            </div>
          </template>
          <span v-else-if="c.slot === 'loading'" class="inline-flex items-center gap-1.5 ink-soft"><AtomSpinner class="h-3 w-3" />{{ t('ctx.fleet.loading') }}</span>
          <span v-else-if="c.slot === 'error'" class="ink-soft">{{ t('ctx.fleet.missing') }}</span>
        </component>
      </li>
    </ul>
  </DashCard>
</template>
