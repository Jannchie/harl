<script setup lang="ts">
// 上下文浏览器里的一项：一行摘要，展开看原文。工具结果展开成完整的一次调用（名字、参数、状态、结果）。
// 正文带工具：在正文里查找（高亮 + 上一个 / 下一个）、复制原文、复制 JSON。
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue'
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

// ---- 查找 ----
/** 最多标这么多处，再多的只计数不高亮 */
const MAX_MARKS = 2000
const findOpen = ref(false)
const findQ = ref('')
const findIdx = ref(0)
const findInput = ref<HTMLInputElement | null>(null)
const pre = ref<HTMLElement | null>(null)

/** 查找时一律看原文（Markdown 渲染后没法按字符位置高亮） */
const showMarkdown = computed(() => asMarkdown.value && props.node.bodyKind !== 'json' && !(findOpen.value && findQ.value))

/** 每处命中在正文里的 [起, 止)；不分大小写 */
const matches = computed<[number, number][]>(() => {
  const q = findQ.value
  if (!findOpen.value || !q) return []
  const re = new RegExp(q.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'gi')
  const out: [number, number][] = []
  for (const m of body.value.matchAll(re)) {
    out.push([m.index, m.index + m[0].length])
    if (out.length >= MAX_MARKS) break
  }
  return out
})

/** 正文切成普通段和命中段；命中段带它的序号 */
const segments = computed(() => {
  const text = shown.value
  const out: { s: string; m: number }[] = []
  let at = 0
  matches.value.forEach(([a, b], i) => {
    if (a >= text.length) return
    if (a > at) out.push({ s: text.slice(at, a), m: -1 })
    out.push({ s: text.slice(a, Math.min(b, text.length)), m: i })
    at = Math.min(b, text.length)
  })
  if (at < text.length) out.push({ s: text.slice(at), m: -1 })
  return out
})

async function openFind() {
  findOpen.value = true
  await nextTick()
  findInput.value?.focus()
  findInput.value?.select()
}
function closeFind() {
  findOpen.value = false
  findQ.value = ''
}

/** 跳到第 i 处：在截断外就先全部显示，再滚到它 */
async function goTo(i: number) {
  const n = matches.value.length
  if (!n) return
  findIdx.value = ((i % n) + n) % n
  const hit = matches.value[findIdx.value]!
  if (hit[0] >= shown.value.length) full.value = true
  await nextTick()
  pre.value?.querySelector(`mark[data-m="${findIdx.value}"]`)?.scrollIntoView({ block: 'nearest' })
}
watch(matches, (m) => {
  if (m.length) void goTo(0)
  else findIdx.value = 0
})
function onFindKey(e: KeyboardEvent) {
  if (e.key === 'Enter') {
    e.preventDefault()
    void goTo(findIdx.value + (e.shiftKey ? -1 : 1))
  } else if (e.key === 'Escape') {
    e.preventDefault()
    closeFind()
  }
}

// ---- 复制 ----
/** JSON 复制：本来就是 JSON 的正文原样；工具结果连同配对的调用；其余是原事件。system / 工具定义没有单一事件，不给 */
const canCopyJson = computed(() => props.node.bodyKind === 'json' || (props.node.cat !== 'system' && props.node.cat !== 'tools'))
function jsonText(): string {
  const n = props.node
  if (n.bodyKind === 'json') return n.body()
  if (n.call) return JSON.stringify({ call: n.call.tool, result: n.ev.tool }, null, 2)
  return JSON.stringify(n.ev, null, 2)
}

const copied = ref<'raw' | 'json' | 'fail' | null>(null)
let copiedTimer: ReturnType<typeof setTimeout> | undefined
async function copy(kind: 'raw' | 'json') {
  let ok = true
  try {
    await navigator.clipboard.writeText(kind === 'raw' ? props.node.body() : jsonText())
  } catch {
    ok = false
  }
  copied.value = ok ? kind : 'fail'
  clearTimeout(copiedTimer)
  copiedTimer = setTimeout(() => (copied.value = null), 1500)
}
onBeforeUnmount(() => clearTimeout(copiedTimer))
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
        <!-- 正文工具 -->
        <span class="ml-auto inline-flex items-center gap-1">
          <span v-if="copied" class="text-[11px]" :class="copied === 'fail' ? 'ink-danger' : 'ink-success'" role="status">
            {{ copied === 'fail' ? t('ctxBrowser.body.copyFailed') : t('ctxBrowser.body.copied') }}
          </span>
          <button
            v-if="body"
            type="button"
            class="icon-button-sm"
            :class="findOpen ? 'pill-selected' : ''"
            :aria-pressed="findOpen"
            :aria-label="t('ctxBrowser.body.find')"
            :title="t('ctxBrowser.body.find')"
            @click="findOpen ? closeFind() : openFind()"
          >
            <i class="i-jannchie-search h-3.5 w-3.5" />
          </button>
          <button type="button" class="icon-button-sm" :aria-label="t('ctxBrowser.body.copyRaw')" :title="t('ctxBrowser.body.copyRaw')" @click="copy('raw')">
            <i class="i-jannchie-copy h-3.5 w-3.5" />
          </button>
          <button
            v-if="canCopyJson"
            type="button"
            class="rounded border-0 px-1.5 py-0.5 text-xs surface surface-hover kbd-ring"
            :title="t('ctxBrowser.body.copyJson')"
            @click="copy('json')"
          >
            {{ t('ctxBrowser.body.copyJson') }}
          </button>
        </span>
      </div>

      <div v-if="findOpen" class="flex items-center gap-1.5">
        <input
          ref="findInput"
          v-model="findQ"
          type="search"
          class="field max-w-64 py-1 text-xs"
          :placeholder="t('ctxBrowser.body.findPlaceholder')"
          :aria-label="t('ctxBrowser.body.find')"
          @keydown="onFindKey"
        />
        <span v-if="findQ" class="min-w-12 num ink-soft" role="status">
          {{
            matches.length
              ? t('ctxBrowser.body.matches', { i: fmtInt(findIdx + 1), n: matches.length >= MAX_MARKS ? `${fmtInt(MAX_MARKS)}+` : fmtInt(matches.length) })
              : t('ctxBrowser.body.noMatch')
          }}
        </span>
        <button type="button" class="icon-button-sm" :disabled="!matches.length" :aria-label="t('ctxBrowser.body.prev')" :title="t('ctxBrowser.body.prev')" @click="goTo(findIdx - 1)">
          <i class="i-jannchie-arrow-up h-3.5 w-3.5" />
        </button>
        <button type="button" class="icon-button-sm" :disabled="!matches.length" :aria-label="t('ctxBrowser.body.next')" :title="t('ctxBrowser.body.next')" @click="goTo(findIdx + 1)">
          <i class="i-jannchie-arrow-down h-3.5 w-3.5" />
        </button>
        <button type="button" class="icon-button-sm" :aria-label="t('ctxBrowser.body.close')" :title="t('ctxBrowser.body.close')" @click="closeFind">
          <i class="i-jannchie-x h-3.5 w-3.5" />
        </button>
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

      <MarkdownText v-if="showMarkdown" :text="shown" class="max-h-[32rem] overflow-auto rounded-lg p-2.5 surface" />
      <!-- pre 里的空白都会显示，模板保持一行 -->
      <pre v-else-if="matches.length" ref="pre" class="payload-block max-h-[32rem]"><template v-for="(g, i) in segments" :key="i"><mark v-if="g.m >= 0" :data-m="g.m" class="find-hit" :class="g.m === findIdx ? 'find-cur' : ''">{{ g.s }}</mark><template v-else>{{ g.s }}</template></template></pre>
      <pre v-else class="payload-block max-h-[32rem]">{{ shown }}</pre>
      <p v-if="body.length > LIMIT && !full" class="m-0 flex items-center gap-2 ink-soft">
        {{ t('ctx.browser.truncated', { n: fmtInt(LIMIT) }) }}
        <button type="button" class="rounded border-0 px-1.5 py-0.5 text-xs surface surface-hover kbd-ring" @click="full = true">{{ t('ctx.browser.showAll') }}</button>
      </p>
    </div>
  </li>
</template>

<style scoped>
.find-hit {
  border-radius: 2px;
  background: color-mix(in srgb, var(--accent) 28%, transparent);
  color: inherit;
}
.find-cur {
  background: var(--accent);
  color: var(--bg);
}
</style>
