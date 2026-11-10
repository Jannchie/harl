<script setup lang="ts">
// 上下文浏览器：选一次请求，看它带了什么。分类模式按七类折叠，带相对上一次请求的增减；
// 平铺模式按模型读取的顺序列出所有项，最新的在上面。
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
  type Cat,
  type CtxFold,
  type CtxNode,
  type Parts,
} from '../../lib/context'
import { catColor, catLabel, usePref } from '../../lib/ctxView'
import { fmtBig, fmtInt, fmtSigned } from '../../lib/format'
import DashCard from '../dash/DashCard.vue'
import Segmented from '../Segmented.vue'
import CtxItem from './CtxItem.vue'

const props = defineProps<{
  fold: CtxFold
  /** 请求序号；= steps.length 是下一次请求（当前上下文） */
  selected: number
  /** 当前上下文的校准组成（ContextView 已经算好） */
  liveParts: Parts
  /** 外面要求展开的项（趋势简报、文件活动、事件跳过来）；nonce 让同一项能再跳一次 */
  focus: { key: string; nonce: number } | null
  /** 外面要求展开的类别 */
  focusCat: { cat: Cat; nonce: number } | null
}>()
const emit = defineEmits<{ select: [idx: number] }>()
const { t } = useI18n()
const theme = useChartTheme()

const layout = usePref<'grouped' | 'flat'>('browser.layout', 'grouped')
const toolSort = usePref<'size' | 'name' | 'count'>('browser.toolSort', 'size')
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

/** 上一次请求：增减徽标的基准（快照时记下的条数和估算） */
const prev = computed(() => (props.selected > 0 ? (props.fold.steps[props.selected - 1] ?? null) : null))

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

function textHit(n: CtxNode, q: string): boolean {
  return (n.name ?? '').toLowerCase().includes(q) || n.label.toLowerCase().includes(q)
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

// 外面要求聚焦某一项：切到分类模式、展开它的类别、清掉挡住它的过滤、展开它、滚过去
watch(
  () => props.focus?.nonce,
  async () => {
    const f = props.focus
    if (!f) return
    const node = [...asm.value.surface, ...asm.value.tools, ...(asm.value.system ? [asm.value.system] : []), ...outputs.value].find(
      (n) => n.key === f.key,
    )
    if (!node) return
    forcedLayout.value = 'grouped'
    if (!outputs.value.includes(node)) {
      openCats.add(node.cat)
      const q = (filters[node.cat] ?? '').trim().toLowerCase()
      if (q && !textHit(node, q)) filters[node.cat] = ''
      if (node.cat === 'assistant' && kindFilter.value && !node.kinds?.includes(kindFilter.value)) kindFilter.value = null
      const i = lists.value[node.cat].indexOf(node)
      if (i >= limitOf(node.cat)) limits[node.cat] = i + 1
    }
    openNodes.add(node.key)
    await nextTick()
    document.getElementById(`ctx-node-${node.key}`)?.scrollIntoView({ block: 'center', behavior: 'smooth' })
  },
)
watch(
  () => props.focusCat?.nonce,
  async () => {
    const f = props.focusCat
    if (!f) return
    forcedLayout.value = 'grouped'
    openCats.add(f.cat)
    await nextTick()
    document.getElementById(`ctx-cat-${f.cat}`)?.scrollIntoView({ block: 'start', behavior: 'smooth' })
  },
)

</script>

<template>
  <DashCard icon="i-jannchie-search-list" :title="t('ctx.browser.title')" :caption="t('ctx.browser.caption')">
    <template #controls>
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
        <span v-if="prev">{{ t('ctx.browser.vsPrev') }}</span>
      </div>
      <p v-if="asm.step?.approximate" class="m-0 rounded-lg px-3 py-2 text-xs ink-warning surface">{{ t('ctx.browser.approximate') }}</p>

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
            <div v-if="byCat[c].length > 6 || c === 'assistant' || c === 'tools'" class="flex flex-wrap items-center gap-2">
              <input v-model="filters[c]" type="search" class="field max-w-64 py-1 text-xs" :placeholder="t('ctx.browser.filter')" :aria-label="t('ctx.browser.filter')" />
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
          <li v-if="asm.tools.length && flat.rows.length <= flatLimit" class="list-none">
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
