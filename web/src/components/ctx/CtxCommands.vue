<script setup lang="ts">
// 命令执行：每次 shell / 代码执行一行，状态、退出码、墙钟时长；展开看完整命令，「定位」在上下文浏览器里看它的结果。
// 左栏是整场的摘要、值得注意的几项和程序分布，与列表筛选无关；点它们套用对应的筛选。
// 口径参考 dsh-context（Apache-2.0）的命令卡，见 lib/commands.ts。
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import type { Event } from '../../api'
import {
  commandLog,
  matchFilter,
  matchQuery,
  SLOW_MS,
  sortCommands,
  type CmdFilter,
  type CmdRow,
  type CmdSort,
  type CmdStatus,
  type CommandLog,
} from '../../lib/commands'
import { usePref } from '../../lib/ctxView'
import { fmtInt, fmtMs, shortenPath } from '../../lib/format'
import DashCard from '../dash/DashCard.vue'
import Segmented from '../Segmented.vue'
import TimeStamp from '../TimeStamp.vue'

const props = defineProps<{
  events: Event[]
  /** 外面已经算好的 commandLog(events)；给了就不再算一遍 */
  log?: CommandLog | null
  /** 从外面聚焦某个程序（统计卡的命令条点进来）；变了就套用 */
  program?: string | null
  /** 调用 seq → 所在的请求序号（从 0 数）；不给就显示 turn / step */
  stepOf?: (seq: number) => number
  /** 外面的跳转意图：程序 + 筛选（all / failed / slow / background，别的值当搜索词）；按内容比，换 nonce 再跳一次 */
  nav?: { program?: string; filter?: string; nonce?: number } | null
}>()
const emit = defineEmits<{
  /** 在浏览器里看：seq 是结果行（嵌套命令是外层结果行），没有结果就是调用行 */
  reveal: [seq: number, row: CmdRow]
}>()
const { t } = useI18n()

const log = computed(() => props.log ?? commandLog(props.events))

const filter = ref<CmdFilter>('all')
const query = ref('')
const sort = usePref<CmdSort>('cmds.sort', 'latest')
const program = ref<string | null>(null)
const programQuery = ref('')
const open = ref(new Set<string>())
const limit = ref(80)

watch(
  () => props.program,
  (p) => {
    if (p) apply({ program: p })
  },
  { immediate: true },
)
watch(
  () => (props.nav ? JSON.stringify(props.nav) : ''),
  () => {
    const v = props.nav
    if (!v) return
    const f = v.filter && ['all', 'failed', 'slow', 'background'].includes(v.filter) ? (v.filter as CmdFilter) : undefined
    apply({ program: v.program, filter: f, query: f || !v.filter ? undefined : v.filter })
  },
  { immediate: true },
)
// 程序不在了（换了会话）就清掉
watch(log, (l) => {
  if (program.value && !l.programs.some((p) => p.name === program.value)) program.value = null
})

/** 搜索 + 程序之后的集合：筛选 chip 的计数按它算 */
const base = computed(() =>
  log.value.rows.filter((r) => matchQuery(r, query.value) && (!program.value || r.programs.includes(program.value))),
)
const counts = computed(() => {
  const out: Record<CmdFilter, number> = { all: 0, failed: 0, slow: 0, background: 0 }
  for (const r of base.value) for (const f of FILTER_KEYS) if (matchFilter(r, f)) out[f]++
  return out
})
const shown = computed(() => sortCommands(base.value.filter((r) => matchFilter(r, filter.value)), sort.value))

const FILTER_KEYS: CmdFilter[] = ['all', 'failed', 'slow', 'background']
const SORTS = computed(() => [
  { value: 'latest' as const, label: t('ctxCmds.sort.latest') },
  { value: 'slowest' as const, label: t('ctxCmds.sort.slowest') },
])

// ---- 左栏：值得注意（点了套用一组筛选，再点恢复） ----

interface Lens {
  filter?: CmdFilter
  query?: string
  sort?: CmdSort
  program?: string
}
function isLens(l: Lens): boolean {
  return (
    (l.filter ?? 'all') === filter.value &&
    (l.query ?? '') === query.value &&
    (l.sort ?? 'latest') === sort.value &&
    (l.program ?? null) === program.value
  )
}
function apply(l: Lens) {
  filter.value = l.filter ?? 'all'
  query.value = l.query ?? ''
  sort.value = l.sort ?? 'latest'
  program.value = l.program ?? null
  limit.value = 80
}
function toggleLens(l: Lens) {
  apply(isLens(l) ? {} : l)
}

const notable = computed(() => {
  const n = log.value.notable
  const out: { key: string; label: string; value: string; mono: boolean; n: string; title?: string; lens: Lens }[] = []
  if (n.slowest)
    out.push({
      key: 'slowest',
      label: t('ctxCmds.notable.slowest'),
      value: preview(n.slowest.cmd),
      mono: true,
      n: fmtMs(n.slowest.ms),
      title: n.slowest.cmd,
      lens: { query: n.slowest.cmd, sort: 'slowest' },
    })
  if (n.failing)
    out.push({
      key: 'failing',
      label: t('ctxCmds.notable.failing'),
      value: n.failing.program,
      mono: true,
      n: `×${fmtInt(n.failing.count)}`,
      lens: { filter: 'failed', program: n.failing.program },
    })
  if (n.repeats)
    out.push({
      key: 'repeats',
      label: t('ctxCmds.notable.repeats'),
      value: preview(n.repeats.cmd),
      mono: true,
      n: `×${fmtInt(n.repeats.count)}`,
      title: n.repeats.cmd,
      lens: { query: n.repeats.cmd },
    })
  if (n.background)
    out.push({ key: 'background', label: t('ctxCmds.notable.background'), value: '', mono: false, n: `×${fmtInt(n.background)}`, lens: { filter: 'background' } })
  return out
})

// ---- 左栏：程序分布 ----

const programMax = computed(() => Math.max(1, ...log.value.programs.map((p) => p.n)))
const programList = computed(() => {
  const q = programQuery.value.trim().toLowerCase()
  return log.value.programs.filter((p) => p.name === program.value || !q || p.name.toLowerCase().includes(q))
})
function toggleProgram(name: string) {
  program.value = program.value === name ? null : name
  limit.value = 80
}

// ---- 行 ----

const STATUS_ICON: Record<CmdStatus, string> = {
  running: 'i-jannchie-loader',
  completed: 'i-jannchie-check-circle',
  failed: 'i-jannchie-x-circle',
  killed: 'i-jannchie-ban',
  stopped: 'i-jannchie-stop',
}
const STATUS_INK: Record<CmdStatus, string> = {
  running: 'ink-accent',
  completed: 'ink-success',
  failed: 'ink-danger',
  killed: 'ink-danger',
  stopped: 'ink-warning',
}

/** 一行预览：第一行非空的内容，去掉开头的 `cd X &&` */
function preview(cmd: string, max = 120): string {
  const line = cmd.split('\n').find((l) => l.trim()) ?? ''
  const s = line.trim().replace(/^cd\s+("[^"]*"|'[^']*'|\S+)\s*(?:&&|;)\s*/, '')
  return s.length > max ? `${s.slice(0, max)}…` : s
}

function lineCount(cmd: string): number {
  let n = 1
  for (let i = 0; i < cmd.length; i++) if (cmd.charCodeAt(i) === 10) n++
  return n
}

/** 行尾的小标签：后台、超时、截断、stderr、沙箱 …… */
function flagsOf(r: CmdRow): { label: string; ink: string; title?: string }[] {
  const out: { label: string; ink: string; title?: string }[] = []
  if (r.lang === 'code') out.push({ label: t('ctxCmds.code'), ink: 'ink-soft' })
  if (r.parentSeq != null) out.push({ label: t('ctxCmds.nested'), ink: 'ink-soft', title: t('ctxCmds.nestedTip') })
  if (r.background) out.push({ label: t('ctxCmds.bg'), ink: 'ink-accent' })
  if (r.flags.timedOut) out.push({ label: t('ctxCmds.timedOut'), ink: 'ink-danger' })
  if (r.flags.denied) out.push({ label: t('ctxCmds.denied'), ink: 'ink-danger' })
  if (r.flags.signal) out.push({ label: t('ctxCmds.signal', { s: r.flags.signal }), ink: 'ink-danger' })
  if (r.flags.stderr) out.push({ label: t('ctxCmds.stderr'), ink: 'ink-warning' })
  if (r.flags.truncated) out.push({ label: t('ctxCmds.truncated'), ink: 'ink-soft' })
  return out
}

function stepLabel(r: CmdRow): { text: string; title: string } | null {
  const seq = r.parentSeq ?? r.seq
  if (props.stepOf) {
    const n = props.stepOf(seq) + 1
    return { text: `#${n}`, title: t('ctxCmds.step', { n }) }
  }
  if (r.turn != null && r.step != null) return { text: `T${r.turn}·S${r.step}`, title: t('ctxCmds.turnStep', { t: r.turn, s: r.step }) }
  if (r.turn != null) return { text: `T${r.turn}`, title: t('ctxCmds.turnStep', { t: r.turn, s: '–' }) }
  return null
}

function rowKey(r: CmdRow): string {
  return `${r.key}:${r.seq}`
}
function toggle(r: CmdRow) {
  const k = rowKey(r)
  const s = new Set(open.value)
  if (s.has(k)) s.delete(k)
  else s.add(k)
  open.value = s
}
</script>

<template>
  <DashCard icon="i-jannchie-terminal" :title="t('ctxCmds.title')" :caption="t('ctxCmds.caption')" :title-hint="t('ctxCmds.titleHint')">
    <template v-if="log.rows.length" #controls>
      <Segmented v-model="sort" :label="t('ctxCmds.sortLabel')" :options="SORTS" />
    </template>

    <p v-if="!log.rows.length" class="m-0 py-6 text-center text-xs ink-soft">{{ t('ctxCmds.empty') }}</p>
    <div v-else class="grid gap-4 lg:grid-cols-[15rem_minmax(0,1fr)]">
      <!-- 左栏：摘要 / 值得注意 / 程序分布 -->
      <aside class="flex min-w-0 flex-col gap-4 text-xs">
        <section class="flex flex-col gap-1.5">
          <h3 class="m-0 flex items-center gap-1.5 text-xs font-medium ink-muted" :title="t('ctxCmds.summary.tip')">
            {{ t('ctxCmds.summary.title') }}<i class="i-jannchie-info h-3 w-3 ink-soft opacity-70" aria-hidden="true" />
          </h3>
          <dl class="m-0 grid grid-cols-2 gap-x-3 gap-y-1">
            <div class="flex items-baseline gap-1.5">
              <dt class="sr-only">{{ t('ctxCmds.summary.runs') }}</dt>
              <dd class="m-0 text-sm num ink-strong">{{ fmtInt(log.summary.runs) }}</dd>
              <span class="ink-soft" aria-hidden="true">{{ t('ctxCmds.summary.runs') }}</span>
            </div>
            <div class="flex items-baseline gap-1.5">
              <dt class="sr-only">{{ t('ctxCmds.summary.failed') }}</dt>
              <dd class="m-0 text-sm num" :class="log.summary.failed ? 'ink-danger' : 'ink-strong'">{{ fmtInt(log.summary.failed) }}</dd>
              <span class="ink-soft" aria-hidden="true">{{ t('ctxCmds.summary.failed') }}</span>
            </div>
            <div class="flex items-baseline gap-1.5">
              <dt class="sr-only">{{ t('ctxCmds.summary.totalTime') }}</dt>
              <dd class="m-0 text-sm num ink-strong">{{ fmtMs(log.summary.ms) }}</dd>
              <span class="ink-soft" aria-hidden="true">{{ t('ctxCmds.summary.totalTime') }}</span>
            </div>
            <div class="flex items-baseline gap-1.5">
              <dt class="sr-only">{{ t('ctxCmds.summary.background') }}</dt>
              <dd class="m-0 text-sm num ink-strong">{{ fmtInt(log.summary.background) }}</dd>
              <span class="ink-soft" aria-hidden="true">{{ t('ctxCmds.summary.background') }}</span>
            </div>
            <div v-if="log.summary.running" class="flex items-baseline gap-1.5">
              <dt class="sr-only">{{ t('ctxCmds.summary.running') }}</dt>
              <dd class="m-0 text-sm num ink-accent">{{ fmtInt(log.summary.running) }}</dd>
              <span class="ink-soft" aria-hidden="true">{{ t('ctxCmds.summary.running') }}</span>
            </div>
          </dl>
        </section>

        <section v-if="notable.length" class="flex flex-col gap-1">
          <h3 class="m-0 flex items-center gap-1.5 text-xs font-medium ink-muted" :title="t('ctxCmds.notable.tip')">
            {{ t('ctxCmds.notable.title') }}<i class="i-jannchie-info h-3 w-3 ink-soft opacity-70" aria-hidden="true" />
          </h3>
          <button
            v-for="n in notable"
            :key="n.key"
            type="button"
            class="flex w-full items-center gap-2 rounded-md border-0 bg-transparent px-1.5 py-1 text-left text-xs surface-hover kbd-ring"
            :class="isLens(n.lens) ? 'pill-selected' : ''"
            :aria-pressed="isLens(n.lens)"
            :title="n.title"
            @click="toggleLens(n.lens)"
          >
            <span class="shrink-0 ink-soft">{{ n.label }}</span>
            <span class="min-w-0 flex-1 truncate" :class="n.mono ? 'font-mono ink-strong' : ''">{{ n.value }}</span>
            <span class="shrink-0 num ink-muted">{{ n.n }}</span>
          </button>
        </section>

        <section class="flex min-h-0 flex-col gap-1">
          <h3 class="m-0 flex items-center gap-1.5 text-xs font-medium ink-muted" :title="t('ctxCmds.programs.tip')">
            {{ t('ctxCmds.programs.title', { n: fmtInt(log.programs.length) }) }}
            <i class="i-jannchie-info h-3 w-3 ink-soft opacity-70" aria-hidden="true" />
          </h3>
          <input
            v-if="log.programs.length > 6 || programQuery"
            v-model="programQuery"
            type="search"
            class="field py-1 text-xs"
            :placeholder="t('ctxCmds.programs.search')"
            :aria-label="t('ctxCmds.programs.search')"
          />
          <ul class="m-0 flex max-h-80 flex-col gap-px overflow-y-auto p-0">
            <li v-for="p in programList" :key="p.name" class="list-none">
              <button
                type="button"
                class="relative flex w-full items-center gap-2 overflow-hidden rounded-md border-0 bg-transparent px-1.5 py-0.5 text-left text-xs surface-hover kbd-ring"
                :class="program === p.name ? 'pill-selected' : ''"
                :aria-pressed="program === p.name"
                :title="program === p.name ? t('ctxCmds.programs.clear', { name: p.name }) : t('ctxCmds.programs.filter', { name: p.name })"
                @click="toggleProgram(p.name)"
              >
                <span class="absolute inset-y-0.5 left-0 rounded-sm surface" :style="{ width: `${(p.n / programMax) * 100}%` }" aria-hidden="true" />
                <span class="relative min-w-0 flex-1 truncate font-mono">{{ p.name }}</span>
                <span class="relative shrink-0 num ink-muted">×{{ fmtInt(p.n) }}</span>
                <span class="relative w-12 shrink-0 text-right num ink-soft">{{ t('ctxCmds.programs.runs', { n: fmtInt(p.runs) }, p.runs) }}</span>
              </button>
            </li>
          </ul>
        </section>
      </aside>

      <!-- 列表 -->
      <div class="flex min-w-0 flex-col gap-2">
        <div class="flex flex-wrap items-center gap-2" role="group" :aria-label="t('ctxCmds.filterLabel')">
          <button
            v-for="f in FILTER_KEYS"
            :key="f"
            type="button"
            class="segment border-0"
            :class="filter === f ? 'pill-selected' : 'surface ink-muted'"
            :aria-pressed="filter === f"
            :title="f === 'slow' ? t('ctxCmds.slowTip', { t: fmtMs(SLOW_MS) }) : undefined"
            @click="filter = f === filter ? 'all' : f"
          >
            {{ t(`ctxCmds.filter.${f}`) }} <span class="num">{{ fmtInt(counts[f]) }}</span>
          </button>
          <button
            v-if="program"
            type="button"
            class="segment border-0 pill-selected"
            :title="t('ctxCmds.programs.clear', { name: program })"
            @click="program = null"
          >
            <span class="font-mono">{{ program }}</span> <i class="i-jannchie-x h-3 w-3" aria-hidden="true" />
          </button>
          <span class="flex-1" />
          <input v-model="query" type="search" class="field max-w-56 py-1 text-xs" :placeholder="t('ctxCmds.search')" :aria-label="t('ctxCmds.search')" />
        </div>

        <p v-if="!shown.length" class="m-0 py-6 text-center text-xs ink-soft">{{ t('ctxCmds.noMatch') }}</p>
        <ul class="m-0 flex flex-col gap-px p-0">
          <li v-for="r in shown.slice(0, limit)" :key="rowKey(r)" class="list-none">
            <button
              type="button"
              class="w-full flex items-center gap-2 rounded-lg border-0 bg-transparent px-2 py-1 text-left text-xs surface-hover kbd-ring"
              :class="open.has(rowKey(r)) ? 'surface-strong' : ''"
              :aria-expanded="open.has(rowKey(r))"
              @click="toggle(r)"
            >
              <i
                class="h-3.5 w-3.5 shrink-0"
                :class="[STATUS_ICON[r.status], STATUS_INK[r.status]]"
                role="img"
                :aria-label="t(`ctxCmds.status.${r.status}`)"
                :title="t(`ctxCmds.status.${r.status}`)"
              />
              <span class="w-24 shrink-0 truncate font-mono ink-strong" :title="r.programs.join(' · ')">{{ r.program }}</span>
              <span class="min-w-0 flex-1 truncate">
                <span v-if="r.description" class="mr-2 ink-strong">{{ r.description }}</span>
                <span class="font-mono ink-soft">{{ preview(r.cmd) }}</span>
              </span>
              <span v-for="f in flagsOf(r)" :key="f.label" class="shrink-0 rounded px-1 surface max-sm:hidden" :class="f.ink" :title="f.title">{{ f.label }}</span>
              <span
                class="w-14 shrink-0 text-right num"
                :class="r.exit != null && r.exit !== 0 ? 'ink-danger' : 'ink-soft'"
                :title="r.exitInferred ? t('ctxCmds.exitInferredTip') : undefined"
              >
                <template v-if="r.exit != null">{{ t('ctxCmds.exit', { n: r.exit }) }}<template v-if="r.exitInferred">*</template></template>
              </span>
              <span class="w-16 shrink-0 text-right num" :class="r.ms != null && r.ms >= SLOW_MS ? 'ink-warning' : 'ink-muted'" :title="r.derived ? t('ctxCmds.derivedTip') : undefined">
                {{ r.ms != null ? fmtMs(r.ms) : r.status === 'running' ? '…' : '–' }}<template v-if="r.ms != null && r.derived">~</template>
              </span>
              <span v-if="stepLabel(r)" class="w-14 shrink-0 text-right num ink-soft" :title="stepLabel(r)!.title">{{ stepLabel(r)!.text }}</span>
              <TimeStamp v-if="r.ts" :ts="r.ts" class="w-16 shrink-0 text-right ink-soft max-sm:hidden" />
            </button>
            <div v-if="open.has(rowKey(r))" class="mb-1 ml-6 flex flex-col gap-1.5 border-l border-line py-1 pl-2">
              <pre class="payload-block max-h-80" :aria-label="t('ctxCmds.command')">{{ r.cmd }}</pre>
              <div class="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs ink-soft">
                <span class="font-mono ink-muted">{{ r.tool }}</span>
                <span :class="STATUS_INK[r.status]">{{ t(`ctxCmds.status.${r.status}`) }}</span>
                <span v-if="r.cwd" :title="`${t('ctxCmds.cwd')}: ${r.cwd}`"><i class="i-jannchie-folder mr-1 h-3 w-3" aria-hidden="true" /><span class="font-mono">{{ shortenPath(r.cwd, 48) }}</span></span>
                <span class="num">{{ t('ctxCmds.lines', { n: fmtInt(lineCount(r.cmd)) }, lineCount(r.cmd)) }}</span>
                <span v-if="r.timeoutMs != null" class="num">{{ t('ctxCmds.timeout', { t: fmtMs(r.timeoutMs) }) }}</span>
                <span v-if="r.outChars != null" class="num">{{ t('ctxCmds.output', { n: fmtInt(r.outChars) }) }}</span>
                <span v-if="r.sandbox">{{ t('ctxCmds.sandbox', { s: r.sandbox }) }}</span>
                <span v-for="f in flagsOf(r)" :key="f.label" class="sm:hidden" :class="f.ink">{{ f.label }}</span>
                <span class="flex-1" />
                <button
                  type="button"
                  class="flex items-center gap-1 rounded-md border-0 px-2 py-0.5 text-xs surface surface-hover kbd-ring"
                  :title="t('ctxCmds.revealTip')"
                  @click="emit('reveal', r.revealSeq, r)"
                >
                  <i class="i-jannchie-locate h-3 w-3" aria-hidden="true" />{{ t('ctxCmds.reveal') }}
                </button>
              </div>
            </div>
          </li>
        </ul>
        <button
          v-if="shown.length > limit"
          type="button"
          class="self-start rounded-md border-0 px-2 py-1 text-xs surface surface-hover kbd-ring"
          @click="limit += 100"
        >
          {{ t('ctxCmds.more', { n: fmtInt(Math.min(100, shown.length - limit)) }) }}
        </button>
      </div>
    </div>
  </DashCard>
</template>
