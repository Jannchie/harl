<script lang="ts">
import type { Cat } from '../../lib/context'

/**
 * 外面要求浏览器去某处（还没接线，给 ContextView 以后用）。字段都可选，按下面的优先级：
 * - nodeId：节点键（CtxNode.key），和 focus 一样：切到分类、展开类别、清掉挡住它的过滤、展开并滚过去
 * - cat：展开这一类并滚过去；filter 给了就把这一类的搜索框设成它；group 给了就按块图的组过滤
 * - group：块图分组键 `<类别>:<组>`（比如 `tool:Bash`）；给了 cat 时也可以只写组名（`Bash`）
 * - nonce：同样的内容再跳一次时换个值（按内容比较，内容不变不会重复触发）
 */
export interface CtxBrowserNav {
  cat?: Cat
  filter?: string
  nodeId?: string
  group?: string
  nonce?: number
}
</script>

<script setup lang="ts">
// 上下文浏览器：选一次请求，看它带了什么。分类模式按七类折叠，带相对基准请求的增减；
// 平铺模式按模型读取的顺序列出所有项，最新的在上面，上面一条 DNA 色带。块图两种模式都能开。
import { computed, nextTick, reactive, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useChartTheme } from '../../lib/chartTheme'
import {
  ALL_CATS,
  assemble,
  estOf,
  itemsByCat,
  outputsOf,
  type AsstKind,
  type CtxFold,
  type CtxNode,
  type Parts,
} from '../../lib/context'
import { catColor, catLabel, usePref } from '../../lib/ctxView'
import { fmtBig, fmtInt, fmtSigned } from '../../lib/format'
import DashCard from '../dash/DashCard.vue'
import Segmented from '../Segmented.vue'
import CtxBlockMap, { groupKey, groupSub, type MapBlock } from './CtxBlockMap.vue'
import CtxItem from './CtxItem.vue'

const props = withDefaults(
  defineProps<{
    fold: CtxFold
    /** 请求序号；= steps.length 是下一次请求（当前上下文） */
    selected: number
    /** 当前上下文的校准组成（ContextView 已经算好） */
    liveParts: Parts
    /** 外面要求展开的项（趋势简报、文件活动、事件跳过来）；nonce 让同一项能再跳一次 */
    focus: { key: string; nonce: number } | null
    /** 外面要求展开的类别 */
    focusCat: { cat: Cat; nonce: number } | null
    /** 外面的导航意图（可选，见 CtxBrowserNav） */
    nav?: CtxBrowserNav | null
  }>(),
  { nav: null },
)
const emit = defineEmits<{ select: [idx: number] }>()
const { t } = useI18n()
const theme = useChartTheme()

const layout = usePref<'grouped' | 'flat'>('browser.layout', 'grouped')
const toolSort = usePref<'size' | 'name' | 'count'>('browser.toolSort', 'size')
/** 增减徽标的基准：上一次请求，或上一轮的最后一次请求 */
const diffBase = usePref<'step' | 'turn'>('browser.diffBase', 'step')
const showMap = usePref<boolean>('browser.map', true)
/** 跳转临时切到分类模式，不改存下来的偏好；用户自己切一次就作废 */
const forcedLayout = ref<'grouped' | null>(null)
const shownLayout = computed({
  get: () => forcedLayout.value ?? layout.value,
  set: (v: 'grouped' | 'flat') => {
    forcedLayout.value = null
    layout.value = v
  },
})

const live = computed(() => props.selected >= props.fold.steps.length)
const asm = computed(() => assemble(props.fold, props.selected))
const byCat = computed(() => itemsByCat(asm.value))

/** 每类的校准值：选中的请求用它的 anchored，当前上下文按最后一次请求的 prompt 校准 */
const anchored = computed<Parts>(() => (live.value ? props.liveParts : asm.value.step!.anchored))
const est = computed(() => estOf(byCat.value))
const scale = computed(() => {
  const out = {} as Record<Cat, number>
  for (const c of ALL_CATS) out[c] = est.value[c] > 0 ? anchored.value[c] / est.value[c] : 1
  return out
})

/** 增减徽标的基准（快照时记下的条数和估算）：上一次请求，或上一轮的最后一次请求；没有就不比 */
const prev = computed(() => {
  const steps = props.fold.steps
  if (props.selected <= 0) return null
  if (diffBase.value === 'step') return steps[props.selected - 1] ?? null
  // 当前上下文算在最后一次请求那一轮里
  const turn = (steps[props.selected] ?? steps[steps.length - 1])?.turnId
  for (let i = Math.min(props.selected, steps.length) - 1; i >= 0; i--) if (steps[i]!.turnId !== turn) return steps[i]!
  return null
})
const BASE_OPTIONS = computed(() => [
  { value: 'step' as const, label: t('ctxBrowser.base.step') },
  { value: 'turn' as const, label: t('ctxBrowser.base.turn') },
])

/** 增减按估算值比：校准系数每次请求都略有不同，用校准值比会冒出没有内容变化的 ± */
const diffs = computed(() => {
  const out = {} as Record<Cat, { items: number; tokens: number } | null>
  const p = prev.value
  for (const c of ALL_CATS) {
    out[c] = null
    if (!p || c === 'other') continue
    const items = byCat.value[c].length - p.counts[c]
    const tokens = Math.round((est.value[c] - p.parts[c]) * scale.value[c])
    if (items || tokens) out[c] = { items, tokens }
  }
  return out
})

const outputs = computed(() => (asm.value.step ? outputsOf(props.fold, asm.value.step) : []))

// ---- 展开状态 ----
const openCats = reactive(new Set<Cat>())
const openNodes = reactive(new Set<string>())
const filters = reactive<Record<string, string>>({})
const kindFilter = ref<AsstKind | null>(null)
/** 块图点出来的组（groupKey），只过滤它那一类 */
const groupFilter = ref<string | null>(null)
const groupCat = computed(() => (groupFilter.value ? (groupFilter.value.slice(0, groupFilter.value.indexOf(':')) as Cat) : null))
const limits = reactive<Record<string, number>>({})
const PAGE = 100

function toggleCat(c: Cat) {
  if (openCats.has(c)) openCats.delete(c)
  else openCats.add(c)
}
function toggleNode(k: string) {
  if (openNodes.has(k)) openNodes.delete(k)
  else openNodes.add(k)
}

/** 每个工具名在这次请求里有几条结果（按调用次数排序用） */
const resultCounts = computed(() => {
  const m = new Map<string, number>()
  for (const n of asm.value.surface) if (n.cat === 'tool' && n.name) m.set(n.name, (m.get(n.name) ?? 0) + 1)
  return m
})

/** 搜索用的小写全文：名字、摘要（工具定义的描述）、来源、正文、配对调用的参数。第一次搜到才拼，按节点缓存 */
const hayCache = new WeakMap<CtxNode, string>()
function hayOf(n: CtxNode): string {
  let s = hayCache.get(n)
  if (s != null) return s
  const parts = [n.name ?? '', n.label, n.plugin ?? '']
  try {
    parts.push(n.body())
  } catch {
    // 正文拼不出来就只搜摘要
  }
  const args = n.call?.tool?.input
  if (args != null) {
    try {
      parts.push(typeof args === 'string' ? args : JSON.stringify(args))
    } catch {
      // 序列化不了就跳过
    }
  }
  s = parts.join('\n').toLowerCase()
  hayCache.set(n, s)
  return s
}
function textHit(n: CtxNode, q: string): boolean {
  return hayOf(n).includes(q)
}

/** 每类过滤、排序后的列表：工具按选的方式排，消息最新的在上 */
const lists = computed(() => {
  const out = {} as Record<Cat, CtxNode[]>
  for (const c of ALL_CATS) {
    // 收起的类别模板不渲染列表，不用排
    if (!openCats.has(c)) {
      out[c] = byCat.value[c]
      continue
    }
    let list = byCat.value[c]
    const q = (filters[c] ?? '').trim().toLowerCase()
    if (q) list = list.filter((n) => textHit(n, q))
    if (c === 'assistant' && kindFilter.value) list = list.filter((n) => n.kinds?.includes(kindFilter.value!))
    if (c === groupCat.value) list = list.filter((n) => groupKey(n) === groupFilter.value)
    if (c === 'tools') {
      const by = toolSort.value
      list = [...list].sort((a, b) =>
        by === 'name'
          ? (a.name ?? '').localeCompare(b.name ?? '')
          : by === 'count'
            ? (resultCounts.value.get(b.name ?? '') ?? 0) - (resultCounts.value.get(a.name ?? '') ?? 0)
            : b.est - a.est,
      )
    } else if (c !== 'system') list = [...list].reverse()
    out[c] = list
  }
  return out
})

function limitOf(k: string): number {
  return limits[k] ?? PAGE
}
const flatLimit = computed(() => limits.flat ?? PAGE * 2)

const kindCounts = computed(() => {
  const out: Record<AsstKind, number> = { thinking: 0, tools: 0, answer: 0 }
  for (const n of byCat.value.assistant) for (const k of n.kinds ?? []) out[k]++
  return out
})

/** 平铺：system → 工具（一组）→ 消息面按顺序；显示时最新的在上 */
const flat = computed(() => {
  const rows: { node: CtxNode; pos: number }[] = []
  let pos = 1
  if (asm.value.system) rows.push({ node: asm.value.system, pos: pos++ })
  const toolsAt = pos
  pos += asm.value.tools.length ? 1 : 0
  for (const n of asm.value.surface) rows.push({ node: n, pos: pos++ })
  return { rows: rows.reverse(), toolsAt }
})

// ---- DNA 色带 ----

/** 色带的一段：读取顺序 system → 工具定义（合成一段，和平铺列表一致）→ 消息面；宽度按校准后的 token */
interface StripSeg {
  node: CtxNode | null
  cat: Cat
  tokens: number
  pos: number
}
const strip = computed(() => {
  const segs: StripSeg[] = []
  const a = asm.value
  if (a.system) segs.push({ node: a.system, cat: 'system', tokens: a.system.est * scale.value.system, pos: 1 })
  if (a.tools.length) segs.push({ node: null, cat: 'tools', tokens: anchored.value.tools, pos: flat.value.toolsAt })
  let pos = flat.value.toolsAt + (a.tools.length ? 1 : 0)
  for (const n of a.surface) segs.push({ node: n, cat: n.cat, tokens: n.est * scale.value[n.cat], pos: pos++ })
  // 每段起点，悬停时二分找
  const starts: number[] = []
  let total = 0
  for (const s of segs) {
    starts.push(total)
    total += Math.max(0, s.tokens)
  }
  // 画的时候相邻同类合成一条：几千项也只有几十个 rect
  const runs: { cat: Cat; x: number; w: number }[] = []
  segs.forEach((s, i) => {
    const last = runs[runs.length - 1]
    if (last && last.cat === s.cat) last.w += Math.max(0, s.tokens)
    else runs.push({ cat: s.cat, x: starts[i]!, w: Math.max(0, s.tokens) })
  })
  return { segs, starts, total, runs }
})
const stripHover = ref<number | null>(null)
const stripHovered = computed(() => (stripHover.value != null ? (strip.value.segs[stripHover.value] ?? null) : null))

function stripAt(e: MouseEvent): number | null {
  const el = e.currentTarget as Element
  const r = el.getBoundingClientRect()
  const { starts, total, segs } = strip.value
  if (!segs.length || r.width <= 0 || total <= 0) return null
  const x = ((e.clientX - r.left) / r.width) * total
  let lo = 0
  let hi = starts.length - 1
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1
    if (starts[mid]! <= x) lo = mid
    else hi = mid - 1
  }
  return lo
}

function describeSeg(s: StripSeg): string {
  const head = `#${s.pos} · ${catLabel(s.cat)}`
  const tok = `${fmtInt(Math.round(s.tokens))} tok`
  if (!s.node) return [head, t('ctxBrowser.dna.tools', { n: fmtInt(asm.value.tools.length) }), tok].join(' · ')
  return [head, s.node.name, s.node.label, tok].filter(Boolean).join(' · ')
}

/** 点色带：在平铺列表里展开那一项并滚过去 */
async function stripClick(e: MouseEvent) {
  const i = stripAt(e)
  if (i == null) return
  const s = strip.value.segs[i]!
  if (!s.node) {
    // 工具定义那一行在列表最底下，只有全部显示出来才有
    limits.flat = Math.max(flatLimit.value, flat.value.rows.length)
    await nextTick()
    document.getElementById('ctx-flat-tools')?.scrollIntoView({ block: 'nearest', behavior: 'smooth' })
    return
  }
  const idx = flat.value.rows.findIndex((r) => r.node === s.node)
  if (idx >= flatLimit.value) limits.flat = idx + 1
  openNodes.add(s.node.key)
  await nextTick()
  document.getElementById(`ctx-node-${s.node.key}`)?.scrollIntoView({ block: 'nearest', behavior: 'smooth' })
}

// ---- 块图 ----

/** 同类的项合成一块，大小按校准后的 token */
const blocks = computed<MapBlock[]>(() => {
  const m = new Map<string, MapBlock>()
  for (const c of ALL_CATS) {
    for (const n of byCat.value[c]) {
      const sub = groupSub(n)
      const key = `${c}:${sub}`
      let b = m.get(key)
      if (!b) {
        b = { key, cat: c, label: sub || catLabel(c), tokens: 0, count: 0 }
        m.set(key, b)
      }
      b.tokens += n.est * scale.value[c]
      b.count++
    }
  }
  // 「未记录」没有条目，只有校准差
  if (!byCat.value.other.length && anchored.value.other > 0)
    m.set('other:', { key: 'other:', cat: 'other', label: catLabel('other'), tokens: anchored.value.other, count: 0 })
  return [...m.values()]
})

const groupLabel = computed(() => {
  const g = groupFilter.value
  if (!g || !groupCat.value) return ''
  return `${catLabel(groupCat.value)} · ${g.slice(g.indexOf(':') + 1)}`
})

/** 按组过滤：整类一组（键以 : 结尾）等于不过滤 */
function setGroup(key: string | null) {
  groupFilter.value = key && !key.endsWith(':') ? key : null
}

/** 点块图：切到分类、展开那一类、清掉这一类的其他过滤、只看这一组；再点同一块取消 */
async function pickBlock(key: string | null, cat: Cat) {
  if (key != null && key === groupFilter.value) {
    groupFilter.value = null
    return
  }
  forcedLayout.value = 'grouped'
  setGroup(key)
  openCats.add(cat)
  filters[cat] = ''
  if (cat === 'assistant') kindFilter.value = null
  await nextTick()
  document.getElementById(`ctx-cat-${cat}`)?.scrollIntoView({ block: 'nearest', behavior: 'smooth' })
}

const stepOptions = computed(() => [
  ...props.fold.steps.map((s) => ({
    value: s.idx,
    label: s.turnN != null ? t('ctx.browser.stepLabel', { n: s.idx + 1, turn: s.turnN }) : `#${s.idx + 1}`,
  })),
  { value: props.fold.steps.length, label: t('ctx.browser.live') },
])

const LAYOUT_OPTIONS = computed(() => [
  { value: 'grouped' as const, label: t('ctx.browser.grouped'), icon: 'i-jannchie-list' },
  { value: 'flat' as const, label: t('ctx.browser.dna'), icon: 'i-jannchie-dna' },
])
const SORT_OPTIONS = computed(() => [
  { value: 'size' as const, label: t('ctx.browser.sortSize') },
  { value: 'name' as const, label: t('ctx.browser.sortName') },
  { value: 'count' as const, label: t('ctx.browser.sortCount') },
])

const visibleCats = computed(() => ALL_CATS.filter((c) => byCat.value[c].length || anchored.value[c] > 0))

// ---- 外面的跳转 ----

/** 聚焦某一项：切到分类模式、展开它的类别、清掉挡住它的过滤、展开它、滚过去 */
async function focusNode(key: string) {
  const node = [...asm.value.surface, ...asm.value.tools, ...(asm.value.system ? [asm.value.system] : []), ...outputs.value].find(
    (n) => n.key === key,
  )
  if (!node) return
  forcedLayout.value = 'grouped'
  if (!outputs.value.includes(node)) {
    openCats.add(node.cat)
    const q = (filters[node.cat] ?? '').trim().toLowerCase()
    if (q && !textHit(node, q)) filters[node.cat] = ''
    if (node.cat === 'assistant' && kindFilter.value && !node.kinds?.includes(kindFilter.value)) kindFilter.value = null
    if (groupCat.value === node.cat && groupKey(node) !== groupFilter.value) groupFilter.value = null
    const i = lists.value[node.cat].indexOf(node)
    if (i >= limitOf(node.cat)) limits[node.cat] = i + 1
  }
  openNodes.add(node.key)
  await nextTick()
  document.getElementById(`ctx-node-${node.key}`)?.scrollIntoView({ block: 'center', behavior: 'smooth' })
}

/** 展开某一类并滚过去 */
async function focusCategory(cat: Cat) {
  forcedLayout.value = 'grouped'
  openCats.add(cat)
  await nextTick()
  document.getElementById(`ctx-cat-${cat}`)?.scrollIntoView({ block: 'start', behavior: 'smooth' })
}

watch(
  () => props.focus?.nonce,
  () => {
    if (props.focus) void focusNode(props.focus.key)
  },
)
watch(
  () => props.focusCat?.nonce,
  () => {
    if (props.focusCat) void focusCategory(props.focusCat.cat)
  },
)
// 导航意图按内容比：父组件每次渲染新建同样内容的对象不会重复触发，要再跳一次就换 nonce
watch(
  () => (props.nav ? JSON.stringify(props.nav) : ''),
  () => {
    const v = props.nav
    if (!v) return
    if (v.nodeId) {
      void focusNode(v.nodeId)
      return
    }
    let cat = v.cat
    let group = v.group
    if (group != null) {
      if (cat) {
        if (!group.startsWith(`${cat}:`)) group = `${cat}:${group}`
      } else if (group.includes(':')) {
        cat = group.slice(0, group.indexOf(':')) as Cat
      }
    }
    if (!cat || !ALL_CATS.includes(cat)) return
    if (v.filter != null) {
      filters[cat] = v.filter
      if (cat === 'assistant') kindFilter.value = null
    }
    if (group != null) setGroup(group)
    else if (groupCat.value === cat) groupFilter.value = null
    void focusCategory(cat)
  },
)
</script>

<template>
  <DashCard icon="i-jannchie-search-list" :title="t('ctx.browser.title')" :caption="t('ctx.browser.caption')">
    <template #controls>
      <button
        type="button"
        class="icon-button-sm"
        :class="showMap ? 'pill-selected' : ''"
        :aria-pressed="showMap"
        :aria-label="t('ctxBrowser.map.toggle')"
        :title="t('ctxBrowser.map.tip')"
        @click="showMap = !showMap"
      >
        <i class="i-jannchie-chart-treemap h-4 w-4" />
      </button>
      <Segmented v-model="shownLayout" :label="t('ctx.browser.title')" :options="LAYOUT_OPTIONS" />
    </template>

    <div class="flex flex-col gap-3">
      <!-- 选哪一次请求 -->
      <div class="flex items-center gap-1.5">
        <button type="button" class="icon-button-sm shrink-0" :disabled="selected <= 0" :aria-label="t('ctx.browser.prev')" :title="t('ctx.browser.prev')" @click="emit('select', selected - 1)">
          <i class="i-jannchie-chevron-left h-4 w-4" />
        </button>
        <select
          class="field min-w-0 flex-1 py-1 text-xs"
          :value="selected"
          :aria-label="t('ctx.browser.title')"
          @change="emit('select', Number(($event.target as HTMLSelectElement).value))"
        >
          <option v-for="o in stepOptions" :key="o.value" :value="o.value">{{ o.label }}</option>
        </select>
        <button type="button" class="icon-button-sm shrink-0" :disabled="live" :aria-label="t('ctx.browser.next')" :title="t('ctx.browser.next')" @click="emit('select', selected + 1)">
          <i class="i-jannchie-chevron-right h-4 w-4" />
        </button>
      </div>
      <div v-if="asm.step" class="flex flex-wrap gap-x-3 gap-y-1 text-xs ink-soft">
        <span v-if="asm.step.model" class="font-mono">{{ asm.step.model }}</span>
        <span v-if="asm.step.prompt != null" class="num">{{ t('ctx.trend.prompt') }} {{ fmtInt(asm.step.prompt) }}</span>
        <span v-if="asm.step.usage" class="num">{{ t('ctx.trend.output') }} {{ fmtInt(asm.step.usage.output) }}</span>
      </div>
      <!-- 增减徽标的基准 -->
      <div v-if="selected > 0" class="flex flex-wrap items-center gap-2 text-xs ink-soft" :title="t('ctxBrowser.base.tip')">
        <Segmented v-model="diffBase" :label="t('ctxBrowser.base.label')" :options="BASE_OPTIONS" />
        <span class="num">{{ prev ? t('ctxBrowser.base.vs', { n: prev.idx + 1 }) : t('ctxBrowser.base.none') }}</span>
      </div>
      <p v-if="asm.step?.approximate" class="m-0 rounded-lg px-3 py-2 text-xs ink-warning surface">{{ t('ctx.browser.approximate') }}</p>

      <!-- 块图 -->
      <CtxBlockMap v-if="showMap" :blocks="blocks" :active="groupFilter" @pick="pickBlock" />
      <div v-if="groupFilter && groupCat && shownLayout === 'grouped'" class="flex items-center gap-2 text-xs">
        <span class="h-2 w-2 shrink-0 rounded-full" :style="{ background: catColor(groupCat, theme) }" />
        <span class="min-w-0 truncate ink-muted">{{ t('ctxBrowser.map.only', { name: groupLabel }) }}</span>
        <button type="button" class="inline-flex items-center gap-1 rounded border-0 px-1.5 py-0.5 text-xs surface surface-hover kbd-ring" @click="groupFilter = null">
          <i class="i-jannchie-x h-3 w-3" aria-hidden="true" />{{ t('ctxBrowser.map.clear') }}
        </button>
      </div>

      <!-- 分类 -->
      <ul v-if="shownLayout === 'grouped'" class="m-0 flex flex-col gap-1 p-0">
        <li v-for="c in visibleCats" :id="`ctx-cat-${c}`" :key="c" class="list-none">
          <button
            type="button"
            class="w-full flex items-center gap-2 rounded-lg border-0 bg-transparent px-2 py-1.5 text-left text-sm surface-hover kbd-ring"
            :aria-expanded="openCats.has(c)"
            :title="c === 'other' ? t('ctx.catHint.other') : undefined"
            @click="toggleCat(c)"
          >
            <i class="h-3.5 w-3.5 shrink-0 ink-soft" :class="openCats.has(c) ? 'i-jannchie-chevron-down' : 'i-jannchie-chevron-right'" aria-hidden="true" />
            <span class="h-2.5 w-2.5 shrink-0 rounded-full" :style="{ background: catColor(c, theme) }" />
            <span class="min-w-0 flex-1 truncate">{{ catLabel(c) }}</span>
            <span v-if="byCat[c].length" class="text-xs ink-soft num">{{ t('ctx.browser.items', { n: fmtInt(byCat[c].length) }, byCat[c].length) }}</span>
            <span v-if="diffs[c]" class="rounded px-1.5 text-[11px] num surface" :class="diffs[c]!.tokens > 0 ? 'ink-warning' : 'ink-success'">
              <template v-if="diffs[c]!.items">{{ diffs[c]!.items > 0 ? '+' : '−' }}{{ Math.abs(diffs[c]!.items) }} · </template>{{ fmtSigned(diffs[c]!.tokens, fmtBig) }}
            </span>
            <span class="w-16 text-right num" :title="fmtInt(anchored[c])">{{ fmtBig(anchored[c]) }}</span>
          </button>

          <div v-if="openCats.has(c)" class="ml-5 mt-1 mb-2 flex flex-col gap-1.5">
            <div v-if="byCat[c].length > 6 || c === 'assistant' || c === 'tools' || c === groupCat" class="flex flex-wrap items-center gap-2">
              <input v-model="filters[c]" type="search" class="field max-w-64 py-1 text-xs" :placeholder="t('ctxBrowser.filter')" :aria-label="t('ctx.browser.filter')" />
              <Segmented v-if="c === 'tools'" v-model="toolSort" :label="t('ctx.browser.sortSize')" :options="SORT_OPTIONS" />
              <template v-if="c === 'assistant'">
                <button
                  v-for="k in (['thinking', 'tools', 'answer'] as const)"
                  :key="k"
                  type="button"
                  class="segment border-0"
                  :class="kindFilter === k ? 'pill-selected' : 'surface ink-muted'"
                  :aria-pressed="kindFilter === k"
                  @click="kindFilter = kindFilter === k ? null : k"
                >
                  {{ t(`ctx.browser.kinds.${k}`) }} <span class="num">{{ kindCounts[k] }}</span>
                </button>
              </template>
              <button
                v-if="c === groupCat"
                type="button"
                class="inline-flex items-center gap-1 rounded border-0 px-1.5 py-0.5 text-xs pill-selected kbd-ring"
                :title="t('ctxBrowser.map.clear')"
                @click="groupFilter = null"
              >
                {{ t('ctxBrowser.map.only', { name: groupLabel }) }}<i class="i-jannchie-x h-3 w-3" aria-hidden="true" />
              </button>
            </div>
            <p v-if="!byCat[c].length" class="m-0 px-2 text-xs ink-soft">{{ c === 'other' ? t('ctx.catHint.other') : t('ctx.browser.noItems') }}</p>
            <ul class="m-0 flex flex-col gap-px p-0">
              <CtxItem
                v-for="n in lists[c].slice(0, limitOf(c))"
                :key="n.key"
                :node="n"
                :scale="scale[c]"
                :open="openNodes.has(n.key)"
                @toggle="toggleNode(n.key)"
              />
            </ul>
            <button
              v-if="lists[c].length > limitOf(c)"
              type="button"
              class="self-start rounded-md border-0 px-2 py-1 text-xs surface surface-hover kbd-ring"
              @click="limits[c] = limitOf(c) + PAGE"
            >
              {{ t('ctx.events.more', { n: fmtInt(Math.min(PAGE, lists[c].length - limitOf(c))) }) }}
            </button>
          </div>
        </li>

        <!-- 这次请求的输出 -->
        <li v-if="outputs.length" class="mt-2 list-none">
          <div class="px-2 pb-1 text-xs font-medium ink-soft">{{ t('ctx.browser.output') }}</div>
          <ul class="m-0 flex flex-col gap-px p-0">
            <CtxItem v-for="n in outputs" :key="n.key" :node="n" :scale="1" :open="openNodes.has(n.key)" @toggle="toggleNode(n.key)" />
          </ul>
        </li>
      </ul>

      <!-- 平铺 -->
      <div v-else class="flex flex-col gap-1">
        <p class="m-0 text-xs ink-soft">{{ t('ctx.browser.dnaHint') }}</p>
        <!-- DNA 色带：读取顺序从左到右，悬停看是哪一项，点击展开 -->
        <template v-if="strip.total > 0">
          <svg
            class="block h-5 w-full cursor-pointer rounded"
            :viewBox="`0 0 ${strip.total} 1`"
            preserveAspectRatio="none"
            role="img"
            :aria-label="t('ctxBrowser.dna.aria')"
            @mousemove="stripHover = stripAt($event)"
            @mouseleave="stripHover = null"
            @click="stripClick($event)"
          >
            <rect v-for="(r, i) in strip.runs" :key="i" :x="r.x" y="0" :width="r.w" height="1" :fill="catColor(r.cat, theme)" />
            <rect
              v-if="stripHovered && stripHover != null"
              :x="strip.starts[stripHover]"
              y="0"
              :width="Math.max(stripHovered.tokens, strip.total / 400)"
              height="1"
              :fill="theme.fg"
              fill-opacity="0.45"
            />
          </svg>
          <p class="m-0 min-h-4 truncate text-xs ink-soft num">{{ stripHovered ? describeSeg(stripHovered) : t('ctxBrowser.dna.hint') }}</p>
        </template>
        <ul class="m-0 flex max-h-[40rem] flex-col gap-px overflow-y-auto p-0">
          <CtxItem
            v-for="r in flat.rows.slice(0, flatLimit)"
            :key="r.node.key"
            :node="r.node"
            :scale="scale[r.node.cat]"
            :open="openNodes.has(r.node.key)"
            :position="r.pos"
            show-cat
            @toggle="toggleNode(r.node.key)"
          />
          <li v-if="asm.tools.length && flat.rows.length <= flatLimit" id="ctx-flat-tools" class="list-none">
            <div class="flex items-center gap-2 px-2 py-1 text-xs">
              <span class="w-3" />
              <span class="w-9 text-right num ink-soft">#{{ flat.toolsAt }}</span>
              <span class="h-2 w-2 rounded-full" :style="{ background: catColor('tools', theme) }" />
              <span class="min-w-0 flex-1 truncate ink-muted">
                {{ catLabel('tools') }} · {{ asm.tools.map((n) => n.name).slice(0, 12).join(', ') }}<template v-if="asm.tools.length > 12">…</template>
              </span>
              <span class="w-14 text-right num">{{ fmtBig(anchored.tools) }}</span>
            </div>
          </li>
        </ul>
        <button
          v-if="flat.rows.length > flatLimit"
          type="button"
          class="self-start rounded-md border-0 px-2 py-1 text-xs surface surface-hover kbd-ring"
          @click="limits.flat = flatLimit + PAGE * 2"
        >
          {{ t('ctx.events.more', { n: fmtInt(Math.min(PAGE * 2, flat.rows.length - flatLimit)) }) }}
        </button>
      </div>
    </div>
  </DashCard>
</template>
