<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { api, ApiError, type SummaryFilter } from '../api'
import { ensurePricing, priceSummary, type PricedSummary } from '../lib/pricing'
import { fmtDateTime, fmtInt } from '../lib/format'
import { type DateRange, isDateStr, isEmpty, isRangeEmpty, localDate, sameRange } from '../lib/summaryModel'
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
import ModelTable from '../components/dash/ModelTable.vue'
import ProjectTable from '../components/dash/ProjectTable.vue'
import SessionTable from '../components/dash/SessionTable.vue'
import Segmented from '../components/Segmented.vue'
import DashCard from '../components/dash/DashCard.vue'
import AtomSpinner from '../components/AtomSpinner.vue'

const route = useRoute()
const router = useRouter()

const summary = ref<PricedSummary | null>(null)
const toolView = ref<'category' | 'tool'>('category')
const TOOL_VIEWS = [
  { value: 'category', label: '按类别' },
  { value: 'tool', label: '按工具' },
] as const
const error = ref<string | null>(null)
const loading = ref(false)
const elapsed = ref(0)
const loadedAt = ref<Date | null>(null)

/** 时间范围只认 URL query（?from=&to=），刷新页面不丢；两端都含，本地日期 */
const range = computed<DateRange>(() => ({
  from: isDateStr(route.query.from) ? route.query.from : null,
  to: isDateStr(route.query.to) ? route.query.to : null,
}))

function setRange(r: DateRange) {
  if (sameRange(r, range.value)) return
  const query = { ...route.query }
  delete query.from
  delete query.to
  if (r.from) query.from = r.from
  if (r.to) query.to = r.to
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
const modelOptions = computed(() => (summary.value?.options.models ?? filter.value.models).map((m) => ({ id: m, label: m })))
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
    const raw = await api.summary(tz, range.value.from, range.value.to, filter.value, ac.signal)
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
watch([range, filterKey], load)
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
const isAll = computed(() => !range.value.from && !range.value.to)
/** 顶栏显示的跨度：有范围显示范围（单边时另一边用 available 补），全部时显示 available */
const spanLabel = computed(() => {
  const s = summary.value
  const b = bounds.value
  if (isAll.value) return b.min && b.max ? `${b.min} → ${b.max}` : null
  const from = range.value.from ?? b.min ?? '…'
  const to = range.value.to ?? b.max ?? '…'
  const days = s?.daily.length ?? 0
  return `${from} → ${to}${s ? ` · ${days} 天有活动` : ''}`
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
        <h1 class="m-0 text-base font-semibold">概览</h1>
        <span v-if="spanLabel" class="text-xs ink-soft num">{{ isAll ? '全部 · ' : '' }}{{ spanLabel }}</span>
        <span class="text-xs ink-soft num" title="后端按这个时区切本地日期 / 小时">{{ tzLabel }}</span>
      </div>
      <RangePicker :model-value="range" :min="bounds.min" :max="bounds.max" :disabled="empty" @update:model-value="setRange" />
      <button
        type="button"
        class="icon-button border-0 bg-transparent"
        :disabled="loading"
        :title="loading ? '计算中…' : loadedAt ? `刷新（上次 ${fmtDateTime(loadedAt.toISOString())}）` : '刷新'"
        :aria-label="loading ? '计算中' : '刷新'"
        @click="load()"
      >
        <AtomSpinner v-if="loading" class="h-4 w-4" />
        <i v-else class="i-carbon-renew h-4 w-4" />
      </button>
      <div class="w-full flex flex-wrap items-center gap-2">
        <MultiSelect v-model="providerSel" label="Agent" :options="providerOptions" :disabled="!summary" />
        <MultiSelect v-model="modelSel" label="模型" :options="modelOptions" :disabled="!summary" />
        <MultiSelect v-model="projectSel" label="项目" :options="projectOptions" :disabled="!summary" />
        <button v-if="filtered" type="button" class="rounded-full px-2.5 py-1 text-xs ink-soft surface-hover kbd-ring" @click="clearFilter">
          清除筛选
        </button>
        <span v-if="filter.models.length" class="text-xs ink-soft">按模型筛选时，工具和墙钟时间按整轮计入</span>
      </div>
    </header>

    <div v-if="error && !summary" role="alert" class="flex flex-col items-center gap-3 py-20 text-center text-sm">
      <span class="ink-danger">加载失败：{{ error }}</span>
      <span class="text-xs ink-soft">确认 <code>harl serve</code> 在跑且版本带 <code>/api/summary</code>。</span>
      <button type="button" class="button-secondary bg-transparent" @click="load()">重试</button>
    </div>
    <!-- 刷新失败但手里还有上一次的数据：留着，只在上面提示 -->
    <div v-else-if="error" role="alert" class="mb-3 flex items-center gap-2 rounded-xl bg-red-500/10 px-3 py-2 text-sm ink-danger">
      <i class="i-carbon-warning-alt h-4 w-4 shrink-0" />
      <span class="min-w-0 flex-1">刷新失败：{{ error }}（显示的是 {{ loadedAt ? fmtDateTime(loadedAt.toISOString()) : '上一次' }} 的数据）</span>
      <button type="button" class="shrink-0 rounded-md border-0 bg-transparent px-2 py-0.5 text-xs surface-hover" @click="load()">重试</button>
    </div>

    <div v-if="!error && !summary" role="status" class="flex flex-col items-center gap-2 py-24 text-sm ink-soft">
      <AtomSpinner class="h-8 w-8" />
      <span>正在汇总全部会话<span v-if="elapsed > 0" class="num">（{{ elapsed }}s）</span></span>
      <span class="text-xs">首次打开要把每个会话的事件过一遍并缓存，之后只算新增。</span>
    </div>

    <div v-else-if="empty" class="py-24 text-center text-sm ink-soft">
      还没有任何会话可统计。先跑 <code>harl capture</code> 归档一些会话，或去
      <RouterLink :to="{ name: 'sessions' }" class="ink-accent">会话列表</RouterLink> 看看。
    </div>

    <div v-else-if="rangeEmpty" class="flex flex-col items-center gap-2 py-24 text-sm ink-soft transition-opacity" :class="{ 'opacity-55': loading }">
      <template v-if="filtered">
        <span>当前筛选<template v-if="!isAll">和时间范围</template>下没有任何 turn。</span>
        <button type="button" class="button-secondary mt-2 bg-transparent" @click="clearFilter">清除筛选</button>
      </template>
      <template v-else>
        <span><span class="num">{{ range.from ?? '…' }} → {{ range.to ?? '…' }}</span> 这段里没有任何 turn。</span>
        <span class="text-xs">有数据的区间是 <span class="num">{{ bounds.min }} → {{ bounds.max }}</span>。</span>
        <button type="button" class="button-secondary mt-2 bg-transparent" @click="setRange({ from: null, to: null })">看全部</button>
      </template>
    </div>

    <div v-else-if="summary" class="flex flex-col gap-3 transition-opacity" :class="{ 'opacity-55 pointer-events-none': loading }">
      <KpiRow :totals="summary.totals" :user-wait-ms="summary.by_category.user?.ms" :pricing="summary.pricing" />

      <CalendarHeat :daily="summary.daily" :range="range" :bounds="bounds" />
      <DailyTrend :daily="summary.daily" :hourly="summary.hourly" :by-model="summary.by_model" :range="range" :bounds="bounds" />

      <!-- 高度不一的卡片不并排：每块占满一行，宽度交给卡片内部分列 -->
      <CostShare :by-provider="summary.by_provider" :by-model="summary.by_model" :total-cost="summary.totals.cost_usd" />
      <TimeShare :totals="summary.totals" :by-model="summary.by_model" :by-category="summary.by_category" :by-tool="summary.by_tool" />
      <WeekHourHeat :hourly="summary.hourly" />

      <!-- 类别和单个工具是同一件事的两种粒度，放一块里切换 -->
      <DashCard
        icon="i-carbon-tool-box"
        title="工具"
        :title-hint="toolView === 'category' ? '「等用户」类别的耗时是在等人回答，不是执行时间' : undefined"
        :caption="toolView === 'tool' ? `共 ${Object.keys(summary.by_tool).length} 个工具` : undefined"
      >
        <template #controls>
          <Segmented v-model="toolView" label="工具视图" :options="TOOL_VIEWS" />
        </template>
        <CategoryBars v-if="toolView === 'category'" :by-category="summary.by_category" />
        <ToolRank v-else :by-tool="summary.by_tool" />
      </DashCard>

      <ModelTable :by-model="summary.by_model" :total-cost="summary.totals.cost_usd" />
      <ProjectTable :sessions="summary.sessions" @pick="(k) => setFilter('project', [k])" />
      <SessionTable :sessions="summary.sessions" />

      <p class="m-0 text-center text-xs ink-soft">
        {{ fmtInt(summary.totals.sessions) }} 个会话 · 费用是按一方 API 价折算的等价费用，不是账单
      </p>
    </div>
  </div>
</template>
