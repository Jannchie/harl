<script setup lang="ts">
// 会话概况：计数、计费 token 按内容拆（Token 统计）、时间拆分与时间轴。对应 dsh-context 的 Context / Token / Timing Stats 三张卡。
// 计数、药丸点了不自己跳，发 nav 意图给 ContextView，由它交给对应的卡。
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useChartTheme } from '../../lib/chartTheme'
import type { CtxEventKind, SpanKind } from '../../lib/context'
import { spanColor, TOK_CATS, tokColor, type CtxNav, type CtxSummary, type FamilyStats, type ModelCost } from '../../lib/ctxView'
import { aggregateOps, type FileOp } from '../../lib/fileOps'
import { fmtBig, fmtInt, fmtMoney, fmtMoneyExact, fmtMs, fmtPct } from '../../lib/format'
import DashCard from '../dash/DashCard.vue'
import StatGrid, { type Stat } from '../dash/StatGrid.vue'
import CompBar, { type BarSeg } from './CompBar.vue'

const props = defineProps<{
  s: CtxSummary
  /** 子 agent 一起算的费用；没有子 agent 时不给 */
  familyCost?: number | null
  /** 子孙 agent 的合计（不含自己）；给了就显示 agent 家族一格 */
  family?: FamilyStats | null
  /** 文件操作：输入输出药丸按文件数计 */
  ops?: FileOp[]
  /** 命令执行的程序分布（lib/commands 的 programStats，按调用次数排好）；给了就显示命令一条 */
  programs?: { name: string; n: number; runs: number }[]
}>()
const emit = defineEmits<{ nav: [nav: CtxNav] }>()
const { t } = useI18n()
const theme = useChartTheme()

const lines = (...xs: (string | false | null | undefined)[]) => xs.filter(Boolean).join('\n')

// ---- 费用悬停：每个模型的每 1M 价格、出处、未定价原因 ----
const SOURCES = new Set(['modelsdev', 'openrouter', 'fallback', 'override', 'missing'])
function priceLine(m: ModelCost): string {
  const c = m.card
  if (!c) return t('ctx.stats.costNotListed', { model: m.model })
  const usd = (n: number) => `$${n < 0.01 && n > 0 ? n.toPrecision(2) : n.toFixed(2)}`
  const src = SOURCES.has(c.source) ? t(`ctx.stats.costSources.${c.source as 'modelsdev'}`) : c.source
  return lines(
    t('ctx.stats.costPriceLine', { model: m.model, input: usd(c.input), read: usd(c.cacheRead), write: usd(c.cacheWrite), output: usd(c.output) }),
    `  ${c.provider ? t('ctx.stats.costSourceProvider', { source: src, provider: c.provider }) : t('ctx.stats.costSource', { source: src })} · ${fmtMoneyExact(m.cost)}`,
  )
}
const costTitle = computed(() =>
  lines(fmtMoneyExact(props.s.cost), t('ctx.stats.costHint'), props.s.models.length > 0 && t('ctx.stats.costPriceHead'), ...props.s.models.map(priceLine)),
)

const counts = computed<Stat[]>(() => [
  { key: 'turns', label: t('ctx.stats.turns'), value: fmtInt(props.s.turns), sub: `${fmtInt(props.s.steps)} ${t('ctx.stats.steps')}` },
  { key: 'human', label: t('ctx.stats.humanInputs'), value: fmtInt(props.s.humanInputs), sub: `${fmtInt(props.s.answers)} ${t('ctx.stats.answers')}` },
  {
    key: 'tools',
    label: t('ctx.stats.toolCalls'),
    value: fmtInt(props.s.toolCalls),
    sub: props.s.toolFailures ? t('ctx.stats.failures', { n: fmtInt(props.s.toolFailures) }) : undefined,
  },
  {
    key: 'ctx',
    label: t('ctx.stats.compactions'),
    value: fmtInt(props.s.compactions),
    sub: `${fmtInt(props.s.injects)} ${t('ctx.stats.injects')} · ${fmtInt(props.s.skills)} ${t('ctx.stats.skills')}`,
  },
  {
    key: 'cost',
    label: t('ctx.stats.cost'),
    value: fmtMoney(props.s.cost),
    sub: props.familyCost != null ? `${fmtMoney(props.familyCost)} ${t('ctx.stats.family')}` : props.s.unpriced ? t('ctx.stats.unpriced') : undefined,
    title: costTitle.value,
  },
])

// ---- 药丸 / 泳道 ----
interface Pill {
  key: string
  label: string
  n: number
  icon: string
  nav: CtxNav
  title: string
}

/** 输入输出：用户消息、读写搜索过的文件数、图片（和文件活动卡的计数同一口径） */
const ioPills = computed<Pill[]>(() => {
  const out = { read: 0, write: 0, search: 0, image: 0 }
  for (const r of aggregateOps(props.ops ?? [])) {
    if (r.reads) out.read++
    if (r.writes) out.write++
    if (r.searches) out.search++
    if (r.form === 'image') out.image++
  }
  const files = t('ctx.stats.filesJump')
  return [
    { key: 'user', label: t('ctx.stats.ioUser'), n: props.s.humanInputs, icon: 'i-jannchie-user', nav: { kind: 'browser', cat: 'user' }, title: t('ctx.stats.userJump') },
    { key: 'read', label: t('ctx.stats.ioRead'), n: out.read, icon: 'i-jannchie-eye', nav: { kind: 'files', op: 'read' }, title: files },
    { key: 'write', label: t('ctx.stats.ioWrite'), n: out.write, icon: 'i-jannchie-clock-edit', nav: { kind: 'files', op: 'write' }, title: files },
    { key: 'search', label: t('ctx.stats.ioSearch'), n: out.search, icon: 'i-jannchie-search', nav: { kind: 'files', op: 'search' }, title: files },
    { key: 'image', label: t('ctx.stats.ioImages'), n: out.image, icon: 'i-jannchie-file-image', nav: { kind: 'files', op: 'image' }, title: files },
  ]
})

const TOP_TOOLS = 6
const topTools = computed(() => props.s.topTools.slice(0, TOP_TOOLS))
const moreTools = computed(() => Math.max(0, props.s.topTools.length - TOP_TOOLS))

const TOP_PROGRAMS = 6
const topPrograms = computed(() => (props.programs ?? []).slice(0, TOP_PROGRAMS))
const morePrograms = computed(() => Math.max(0, (props.programs?.length ?? 0) - TOP_PROGRAMS))

const EVENT_KINDS: CtxEventKind[] = ['compaction', 'prune', 'inject', 'model', 'mode']
const EVENT_ICON: Record<CtxEventKind, string> = {
  inject: 'i-jannchie-plus-circle',
  compaction: 'i-jannchie-scissors',
  prune: 'i-jannchie-minus-circle',
  model: 'i-jannchie-arrows-horizontal',
  mode: 'i-jannchie-flag',
}
const eventPills = computed<Pill[]>(() => {
  const out: Pill[] = EVENT_KINDS.filter((k) => props.s.eventCounts[k] > 0).map((k) => {
    const label = t(`ctx.events.kinds.${k}`)
    return { key: k, label, n: props.s.eventCounts[k], icon: EVENT_ICON[k], nav: { kind: 'events', eventKind: k }, title: t('ctx.stats.eventsKindJump', { kind: label }) }
  })
  if (props.s.skillLoads)
    out.push({
      key: 'skill',
      label: t('ctx.stats.skillLoads'),
      n: props.s.skillLoads,
      icon: 'i-jannchie-puzzle',
      nav: { kind: 'browser', cat: 'skill' },
      title: t('ctx.stats.skillsJump'),
    })
  return out
})

// ---- Token 统计 ----
const tokSegs = computed<BarSeg[]>(() =>
  TOK_CATS.map((c) => ({ key: c, label: t(`ctx.tokens.cats.${c}`), value: Math.round(props.s.tokenStats[c]), color: tokColor(c, theme.value) })),
)
const TOK_HINT: Partial<Record<string, string>> = {
  reasoning: 'ctx.tokens.reasoningHint',
  answer: 'ctx.tokens.answerHint',
  toolCalls: 'ctx.tokens.toolCallsHint',
  other: 'ctx.catHint.other',
}

// ---- 时间 ----
const SPAN_LABEL: Record<SpanKind, string> = {
  ttft: 'ctx.timing.ttft',
  reasoning: 'ctx.timing.reasoning',
  text: 'ctx.timing.text',
  toolArgs: 'ctx.timing.toolArgs',
  model: 'ctx.timing.model',
  tools: 'ctx.timing.tools',
  overhead: 'ctx.timing.overhead',
  wait: 'ctx.timing.wait',
  idle: 'ctx.timing.idleGap',
}

const timing = computed(() => {
  const tm = props.s.timing
  const th = theme.value
  const kinds: SpanKind[] = ['ttft', 'reasoning', 'text', 'toolArgs', 'model', 'tools', 'overhead']
  const segs: (BarSeg & { sub?: string; hint?: string })[] = kinds
    .filter((k) => tm.byKind[k] > 0)
    .map((k) => {
      const b = tm.blocks && (k === 'reasoning' || k === 'text' || k === 'toolArgs') ? tm.blocks[k] : null
      return {
        key: k,
        label: t(SPAN_LABEL[k]),
        value: tm.byKind[k],
        color: spanColor(k, th),
        sub: b ? t('ctx.timing.blocks', { n: fmtInt(b.n) }) : k === 'tools' ? t('ctx.timing.toolRuns', { n: fmtInt(tm.toolRuns) }) : undefined,
        hint: k === 'overhead' ? t('ctx.timing.overheadHint') : undefined,
      }
    })
  return { tm, segs }
})

/** 时间轴的刻度：零、四分位、全程 */
const ticks = computed(() => [0, 0.25, 0.5, 0.75, 1].map((q) => ({ q, label: fmtMs(props.s.timing.activeMs * q) })))
const stripTotal = computed(() => props.s.timing.strip.reduce((a, x) => a + x.ms, 0))
</script>

<template>
  <DashCard icon="i-jannchie-gauge-list" :title="t('ctx.stats.title')">
    <div class="flex flex-col gap-5">
      <StatGrid :stats="counts" />

      <!-- 药丸：点了跳到对应的卡 -->
      <div class="grid gap-x-6 gap-y-3 text-xs md:grid-cols-2">
        <div class="flex min-w-0 flex-wrap items-center gap-1.5">
          <span class="w-16 shrink-0 ink-soft">{{ t('ctx.stats.io') }}</span>
          <template v-for="p in ioPills" :key="p.key">
            <button v-if="p.n > 0 || p.key === 'user'" type="button" class="segment border-0 surface ink-muted" :title="p.title" @click="emit('nav', p.nav)">
              <i class="h-3.5 w-3.5" :class="p.icon" aria-hidden="true" />{{ p.label }} <span class="num ink-strong">{{ fmtInt(p.n) }}</span>
            </button>
          </template>
        </div>
        <div class="flex min-w-0 flex-wrap items-center gap-1.5">
          <span class="w-16 shrink-0 ink-soft">{{ t('ctx.stats.events') }}</span>
          <button
            v-for="p in eventPills"
            :key="p.key"
            type="button"
            class="segment border-0 surface ink-muted"
            :title="p.title"
            @click="emit('nav', p.nav)"
          >
            <i class="h-3.5 w-3.5" :class="p.icon" aria-hidden="true" />{{ p.label }} <span class="num ink-strong">{{ fmtInt(p.n) }}</span>
          </button>
          <span v-if="!eventPills.length" class="ink-soft">{{ t('ctx.stats.laneEmpty') }}</span>
        </div>
        <div class="flex min-w-0 flex-wrap items-center gap-1.5 md:col-span-2">
          <span class="w-16 shrink-0 ink-soft">{{ t('ctx.stats.toolCalls') }}</span>
          <button
            v-for="x in topTools"
            :key="x.name"
            type="button"
            class="segment border-0 surface ink-muted"
            :title="lines(t('ctx.stats.toolJump'), x.failures ? t('ctx.stats.failures', { n: fmtInt(x.failures) }) : null, x.ms ? fmtMs(x.ms) : null)"
            @click="emit('nav', { kind: 'browser', cat: 'tool', filter: x.name })"
          >
            <span class="font-mono">{{ x.name || '?' }}</span>
            <span class="num ink-strong">×{{ fmtInt(x.calls) }}</span>
            <span v-if="x.failures" class="num ink-danger">{{ fmtInt(x.failures) }}</span>
          </button>
          <button
            v-if="moreTools"
            type="button"
            class="segment border-0 surface ink-soft"
            :title="t('ctx.stats.toolsMoreTip')"
            @click="emit('nav', { kind: 'browser', cat: 'tool' })"
          >
            <span class="num">{{ t('ctx.stats.toolsMore', { n: fmtInt(moreTools) }) }}</span>
          </button>
          <span v-if="!topTools.length" class="ink-soft">{{ t('ctx.stats.laneEmpty') }}</span>
        </div>
        <!-- 命令执行：最常跑的程序，点了在命令执行卡里只看它 -->
        <div v-if="programs" class="flex min-w-0 flex-wrap items-center gap-1.5 md:col-span-2">
          <span class="w-16 shrink-0 ink-soft">{{ t('ctx.stats.commands') }}</span>
          <button
            v-for="p in topPrograms"
            :key="p.name"
            type="button"
            class="segment border-0 surface ink-muted"
            :title="t('ctx.stats.programJump', { calls: fmtInt(p.n), runs: fmtInt(p.runs) })"
            @click="emit('nav', { kind: 'commands', program: p.name })"
          >
            <span class="font-mono">{{ p.name }}</span>
            <span class="num ink-strong">×{{ fmtInt(p.n) }}</span>
          </button>
          <button
            v-if="morePrograms"
            type="button"
            class="segment border-0 surface ink-soft"
            :title="t('ctx.stats.programsMoreTip')"
            @click="emit('nav', { kind: 'commands' })"
          >
            <span class="num">{{ t('ctx.stats.toolsMore', { n: fmtInt(morePrograms) }) }}</span>
          </button>
          <span v-if="!topPrograms.length" class="ink-soft">{{ t('ctx.stats.laneEmpty') }}</span>
        </div>
        <!-- agent 家族：自己 + 子孙 agent，点了看 Agent 网络 -->
        <div v-if="family && family.agents > 0" class="flex min-w-0 flex-wrap items-center gap-1.5 md:col-span-2">
          <span class="w-16 shrink-0 ink-soft">{{ t('ctx.stats.agents') }}</span>
          <button type="button" class="segment border-0 surface ink-muted" :title="t('ctx.stats.ownCostHint')" @click="emit('nav', { kind: 'fleet' })">
            <i class="i-jannchie-robot h-3.5 w-3.5" aria-hidden="true" />{{ t('ctx.stats.currentAgent') }}
            <span class="num ink-strong">{{ fmtBig(s.billed) }}</span>
            <span class="num">{{ fmtMoney(s.cost) }}</span>
          </button>
          <button
            type="button"
            class="segment border-0 surface ink-muted"
            :title="lines(t('ctx.stats.subCostHint'), family.partial ? t('ctx.stats.familyPartialTip', { n: fmtInt(family.partial) }) : null)"
            @click="emit('nav', { kind: 'fleet' })"
          >
            <i class="i-jannchie-network h-3.5 w-3.5" aria-hidden="true" />{{ t('ctx.stats.subAgents', { n: fmtInt(family.agents) }) }}
            <span class="num ink-strong">{{ fmtBig(family.billed) }}</span>
            <span class="num">{{ fmtMoney(family.cost) }}</span>
            <span v-if="family.partial" class="ink-warning">{{ t('ctx.stats.familyPartial') }}</span>
          </button>
        </div>
      </div>

      <div class="grid gap-5 lg:grid-cols-2">
        <!-- Token 统计 -->
        <section class="flex min-w-0 flex-col gap-2">
          <div class="flex items-baseline justify-between gap-3">
            <h3 class="m-0 text-xs font-medium ink-soft" :title="t('ctx.tokens.statsHint')">{{ t('ctx.tokens.billed') }}</h3>
            <span class="text-xs ink-soft" :title="t('ctx.tokens.cacheHitHint')">
              {{ t('ctx.tokens.cacheHit') }} <span class="num ink-strong">{{ fmtPct(s.cacheHit, 2) }}</span>
            </span>
          </div>
          <div class="text-xl font-semibold leading-tight num" :title="lines(fmtInt(s.billed), t('ctx.tokens.billedHint'))">{{ fmtBig(s.billed) }}</div>
          <CompBar :segs="tokSegs" :height="12" />
          <ul class="m-0 grid list-none grid-cols-2 gap-x-4 gap-y-1 p-0 text-xs sm:grid-cols-3" :aria-label="t('ctx.tokens.statsHint')">
            <li
              v-for="seg in tokSegs.filter((x) => x.value > 0)"
              :key="seg.key"
              class="flex min-w-0 items-center gap-1.5"
              :title="lines(`${seg.label} ${fmtInt(seg.value)} · ${fmtPct(s.billed ? seg.value / s.billed : 0)}`, TOK_HINT[seg.key] && t(TOK_HINT[seg.key]!))"
            >
              <span class="h-2 w-2 shrink-0 rounded-full" :style="{ background: seg.color }" />
              <span class="min-w-0 flex-1 truncate ink-muted">{{ seg.label }}</span>
              <span class="num">{{ fmtBig(seg.value) }}</span>
            </li>
          </ul>
        </section>

        <!-- 时间 -->
        <section class="flex min-w-0 flex-col gap-2">
          <div class="flex items-baseline justify-between gap-3">
            <h3 class="m-0 text-xs font-medium ink-soft" :title="t('ctx.timing.activeHint')">{{ t('ctx.timing.active') }}</h3>
            <span v-if="s.outputPerSec" class="text-xs ink-soft">
              {{ t('ctx.timing.tps') }} <span class="num ink-strong">{{ t('ctx.timing.tpsUnit', { n: s.outputPerSec.toFixed(1) }) }}</span>
            </span>
          </div>
          <div class="flex items-baseline gap-3">
            <span class="text-xl font-semibold leading-tight num">{{ fmtMs(timing.tm.activeMs) }}</span>
            <span class="text-xs ink-soft" :title="t('ctx.timing.wallHint')">
              {{ t('ctx.timing.wall') }} <span class="num">{{ fmtMs(s.wallMs) }}</span>
            </span>
          </div>
          <CompBar :segs="timing.segs" :height="12" />
          <ul class="m-0 grid list-none grid-cols-2 gap-x-4 gap-y-1 p-0 text-xs sm:grid-cols-3">
            <li v-for="seg in timing.segs" :key="seg.key" class="flex min-w-0 items-center gap-1.5" :title="seg.hint">
              <span class="h-2 w-2 shrink-0 rounded-full" :style="{ background: seg.color }" />
              <span class="min-w-0 flex-1 truncate ink-muted">
                {{ seg.label }}<span v-if="seg.sub" class="num ink-soft"> · {{ seg.sub }}</span>
              </span>
              <span class="num">{{ fmtMs(seg.value) }}</span>
            </li>
          </ul>
          <div class="flex flex-wrap gap-x-4 gap-y-1 text-xs ink-soft">
            <span v-if="timing.tm.toolSumMs > 0" :title="t('ctx.timing.toolBusyHint', { sum: fmtMs(timing.tm.toolSumMs) })">
              {{ t('ctx.timing.toolBusy') }} <span class="num ink-strong">{{ fmtMs(timing.tm.toolBusyMs) }}</span>
              <span class="num"> / Σ {{ fmtMs(timing.tm.toolSumMs) }}</span>
            </span>
            <span v-if="timing.tm.waitMs > 0" :title="t('ctx.timing.waitHint')">
              {{ t('ctx.timing.wait') }} <span class="num ink-strong">{{ fmtMs(timing.tm.waitMs) }}</span>
            </span>
            <span v-if="timing.tm.idleMs > 0" :title="t('ctx.timing.idleGapHint')">
              {{ t('ctx.timing.idleGap') }} <span class="num ink-strong">{{ fmtMs(timing.tm.idleMs) }}</span>
            </span>
          </div>

          <!-- 时间轴：各段按发生顺序紧排 -->
          <div v-if="s.timing.strip.length > 1 && stripTotal > 0" class="flex flex-col gap-1" :title="t('ctx.timing.strip')">
            <div class="flex h-2.5 w-full overflow-hidden rounded-sm surface" role="img" :aria-label="t('ctx.timing.strip')">
              <div
                v-for="(x, i) in s.timing.strip"
                :key="i"
                class="h-full shrink-0"
                :style="{ width: `${(x.ms / stripTotal) * 100}%`, background: spanColor(x.kind, theme) }"
                :title="`${t(SPAN_LABEL[x.kind])} ${fmtMs(x.ms)}`"
              />
            </div>
            <div class="relative h-4 text-[10px] ink-soft">
              <span
                v-for="tk in ticks"
                :key="tk.q"
                class="num absolute top-0 whitespace-nowrap"
                :style="{ left: `${tk.q * 100}%`, transform: tk.q === 0 ? 'none' : tk.q === 1 ? 'translateX(-100%)' : 'translateX(-50%)' }"
              >
                {{ tk.label }}
              </span>
            </div>
          </div>

          <div v-if="s.slowTools.length" class="flex flex-wrap gap-x-3 gap-y-1 text-xs ink-soft">
            <span>{{ t('ctx.timing.slowTools') }}</span>
            <span v-for="[name, x] in s.slowTools" :key="name" class="ink-muted">
              <span class="font-mono">{{ name }}</span> <span class="num">{{ fmtMs(x.ms) }}</span>
            </span>
          </div>
        </section>
      </div>
    </div>
  </DashCard>
</template>
