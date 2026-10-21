<script setup lang="ts">
// 上下文浏览器里的一项：一行摘要，展开看原文。工具结果展开成完整的一次调用（名字、参数、状态、结果）。
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { RouterLink, useRoute } from 'vue-router'
import { useChartTheme } from '../../lib/chartTheme'
import { toolSource, type CtxNode } from '../../lib/context'
import { catColor, catLabel } from '../../lib/ctxView'
import { fmtBig, fmtBytes, fmtInt } from '../../lib/format'
import JsonBlock from '../JsonBlock.vue'
import MarkdownText from '../MarkdownText.vue'

const props = defineProps<{
  node: CtxNode
  /** 估算 → 校准后的系数（这一类的校准值 ÷ 估算值） */
  scale: number
  open: boolean
  /** 平铺模式下的读取位置 */
  position?: number
  /** 平铺模式下标出类别 */
  showCat?: boolean
}>()
const emit = defineEmits<{ toggle: [] }>()
const { t } = useI18n()
const theme = useChartTheme()
const route = useRoute()

const LIMIT = 20000
const full = ref(false)
const asMarkdown = ref(props.node.bodyKind === 'markdown')

const tokens = computed(() => Math.round(props.node.est * props.scale))
const body = computed(() => (props.open ? props.node.body() : ''))
const shown = computed(() => (full.value || body.value.length <= LIMIT ? body.value : body.value.slice(0, LIMIT)))
const lines = computed(() => {
  const b = body.value
  if (!b) return 0
  let n = 1
  for (let i = b.indexOf('\n'); i !== -1; i = b.indexOf('\n', i + 1)) n++
  return n
})
const source = computed(() => (props.node.cat === 'tools' ? toolSource(props.node) : null))
const isResult = computed(() => props.node.ev.kind === 'tool_result')
const turnHash = computed(() => (props.node.ev.turn ? `#turn-t:${props.node.ev.turn.id}` : undefined))
</script>

<template>
  <li :id="`ctx-node-${node.key}`" class="list-none">
    <button
      type="button"
      class="w-full flex items-center gap-2 rounded-lg border-0 bg-transparent px-2 py-1 text-left text-xs surface-hover kbd-ring"
      :class="open ? 'surface-strong' : ''"
      :aria-expanded="open"
      @click="emit('toggle')"
    >
      <i class="h-3 w-3 shrink-0 ink-soft" :class="open ? 'i-jannchie-chevron-down' : 'i-jannchie-chevron-right'" aria-hidden="true" />
      <span v-if="position != null" class="w-9 shrink-0 text-right num ink-soft">#{{ position }}</span>
      <span v-if="showCat" class="h-2 w-2 shrink-0 rounded-full" :style="{ background: catColor(node.cat, theme) }" :title="catLabel(node.cat)" />
      <span v-if="node.name" class="max-w-48 shrink-0 truncate font-mono ink-strong">{{ node.name }}</span>
      <span v-if="source" class="shrink-0 rounded px-1 text-[11px] ink-soft surface">{{ source }}</span>
      <span v-for="k in node.kinds ?? []" :key="k" class="shrink-0 rounded px-1 text-[11px] ink-soft surface">{{ t(`ctx.browser.kinds.${k}`) }}</span>
      <span v-if="node.err" class="shrink-0 rounded px-1 text-[11px] ink-danger surface">{{ t('ctx.browser.error') }}</span>
      <i v-if="node.images?.length" class="i-jannchie-file-image h-3.5 w-3.5 shrink-0 ink-soft" aria-hidden="true" />
      <span class="min-w-0 flex-1 truncate ink-muted">{{ node.label }}</span>
      <span class="shrink-0 ink-soft num" :title="t('ctx.browser.introduced', { n: node.step + 1 })">#{{ node.step + 1 }}</span>
      <span class="w-14 shrink-0 text-right num" :title="fmtInt(tokens)">{{ fmtBig(tokens) }}</span>
    </button>

    <div v-if="open" class="ml-5 mt-1 mb-2 flex flex-col gap-2 border-l border-line pl-3 text-xs">
      <div class="flex flex-wrap items-center gap-x-3 gap-y-1 ink-soft">
        <span>{{ t('ctx.browser.introduced', { n: node.step + 1 }) }}</span>
        <span class="num">≈{{ fmtInt(node.est) }} tok</span>
        <span v-if="lines" class="num">{{ t('ctx.browser.lines', { n: fmtInt(lines) }, lines) }}</span>
        <span v-if="isResult" :class="node.err ? 'ink-danger' : 'ink-success'">{{ node.err ? t('ctx.browser.error') : t('ctx.browser.ok') }}</span>
        <span v-if="source">{{ t('ctx.browser.source') }} {{ source }}</span>
        <button
          v-if="node.bodyKind !== 'json'"
          type="button"
          class="rounded border-0 px-1.5 py-0.5 text-xs surface surface-hover kbd-ring"
          @click="asMarkdown = !asMarkdown"
        >
          {{ asMarkdown ? t('ctx.browser.raw') : t('ctx.browser.markdown') }}
        </button>
        <RouterLink
          v-if="turnHash"
          :to="{ name: 'session', params: route.params, query: {}, hash: turnHash }"
          class="ink-accent hover:underline"
        >
          {{ t('ctx.browser.openInChat') }}
        </RouterLink>
      </div>

      <div v-if="node.images?.length" class="flex flex-wrap gap-2">
        <span v-for="(im, i) in node.images" :key="i" class="inline-flex items-center gap-1.5 rounded-md px-2 py-1 surface">
          <i class="i-jannchie-file-image h-3.5 w-3.5 ink-soft" aria-hidden="true" />
          <span>{{ im.width && im.height ? t('ctx.browser.image', { w: im.width, h: im.height }) : t('ctx.browser.imageNoSize') }}</span>
          <span v-if="im.name" class="font-mono ink-soft">{{ im.name }}</span>
          <span v-if="im.bytes" class="num ink-soft">{{ fmtBytes(im.bytes) }}</span>
          <span class="num ink-soft">≈{{ fmtInt(im.tokens ?? 0) }} tok</span>
        </span>
      </div>

      <template v-if="isResult && node.call">
        <div class="font-medium ink-soft">{{ t('ctx.browser.args') }} · <span class="font-mono">{{ node.call.tool?.name }}</span></div>
        <JsonBlock :value="node.call.tool?.input" />
        <div class="font-medium ink-soft">{{ t('ctx.browser.result') }}</div>
      </template>

      <MarkdownText v-if="asMarkdown && node.bodyKind !== 'json'" :text="shown" class="max-h-[32rem] overflow-auto rounded-lg p-2.5 surface" />
      <pre v-else class="payload-block max-h-[32rem]">{{ shown }}</pre>
      <p v-if="body.length > LIMIT && !full" class="m-0 flex items-center gap-2 ink-soft">
        {{ t('ctx.browser.truncated', { n: fmtInt(LIMIT) }) }}
        <button type="button" class="rounded border-0 px-1.5 py-0.5 text-xs surface surface-hover kbd-ring" @click="full = true">{{ t('ctx.browser.showAll') }}</button>
      </p>
    </div>
  </li>
</template>
