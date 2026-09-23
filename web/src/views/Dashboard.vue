<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { api, ApiError } from '../api'
import { ensurePricing, priceSummary, type PricedSummary } from '../lib/pricing'
import { fmtDateTime, fmtInt } from '../lib/format'
import { type DateRange, isDateStr, isEmpty, isRangeEmpty, localDate, sameRange } from '../lib/summaryModel'
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

const route = useRoute()
const router = useRouter()

const summary = ref<PricedSummary | null>(null)
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
  const query: Record<string, string> = {}
  if (r.from) query.from = r.from
  if (r.to) query.to = r.to
  router.replace({ query })
}

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
    const raw = await api.summary(tz, range.value.from, range.value.to, ac.signal)
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
watch(range, load)
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
        <i class="h-4 w-4" :class="loading ? 'i-carbon-circle-dash animate-spin' : 'i-carbon-renew'" />
      </button>
    </header>

    <div v-if="error && !summary" class="flex flex-col items-center gap-3 py-20 text-center text-sm">
      <span class="ink-danger">加载失败：{{ error }}</span>
      <span class="text-xs ink-soft">确认 <code>harl serve</code> 在跑且版本带 <code>/api/summary</code>。</span>
      <button type="button" class="button-secondary bg-transparent" @click="load()">重试</button>
    </div>
    <!-- 刷新失败但手里还有上一次的数据：留着，只在上面提示 -->
    <div v-else-if="error" class="mb-3 flex items-center gap-2 rounded-xl bg-red-500/10 px-3 py-2 text-sm ink-danger">
      <i class="i-carbon-warning-alt h-4 w-4 shrink-0" />
      <span class="min-w-0 flex-1">刷新失败：{{ error }}（显示的是 {{ loadedAt ? fmtDateTime(loadedAt.toISOString()) : '上一次' }} 的数据）</span>
      <button type="button" class="shrink-0 rounded-md border-0 bg-transparent px-2 py-0.5 text-xs surface-hover" @click="load()">重试</button>
    </div>

    <div v-if="!error && !summary" class="flex flex-col items-center gap-2 py-24 text-sm ink-soft">
      <i class="i-carbon-circle-dash h-5 w-5 animate-spin text-blue-500" />
      <span>正在汇总全部会话<span v-if="elapsed > 0" class="num">（{{ elapsed }}s）</span></span>
      <span class="text-xs">首次打开要把每个会话的事件过一遍并缓存，之后只算新增。</span>
    </div>

    <div v-else-if="empty" class="py-24 text-center text-sm ink-soft">
      还没有任何会话可统计。先跑 <code>harl capture</code> 归档一些会话，或去
      <RouterLink :to="{ name: 'sessions' }" class="ink-accent">会话列表</RouterLink> 看看。
    </div>

    <div v-else-if="rangeEmpty" class="flex flex-col items-center gap-2 py-24 text-sm ink-soft transition-opacity" :class="{ 'opacity-55': loading }">
      <span><span class="num">{{ range.from ?? '…' }} → {{ range.to ?? '…' }}</span> 这段里没有任何 turn。</span>
      <span class="text-xs">有数据的区间是 <span class="num">{{ bounds.min }} → {{ bounds.max }}</span>。</span>
      <button type="button" class="button-secondary mt-2 bg-transparent" @click="setRange({ from: null, to: null })">看全部</button>
    </div>

    <div v-else-if="summary" class="flex flex-col gap-3 transition-opacity" :class="{ 'opacity-55 pointer-events-none': loading }">
      <KpiRow :totals="summary.totals" :user-wait-ms="summary.by_category.user?.ms" :pricing="summary.pricing" />

      <CalendarHeat :daily="summary.daily" :range="range" :bounds="bounds" />
      <DailyTrend :daily="summary.daily" :by-model="summary.by_model" :range="range" :bounds="bounds" />

      <div class="grid gap-3 lg:grid-cols-2 xl:grid-cols-3">
        <CostShare :by-provider="summary.by_provider" :by-model="summary.by_model" :total-cost="summary.totals.cost_usd" />
        <TimeShare :totals="summary.totals" :by-model="summary.by_model" :by-category="summary.by_category" :by-tool="summary.by_tool" />
        <WeekHourHeat class="lg:col-span-2 xl:col-span-1" :hourly="summary.hourly" />
      </div>

      <div class="grid gap-3 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.4fr)]">
        <CategoryBars :by-category="summary.by_category" />
        <ToolRank :by-tool="summary.by_tool" />
      </div>

      <ModelTable :by-model="summary.by_model" :total-cost="summary.totals.cost_usd" />

      <p class="m-0 text-center text-xs ink-soft">
        {{ fmtInt(summary.totals.sessions) }} 个会话 · 费用是按一方 API 价折算的等价费用，不是账单
      </p>
    </div>
  </div>
</template>
