<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { api, ApiError, type SummaryFilter } from '../api'
import { ensurePricing, modelLabel, priceSummary, type PricedSummary } from '../lib/pricing'
import { fmtDateTime, tn } from '../lib/format'
import { type DateRange, dayCount, isDateStr, isEmpty, isPreset, isRangeEmpty, localDate, presetLabel, presetRange, sameRange, spanOf } from '../lib/summaryModel'
import { providerDot, providerName } from '../lib/providers'
import { projectOf } from '../lib/project'
import MultiSelect from '../components/MultiSelect.vue'
import RangePicker from '../components/dash/RangePicker.vue'
import KpiRow from '../components/dash/KpiRow.vue'
import CalendarHeat from '../components/dash/CalendarHeat.vue'
import DailyTrend from '../components/dash/DailyTrend.vue'
import CostShare from '../components/dash/CostShare.vue'
import TimeShare from '../components/dash/TimeShare.vue'
import WeekHourHeat from '../components/dash/WeekHourHeat.vue'
import CategoryBars from '../components/dash/CategoryBars.vue'
import ToolRank from '../components/dash/ToolRank.vue'
import ReviewCard from '../components/dash/ReviewCard.vue'
import TurnDurations from '../components/dash/TurnDurations.vue'
import CollabCard from '../components/dash/CollabCard.vue'
import ModelTable from '../components/dash/ModelTable.vue'
import EffortSkill from '../components/dash/EffortSkill.vue'
import ProjectTable from '../components/dash/ProjectTable.vue'
import SessionTable from '../components/dash/SessionTable.vue'
import Segmented from '../components/Segmented.vue'
import DashCard from '../components/dash/DashCard.vue'
import AtomSpinner from '../components/AtomSpinner.vue'

const route = useRoute()
const router = useRouter()
const { t } = useI18n()

const summary = ref<PricedSummary | null>(null)
const toolView = ref<'category' | 'tool'>('category')
const TOOL_VIEWS = computed(() => [
  { value: 'category' as const, label: t('dash.dashboard.byCategory') },
  { value: 'tool' as const, label: t('dash.dashboard.byTool') },
])
const error = ref<string | null>(null)
const loading = ref(false)
const elapsed = ref(0)
const loadedAt = ref<Date | null>(null)

/** 时间范围只认 URL query，刷新页面不丢：预设存名字（?range=month、?range=24h），自定义存本地日期（?from=&to=，两端都含）；都没有就是全部 */
const selKey = computed(() => {
  const q = route.query
  if (isPreset(q.range)) return q.range
  const from = isDateStr(q.from) ? q.from : ''
  const to = isDateStr(q.to) ? q.to : ''
  return from || to ? `${from}~${to}` : 'all'
})
/** 预设按这一刻落到日期上；每次加载都刷新，"最近 24 小时"才会往前走 */
const now = ref(new Date())
const range = computed<DateRange>(() => {
  const k = selKey.value
  if (isPreset(k)) return presetRange(k, now.value)
  const [from, to] = k.split('~')
  return { from: from || null, to: to || null, since: null, preset: null }
})

function setRange(r: DateRange) {
  if (sameRange(r, range.value)) return
  const query = { ...route.query }
  delete query.range
  delete query.from
  delete query.to
  if (r.preset && r.preset !== 'all') query.range = r.preset
  else if (!r.preset) {
    if (r.from) query.from = r.from
    if (r.to) query.to = r.to
  }
  router.replace({ query })
}

/** 过滤也在 URL 里：?agent=claude-code,pfh&model=…&project=…（项目是路径，可能含逗号，所以每个一项） */
const csv = (v: unknown) => (typeof v === 'string' && v ? v.split(',').filter(Boolean) : [])
const list = (v: unknown) => (v == null ? [] : (Array.isArray(v) ? v : [v]).map((x) => (typeof x === 'string' ? x : '')))
const filter = computed<SummaryFilter>(() => ({
  providers: csv(route.query.agent),
  models: csv(route.query.model),
  projects: list(route.query.project),
}))
const filterKey = computed(() => JSON.stringify(filter.value))
const filtered = computed(() => filter.value.providers.length + filter.value.models.length + filter.value.projects.length > 0)

function setFilter(key: 'agent' | 'model' | 'project', ids: string[]) {
  const query = { ...route.query }
  if (!ids.length) delete query[key]
  else query[key] = key === 'project' ? ids : ids.join(',')
  router.replace({ query })
}
const providerSel = computed({ get: () => filter.value.providers, set: (v: string[]) => setFilter('agent', v) })
const modelSel = computed({ get: () => filter.value.models, set: (v: string[]) => setFilter('model', v) })
const projectSel = computed({ get: () => filter.value.projects, set: (v: string[]) => setFilter('project', v) })
function clearFilter() {
  const query = { ...route.query }
  delete query.agent
  delete query.model
  delete query.project
  router.replace({ query })
}

/** 选项来自上一次汇总；过滤掉的也留着，可以再选回来 */
const providerOptions = computed(() =>
  (summary.value?.options.providers ?? filter.value.providers).map((p) => ({ id: p, label: providerName(p), dot: providerDot(p) })),
)
const modelOptions = computed(() => (summary.value?.options.models ?? filter.value.models).map((m) => ({ id: m, label: modelLabel(m) })))
/** 项目选项按侧栏的分组键合并（大小写不同的同一目录算一个）；目录名重名时显示完整路径 */
const projectOptions = computed(() => {
  const byKey = new Map<string, ReturnType<typeof projectOf>>()
  for (const cwd of summary.value?.options.projects ?? []) {
    const p = projectOf({ cwd })
    if (!byKey.has(p.key)) byKey.set(p.key, p)
  }
  // 选中的项目键本身就是规范化的路径，还没拿到选项时也能显示
  for (const k of filter.value.projects) if (!byKey.has(k)) byKey.set(k, projectOf({ cwd: k }))
  const ps = [...byKey.values()].sort((a, b) => a.label.localeCompare(b.label))
  const seen = new Map<string, number>()
  for (const p of ps) seen.set(p.label, (seen.get(p.label) ?? 0) + 1)
  return ps.map((p) => ({ id: p.key, label: (seen.get(p.label) ?? 0) > 1 ? p.path : p.label }))
})

let ac = new AbortController()
let timer: ReturnType<typeof setInterval> | null = null

async function load() {
  ac.abort()
  ac = new AbortController()
  error.value = null
  loading.value = true
  elapsed.value = 0
  const t0 = Date.now()
  timer = setInterval(() => (elapsed.value = Math.floor((Date.now() - t0) / 1000)), 500)
  try {
    // 东为正，与后端约定一致
    const tz = -new Date().getTimezoneOffset()
    now.value = new Date()
    const r = range.value
    // 滚动窗口同时带 from 和精确起点 since，后端取两者中更晚的
    const raw = await api.summary(tz, r.from, r.to, filter.value, ac.signal, r.since)
    // 先用手里的目录（首次是内置快照）出图，不等 4 MB 的 models.dev；拉到了再重算一次
    summary.value = priceSummary(raw)
    loadedAt.value = new Date()
    await ensurePricing()
    if (ac.signal.aborted) return
    summary.value = priceSummary(raw)
  } catch (e) {
    if (e instanceof DOMException && e.name === 'AbortError') return
    error.value = e instanceof ApiError ? e.message : e instanceof Error ? e.message : String(e)
  } finally {
    loading.value = false
    if (timer) clearInterval(timer)
    timer = null
  }
}

onMounted(load)
watch([selKey, filterKey], load)
onUnmounted(() => {
  ac.abort()
  if (timer) clearInterval(timer)
})

const empty = computed(() => summary.value != null && isEmpty(summary.value))
const rangeEmpty = computed(() => summary.value != null && !empty.value && isRangeEmpty(summary.value))

/** available 转成本地日期，给选择器定边界 */
const bounds = computed(() => {
  const a = summary.value?.available
  return {
    min: a?.[0] ? localDate(new Date(a[0])) : null,
    max: a?.[1] ? localDate(new Date(a[1])) : null,
  }
})
/** 范围跨几天（单边时另一边用 available 补），和趋势图取的是同一段 */
const spanDays = computed(() => {
  const span = summary.value && spanOf(range.value, bounds.value, summary.value.daily)
  return span ? dayCount(...span) : 0
})
const isAll = computed(() => !range.value.from && !range.value.to)
const rangeName = computed(() => (range.value.preset ? presetLabel(range.value.preset) : null))
/** 顶栏显示的跨度：有范围显示范围（单边时另一边用 available 补），全部时显示 available */
const spanLabel = computed(() => {
  const s = summary.value
  const b = bounds.value
  if (isAll.value) return b.min && b.max ? `${b.min} → ${b.max}` : null
  const since = range.value.since
  const from = since ? fmtDateTime(since).slice(0, 16) : (range.value.from ?? b.min ?? '…')
  const to = since ? t('dash.dashboard.now') : (range.value.to ?? b.max ?? '…')
  const days = s?.daily.length ?? 0
  return `${from} → ${to}${s ? ` · ${tn('dash.common.activeDays', days)}` : ''}`
})
const tzLabel = computed(() => {
  const m = summary.value?.tz_offset_min ?? -new Date().getTimezoneOffset()
  const sign = m >= 0 ? '+' : '−'
  const a = Math.abs(m)
  return `UTC${sign}${String(Math.floor(a / 60)).padStart(2, '0')}:${String(a % 60).padStart(2, '0')}`
})
</script>

<template>
  <div class="mx-auto w-full max-w-1200px px-6 pb-16 max-sm:px-4">
    <header class="sticky top-0 z-10 page-ground flex flex-wrap items-center gap-x-3 gap-y-2 py-4">
      <div class="min-w-0 flex flex-1 flex-wrap items-baseline gap-x-3 gap-y-1">
        <h1 class="m-0 text-base font-semibold">{{ t('common.nav.dashboard') }}</h1>
        <span v-if="spanLabel" class="text-xs ink-soft num">{{ rangeName ? `${rangeName} · ` : '' }}{{ spanLabel }}</span>
        <span class="text-xs ink-soft num" :title="t('dash.dashboard.tzTitle')">{{ tzLabel }}</span>
      </div>
      <RangePicker :model-value="range" :min="bounds.min" :max="bounds.max" :disabled="empty" @update:model-value="setRange" />
      <button
        type="button"
        class="icon-button border-0 bg-transparent"
        :disabled="loading"
        :title="
          loading
            ? t('dash.dashboard.computing')
            : loadedAt
              ? t('dash.dashboard.refreshAt', { time: fmtDateTime(loadedAt.toISOString()) })
              : t('dash.dashboard.refresh')
        "
        :aria-label="loading ? t('dash.dashboard.computingAria') : t('dash.dashboard.refresh')"
        @click="load()"
      >
        <AtomSpinner v-if="loading" class="h-4 w-4" />
        <i v-else class="i-jannchie-refresh h-4 w-4" />
      </button>
      <div class="w-full flex flex-wrap items-center gap-2">
        <MultiSelect v-model="providerSel" label="Agent" :options="providerOptions" :disabled="!summary" />
        <MultiSelect v-model="modelSel" :label="t('common.metric.model')" :options="modelOptions" :disabled="!summary" />
        <MultiSelect v-model="projectSel" :label="t('common.metric.project')" :options="projectOptions" :disabled="!summary" />
        <button v-if="filtered" type="button" class="rounded-full px-2.5 py-1 text-xs ink-soft surface-hover kbd-ring" @click="clearFilter">
          {{ t('dash.dashboard.clearFilter') }}
        </button>
        <span v-if="filter.models.length" class="text-xs ink-soft">{{ t('dash.dashboard.modelFilterNote') }}</span>
      </div>
    </header>

    <div v-if="error && !summary" role="alert" class="flex flex-col items-center gap-3 py-20 text-center text-sm">
      <span class="ink-danger">{{ t('common.loadFailed', { error }) }}</span>
      <i18n-t keypath="dash.dashboard.checkServe" tag="span" class="text-xs ink-soft" scope="global">
        <template #serve><code>harl serve</code></template>
        <template #api><code>/api/summary</code></template>
      </i18n-t>
      <button type="button" class="button-secondary bg-transparent" @click="load()">{{ t('common.retry') }}</button>
    </div>
    <!-- 刷新失败但手里还有上一次的数据：留着，只在上面提示 -->
    <div v-else-if="error" role="alert" class="mb-3 flex items-center gap-2 rounded-xl bg-red-500/10 px-3 py-2 text-sm ink-danger">
      <i class="i-jannchie-alert-triangle h-4 w-4 shrink-0" />
      <span class="min-w-0 flex-1">{{
        t('dash.dashboard.refreshFailed', { error, time: loadedAt ? fmtDateTime(loadedAt.toISOString()) : t('dash.dashboard.lastTime') })
      }}</span>
      <button type="button" class="shrink-0 rounded-md border-0 bg-transparent px-2 py-0.5 text-xs surface-hover" @click="load()">
        {{ t('common.retry') }}
      </button>
    </div>

    <div v-if="!error && !summary" role="status" class="flex flex-col items-center gap-2 py-24 text-sm ink-soft">
      <AtomSpinner class="h-8 w-8" />
      <span
        >{{ t('dash.dashboard.loadingAll') }}<span v-if="elapsed > 0" class="num">{{ t('dash.dashboard.elapsed', { s: elapsed }) }}</span></span
      >
      <span class="text-xs">{{ t('dash.dashboard.firstOpen') }}</span>
    </div>

    <i18n-t v-else-if="empty" keypath="dash.dashboard.empty" tag="div" class="py-24 text-center text-sm ink-soft" scope="global">
      <template #capture><code>harl capture</code></template>
      <template #link>
        <RouterLink :to="{ name: 'sessions' }" class="ink-accent">{{ t('dash.dashboard.sessionList') }}</RouterLink>
      </template>
    </i18n-t>

    <div v-else-if="rangeEmpty" class="flex flex-col items-center gap-2 py-24 text-sm ink-soft transition-opacity" :class="{ 'opacity-55': loading }">
      <template v-if="filtered">
        <span>{{ isAll ? t('dash.dashboard.noTurnsFilter') : t('dash.dashboard.noTurnsFilterRange') }}</span>
        <button type="button" class="button-secondary mt-2 bg-transparent" @click="clearFilter">{{ t('dash.dashboard.clearFilter') }}</button>
      </template>
      <template v-else>
        <i18n-t keypath="dash.dashboard.noTurnsRange" tag="span" scope="global">
          <template #range><span class="num">{{ range.from ?? '…' }} → {{ range.to ?? '…' }}</span></template>
        </i18n-t>
        <i18n-t keypath="dash.dashboard.dataSpan" tag="span" class="text-xs" scope="global">
          <template #span><span class="num">{{ bounds.min }} → {{ bounds.max }}</span></template>
        </i18n-t>
        <button type="button" class="button-secondary mt-2 bg-transparent" @click="setRange(presetRange('all'))">
          {{ t('dash.dashboard.viewAll') }}
        </button>
      </template>
    </div>

    <div v-else-if="summary" class="flex flex-col gap-3 transition-opacity" :class="{ 'opacity-55 pointer-events-none': loading }">
      <KpiRow :totals="summary.totals" :user-wait-ms="summary.by_category.user?.ms" :pricing="summary.pricing" />

      <CalendarHeat :daily="summary.daily" :range="range" :bounds="bounds" />
      <DailyTrend :daily="summary.daily" :hourly="summary.hourly" :by-model="summary.by_model" :range="range" :bounds="bounds" />

      <!-- 高度不一的卡片不并排：每块占满一行，宽度交给卡片内部分列 -->
      <CostShare :by-provider="summary.by_provider" :by-model="summary.by_model" :total-cost="summary.totals.cost_usd" />
      <TimeShare :totals="summary.totals" :by-model="summary.by_model" :by-category="summary.by_category" :by-tool="summary.by_tool" />
      <!-- 不满一周时每个星期几最多一天，这张图就只是换了排法的分时图 -->
      <WeekHourHeat v-if="spanDays >= 7" :hourly="summary.hourly" />
      <TurnDurations :d="summary.turn_durations" />
      <CollabCard
        v-if="summary.totals.signals.steers + summary.totals.signals.interrupts + summary.totals.signals.compactions"
        :signals="summary.totals.signals"
        :turns="summary.totals.turns"
      />

      <!-- 类别和单个工具是同一件事的两种粒度，放一块里切换 -->
      <DashCard
        icon="i-jannchie-toolbox"
        :title="t('common.metric.tools')"
        :title-hint="toolView === 'category' ? t('dash.dashboard.toolsHint') : undefined"
        :caption="
          toolView === 'tool'
            ? tn('dash.dashboard.toolCount', Object.keys(summary.by_tool).length)
            : undefined
        "
      >
        <template #controls>
          <Segmented v-model="toolView" :label="t('dash.dashboard.toolView')" :options="TOOL_VIEWS" />
        </template>
        <CategoryBars v-if="toolView === 'category'" :by-category="summary.by_category" />
        <ToolRank v-else :by-tool="summary.by_tool" />
      </DashCard>

      <ReviewCard v-if="summary.totals.review.reviews" :review="summary.totals.review" :tool-calls="summary.totals.tool_calls" />

      <ModelTable :by-model="summary.by_model" :total-cost="summary.totals.cost_usd" />
      <EffortSkill
        v-if="Object.keys(summary.by_effort).length || Object.keys(summary.by_skill).length"
        :by-effort="summary.by_effort"
        :by-skill="summary.by_skill"
        :total-steps="summary.totals.steps"
        :total-cost="summary.totals.cost_usd"
        :daily="summary.daily"
        :range="range"
        :bounds="bounds"
      />
      <ProjectTable :sessions="summary.sessions" @pick="(k) => setFilter('project', [k])" />
      <SessionTable :sessions="summary.sessions" />

      <p class="m-0 text-center text-xs ink-soft">
        {{ tn('dash.dashboard.footer', summary.totals.sessions) }}
      </p>
    </div>
  </div>
</template>
