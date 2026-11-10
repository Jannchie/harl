<script setup lang="ts">
// 会话页的「上下文」视图：把事件流折成每次请求的上下文，再分成几张卡。
// 选中的请求是各卡共享的：趋势图点一根柱子，浏览器跳过去，文件活动也只算到那里。
// 概况卡的计数、药丸点了发 nav 意图，这里交给对应的卡（浏览器 / 文件活动 / 上下文事件 / 命令执行 / Agent 网络）。
import { computed, nextTick, onMounted, ref, shallowRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import type { Event, SessionInfo } from '../../api'
import { currentParts, foldContext, type Cat, type CtxEvent, type CtxEventKind, type CtxNode } from '../../lib/context'
import { summarize, type CtxNav, type FamilyStats } from '../../lib/ctxView'
import { commandLog } from '../../lib/commands'
import { collectFileOps, type FileOp } from '../../lib/fileOps'
import { ensurePricing } from '../../lib/pricing'
import CtxBrowser, { type CtxBrowserNav } from './CtxBrowser.vue'
import CtxCommands from './CtxCommands.vue'
import CtxCurrent from './CtxCurrent.vue'
import CtxEvents from './CtxEvents.vue'
import CtxFiles from './CtxFiles.vue'
import CtxFleet from './CtxFleet.vue'
import CtxStats from './CtxStats.vue'
import CtxTrend from './CtxTrend.vue'

const props = defineProps<{
  events: Event[]
  info: SessionInfo | null
  /** 从对话里某一轮跳过来：选中这一轮的最后一次请求 */
  turn?: string | null
}>()
const { t } = useI18n()

const fold = computed(() => foldContext(props.events))
const ops = computed(() => collectFileOps(props.events, fold.value.calls))
const current = computed(() => currentParts(fold.value))
/** 命令执行：概况卡的命令条和命令执行卡共用一份 */
const cmdLog = computed(() => commandLog(props.events))

// 价格目录拉到之后重算一次费用（之前用的是内置快照）
const priced = ref(0)
onMounted(() => void ensurePricing().then(() => priced.value++))
const summary = computed(() => {
  void priced.value
  return summarize(fold.value)
})
/**
 * 文件活动、命令执行、Agent 网络在首屏下面：晚一个任务再挂，大会话第一次渲染不用一口气把所有卡都画完
 * （折叠和概况要的数照样先算好，只是这几张卡的 DOM 晚一点）
 */
const lower = ref(false)
onMounted(() => setTimeout(() => (lower.value = true)))
/** Agent 网络算出来的：当前会话 + 子孙的费用、子孙（不含自己）的合计 */
const familyCost = ref<number | null>(null)
const family = shallowRef<FamilyStats | null>(null)

/** 选中的请求；= steps.length 是当前上下文 */
const selected = ref(Number.POSITIVE_INFINITY)
const sel = computed(() => Math.min(selected.value, fold.value.steps.length))
/** 趋势图悬停预览的请求：浏览器临时看它，移开恢复 sel */
const preview = ref<number | null>(null)
const browserSel = computed(() => (preview.value != null ? Math.max(0, Math.min(preview.value, fold.value.steps.length)) : sel.value))
const focus = shallowRef<{ key: string; nonce: number } | null>(null)
const focusCat = shallowRef<{ cat: Cat; nonce: number } | null>(null)
let nonce = 0

function select(i: number) {
  selected.value = Math.max(0, Math.min(i, fold.value.steps.length))
}

/** seq 之后的第一次请求（某一项第一次出现在它的上下文里）；没有就是 steps.length */
function stepAfter(seq: number): number {
  const steps = fold.value.steps
  let lo = 0
  let hi = steps.length
  while (lo < hi) {
    const mid = (lo + hi) >> 1
    if (steps[mid]!.seq > seq) hi = mid
    else lo = mid + 1
  }
  return lo
}

/** 输出节点在产生它的那次请求里看（浏览器底部的「本次输出」），其余在它进入上下文之后的那次请求里看 */
function focusNode(n: CtxNode) {
  select(n.cat === 'assistant' ? n.step : stepAfter(n.seq))
  focus.value = { key: n.key, nonce: ++nonce }
}

function nodeByKey(key: string): CtxNode | undefined {
  return fold.value.nodes.find((n) => n.key === key)
}

function pickEvent(e: CtxEvent) {
  if (e.node) focusNode(e.node)
  else select(e.step)
}

function jumpOp(o: FileOp) {
  const node = nodeByKey(o.result ? `n${o.result.seq}` : `n${o.call.seq}`)
  if (node) focusNode(node)
}

/** 命令执行卡的「定位」：seq 是结果行（没有结果就是调用行）；节点没折出来就只选中它之后的那次请求 */
function revealCmd(seq: number) {
  const node = nodeByKey(`n${seq}`)
  if (node) focusNode(node)
  else select(stepAfter(seq))
}

function pickCat(c: Cat) {
  select(fold.value.steps.length)
  focusCat.value = { cat: c, nonce: ++nonce }
}

// ---- 趋势图 ----

/** 总览里固定了一条类别带：在浏览器里展开这一类（选中哪次请求趋势图另外发 select） */
function trendCat(c: Cat) {
  focusCat.value = { cat: c, nonce: ++nonce }
}

/** DNA 里点了一项：打开那次请求、定位到这一项（不走 focusNode，那会跳到它第一次出现的请求） */
function locate(key: string, idx: number) {
  select(idx)
  focus.value = { key, nonce: ++nonce }
}

// ---- 概况卡发来的跳转意图 ----
const browserCard = ref<InstanceType<typeof CtxBrowser> | null>(null)
const eventsCard = ref<InstanceType<typeof CtxEvents> | null>(null)
const filesCard = ref<InstanceType<typeof CtxFiles> | null>(null)
const cmdsCard = ref<InstanceType<typeof CtxCommands> | null>(null)
const fleetCard = ref<InstanceType<typeof CtxFleet> | null>(null)

const browserNav = shallowRef<CtxBrowserNav | null>(null)
const filesOp = shallowRef<{ op: FileOp['kind'] | 'image'; nonce: number } | null>(null)
const eventKind = shallowRef<{ kind: CtxEventKind; nonce: number } | null>(null)
const cmdNav = shallowRef<{ program?: string; filter?: string; nonce: number } | null>(null)

function scrollTo(card: { $el?: unknown } | null) {
  const el = card?.$el
  if (el instanceof HTMLElement) el.scrollIntoView({ block: 'start', behavior: 'smooth' })
}

function onNav(nav: CtxNav) {
  switch (nav.kind) {
    case 'browser': {
      const node = nav.nodeId ? nodeByKey(nav.nodeId) : undefined
      if (node) focusNode(node)
      else if (nav.cat) {
        // 概况是整场的数，看当前上下文；浏览器自己展开、套上搜索词、滚过去
        select(fold.value.steps.length)
        browserNav.value = { cat: nav.cat, filter: nav.filter ?? '', nonce: ++nonce }
      }
      break
    }
    case 'files':
      if (nav.op) filesOp.value = { op: nav.op, nonce: ++nonce }
      // 截止到某次请求时文件数和概况对不上，跳过来看整场
      select(fold.value.steps.length)
      scrollTo(filesCard.value)
      break
    case 'events':
      if (nav.eventKind) eventKind.value = { kind: nav.eventKind, nonce: ++nonce }
      scrollTo(eventsCard.value)
      break
    case 'commands':
      cmdNav.value = { program: nav.program, filter: nav.filter, nonce: ++nonce }
      scrollTo(cmdsCard.value)
      break
    case 'fleet':
      scrollTo(fleetCard.value)
      break
  }
}

/** op 的 seq → 所在的请求序号（它的调用由哪次请求发出） */
function stepOf(seq: number): number {
  return Math.max(0, stepAfter(seq) - 1)
}

const upTo = computed(() => (sel.value < fold.value.steps.length ? (fold.value.steps[sel.value]?.seq ?? null) : null))

// ---- 从对话跳过来：选中那一轮的最后一次请求，滚到浏览器 ----
let appliedTurn: string | null = null
watch(
  [() => props.turn, fold],
  async ([id]) => {
    if (!id || appliedTurn === id) return
    const turn = fold.value.turns.find((x) => x.id === id)
    if (!turn) return
    appliedTurn = id
    select(turn.last)
    await nextTick()
    scrollTo(browserCard.value)
  },
  { immediate: true },
)
</script>

<template>
  <p v-if="!fold.steps.length" class="py-12 text-center text-sm ink-soft">{{ t('ctx.noSteps') }}</p>
  <div v-else class="flex flex-col gap-4 pt-2">
    <CtxStats :s="summary" :family-cost="familyCost" :family="family" :ops="ops" :programs="cmdLog.programs" @nav="onNav" />
    <CtxCurrent
      :parts="current.parts"
      :tokens="current.tokens"
      :estimated="current.est"
      :window="fold.window"
      :max-output="fold.maxOutput"
      :model="fold.steps.at(-1)?.model ?? null"
      :provider="info?.provider"
      @pick="pickCat"
    />
    <CtxTrend
      :fold="fold"
      :selected="sel"
      @select="select"
      @focus="focusNode"
      @preview="(i) => (preview = i)"
      @focus-cat="trendCat"
      @locate="locate"
    />
    <div class="grid gap-4 xl:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]">
      <CtxBrowser
        ref="browserCard"
        :fold="fold"
        :selected="browserSel"
        :live-parts="current.parts"
        :focus="focus"
        :focus-cat="focusCat"
        :nav="browserNav"
        @select="select"
      />
      <CtxEvents ref="eventsCard" :fold="fold" :event-kind="eventKind" class="self-start" @pick="pickEvent" />
    </div>
    <template v-if="lower">
      <CtxFiles
        ref="filesCard"
        :ops="ops"
        :up-to="upTo"
        :up-to-step="upTo != null ? sel : null"
        :step-of="stepOf"
        :op="filesOp"
        @jump="jumpOp"
        @clear-up-to="select(fold.steps.length)"
      />
      <CtxCommands ref="cmdsCard" :events="events" :log="cmdLog" :step-of="stepOf" :nav="cmdNav" @reveal="revealCmd" />
      <CtxFleet
        v-if="info"
        ref="fleetCard"
        :info="info"
        :fold="fold"
        :summary="summary"
        :events="events"
        @family="(c) => (familyCost = c)"
        @family-stats="(f) => (family = f)"
      />
    </template>
  </div>
</template>
