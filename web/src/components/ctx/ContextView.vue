<script setup lang="ts">
// 会话页的「上下文」视图：把事件流折成每次请求的上下文，再分成几张卡。
// 选中的请求是各卡共享的：趋势图点一根柱子，浏览器跳过去，文件活动也只算到那里。
import { computed, onMounted, ref, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import type { Event, SessionInfo } from '../../api'
import { currentParts, foldContext, type Cat, type CtxEvent, type CtxNode } from '../../lib/context'
import { summarize } from '../../lib/ctxView'
import { collectFileOps, type FileOp } from '../../lib/fileOps'
import { ensurePricing } from '../../lib/pricing'
import CtxBrowser from './CtxBrowser.vue'
import CtxCurrent from './CtxCurrent.vue'
import CtxEvents from './CtxEvents.vue'
import CtxFiles from './CtxFiles.vue'
import CtxFleet from './CtxFleet.vue'
import CtxStats from './CtxStats.vue'
import CtxTrend from './CtxTrend.vue'

const props = defineProps<{ events: Event[]; info: SessionInfo | null }>()
const { t } = useI18n()

const fold = computed(() => foldContext(props.events))
const ops = computed(() => collectFileOps(props.events, fold.value.calls))
const current = computed(() => currentParts(fold.value))

// 价格目录拉到之后重算一次费用（之前用的是内置快照）
const priced = ref(0)
onMounted(() => void ensurePricing().then(() => priced.value++))
const summary = computed(() => {
  void priced.value
  return summarize(fold.value)
})
const familyCost = ref<number | null>(null)

/** 选中的请求；= steps.length 是当前上下文 */
const selected = ref(Number.POSITIVE_INFINITY)
const sel = computed(() => Math.min(selected.value, fold.value.steps.length))
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

function pickEvent(e: CtxEvent) {
  if (e.node) focusNode(e.node)
  else select(e.step)
}

function jumpOp(o: FileOp) {
  const key = o.result ? `n${o.result.seq}` : `n${o.call.seq}`
  const node = fold.value.nodes.find((n) => n.key === key)
  if (node) focusNode(node)
}

function pickCat(c: Cat) {
  select(fold.value.steps.length)
  focusCat.value = { cat: c, nonce: ++nonce }
}

/** op 的 seq → 所在的请求序号（它的调用由哪次请求发出） */
function stepOf(seq: number): number {
  return Math.max(0, stepAfter(seq) - 1)
}

const upTo = computed(() => (sel.value < fold.value.steps.length ? (fold.value.steps[sel.value]?.seq ?? null) : null))
</script>

<template>
  <p v-if="!fold.steps.length" class="py-12 text-center text-sm ink-soft">{{ t('ctx.noSteps') }}</p>
  <div v-else class="flex flex-col gap-4 pt-2">
    <CtxStats :s="summary" :human-inputs="fold.humanInputs" :answers="fold.answers" :family-cost="familyCost" />
    <CtxCurrent
      :parts="current.parts"
      :tokens="current.tokens"
      :estimated="current.est"
      :window="fold.window"
      :max-output="fold.maxOutput"
      @pick="pickCat"
    />
    <CtxTrend :fold="fold" :selected="sel" @select="select" @focus="focusNode" />
    <div class="grid gap-4 xl:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]">
      <CtxBrowser :fold="fold" :selected="sel" :live-parts="current.parts" :focus="focus" :focus-cat="focusCat" @select="select" />
      <CtxEvents :fold="fold" class="self-start" @pick="pickEvent" />
    </div>
    <CtxFiles
      :ops="ops"
      :up-to="upTo"
      :up-to-step="upTo != null ? sel : null"
      :step-of="stepOf"
      @jump="jumpOp"
      @clear-up-to="select(fold.steps.length)"
    />
    <CtxFleet v-if="info" :info="info" :fold="fold" :summary="summary" @family="(c) => (familyCost = c)" />
  </div>
</template>
