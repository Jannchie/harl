<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { api, ApiError, type Summary } from '../api'
import { fmtDateTime, fmtInt } from '../lib/format'
import { type DateRange, isDateStr, isEmpty, isRangeEmpty, localDate, sameRange } from '../lib/summaryModel'
import RangePicker from '../components/dash/RangePicker.vue'
import KpiRow from '../components/dash/KpiRow.vue'
import CalendarHeat from '../components/dash/CalendarHeat.vue'
import DailyTrend from '../components/dash/DailyTrend.vue'
import CostSunburst from '../components/dash/CostSunburst.vue'
import TimeSunburst from '../components/dash/TimeSunburst.vue'
import WeekHourHeat from '../components/dash/WeekHourHeat.vue'
import CategoryBars from '../components/dash/CategoryBars.vue'
import ToolRank from '../components/dash/ToolRank.vue'
import SessionTreemap from '../components/dash/SessionTreemap.vue'
import ModelTable from '../components/dash/ModelTable.vue'

const route = useRoute()
const router = useRouter()

const summary = ref<Summary | null>(null)
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
    summary.value = await api.summary(tz, range.value.from, range.value.to, ac.signal)
    loadedAt.value = new Date()
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
  <div class="page dash">
    <div class="bar">
      <div class="lead">
        <span class="title">仪表盘</span>
        <span v-if="spanLabel" class="muted range mono">
          <span v-if="isAll" class="all">全部</span>{{ spanLabel }}
        </span>
        <span class="muted tz mono" title="后端按这个时区切本地日期 / 小时">{{ tzLabel }}</span>
      </div>
      <div class="spacer"></div>
      <RangePicker :model-value="range" :min="bounds.min" :max="bounds.max" :disabled="empty" @update:model-value="setRange" />
      <span v-if="loadedAt && !loading" class="muted at mono" :title="`上次加载 ${fmtDateTime(loadedAt.toISOString())}`">{{ fmtDateTime(loadedAt.toISOString()).slice(11, 16) }}</span>
      <button type="button" @click="load()" :disabled="loading">{{ loading ? '计算中…' : '刷新' }}</button>
    </div>

    <div v-if="error && !summary" class="state error">
      加载失败：<code>{{ error }}</code>
      <div class="hint muted">确认 <code>harl serve</code> 在跑且版本带 <code>/api/summary</code>。<button type="button" @click="load()">重试</button></div>
    </div>
    <!-- 刷新失败但手里还有上一次的数据：留着，只在上面提示 -->
    <div v-else-if="error" class="state error inline">
      刷新失败：<code>{{ error }}</code>（显示的是 {{ loadedAt ? fmtDateTime(loadedAt.toISOString()) : '上一次' }} 的数据）
      <button type="button" @click="load()">重试</button>
    </div>

    <div v-if="!error && !summary" class="state loading">
      <div class="pulse" aria-hidden="true"><span></span><span></span><span></span></div>
      <div>正在汇总全部会话{{ elapsed > 0 ? `（${elapsed}s）` : '' }}</div>
      <div class="hint muted">首次打开要把每个会话的事件过一遍并缓存，通常 5 秒以内；之后只算新增。</div>
    </div>

    <div v-else-if="empty" class="state">
      还没有任何会话可统计。先跑 <code>harl capture</code> 归档一些会话，或去
      <RouterLink :to="{ name: 'sessions' }">会话列表</RouterLink> 看看。
    </div>

    <div v-else-if="rangeEmpty" class="state range-empty" :class="{ stale: loading }">
      <div>
        <span class="mono">{{ range.from ?? '…' }} → {{ range.to ?? '…' }}</span> 这段里没有任何 turn。
      </div>
      <div class="hint muted">
        有数据的区间是 <span class="mono">{{ bounds.min }} → {{ bounds.max }}</span>。
        <button type="button" @click="setRange({ from: null, to: null })">看全部</button>
      </div>
    </div>

    <div v-else-if="summary" class="grid" :class="{ stale: loading }">
      <section class="kpi">
        <header class="sec-head">
          <span class="n mono">01</span>
          <span class="t">总览</span>
          <span class="muted c">{{ fmtInt(summary.totals.sessions) }} 个会话 · 费用是 API 等价，不是账单</span>
        </header>
        <KpiRow :totals="summary.totals" :user-wait-ms="summary.by_category.user?.ms" />
      </section>

      <CalendarHeat class="span-all" :daily="summary.daily" :range="range" :bounds="bounds" />
      <DailyTrend class="span-all" :daily="summary.daily" :by-model="summary.by_model" :range="range" :bounds="bounds" />

      <div class="row3">
        <CostSunburst :by-provider="summary.by_provider" :by-model="summary.by_model" :total-cost="summary.totals.cost_usd" />
        <TimeSunburst :totals="summary.totals" :by-model="summary.by_model" :by-tool="summary.by_tool" />
        <WeekHourHeat :hourly-turns="summary.hourly_turns" :hourly-cost="summary.hourly_cost" />
      </div>

      <div class="row2">
        <CategoryBars :by-category="summary.by_category" />
        <ToolRank :by-tool="summary.by_tool" />
      </div>

      <SessionTreemap class="span-all" :sessions="summary.sessions" />

      <ModelTable class="span-all" :by-model="summary.by_model" :total-cost="summary.totals.cost_usd" />
    </div>
  </div>
</template>

<style scoped>
.dash {
  max-width: 1500px;
}
.bar {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 0 8px;
  position: sticky;
  top: 32px;
  background: var(--bg);
  z-index: 5;
  border-bottom: 1px solid var(--line-1);
  margin-bottom: 12px;
  flex-wrap: wrap;
}
.lead {
  display: flex;
  align-items: baseline;
  gap: 12px;
  flex-wrap: wrap;
}
.title {
  font-weight: 600;
  letter-spacing: 0.01em;
}
.range,
.tz,
.at {
  font-size: var(--fs-xs);
}
.all {
  display: inline-block;
  border: 1px solid var(--line);
  border-radius: var(--radius);
  padding: 0 4px;
  margin-right: 6px;
  color: var(--fg-1);
  line-height: 1.3;
}
.range-empty {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 48px 16px;
  align-items: center;
  color: var(--fg-1);
}
.range-empty.stale {
  opacity: 0.55;
}
.spacer {
  flex: 1;
}
.grid {
  display: grid;
  gap: 12px;
  transition: opacity 0.2s;
}
.grid.stale {
  opacity: 0.55;
  pointer-events: none;
}
.sec-head {
  display: flex;
  align-items: baseline;
  gap: 8px;
  padding: 0 0 6px;
}
.sec-head .n {
  font-size: var(--fs-xs);
  color: var(--fg-2);
  letter-spacing: 0.08em;
}
.sec-head .t {
  font-weight: 600;
}
.sec-head .c {
  font-size: var(--fs-xs);
}
.row3 {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 12px;
}
.row2 {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1.5fr);
  gap: 12px;
  align-items: start;
}
.state.loading {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  padding: 64px 16px;
  color: var(--fg-1);
}
.hint {
  font-size: var(--fs-xs);
}
.hint button {
  margin-left: 8px;
}
.state.error.inline {
  padding: 6px 0 10px;
  font-size: var(--fs-sm);
}
.state.error.inline button {
  margin-left: 8px;
  color: var(--fg);
}
.pulse {
  display: flex;
  gap: 5px;
}
.pulse span {
  width: 6px;
  height: 18px;
  background: var(--seq-4);
  animation: pulse 1s ease-in-out infinite;
}
.pulse span:nth-child(2) {
  animation-delay: 0.15s;
}
.pulse span:nth-child(3) {
  animation-delay: 0.3s;
}
@keyframes pulse {
  0%,
  100% {
    transform: scaleY(0.35);
    opacity: 0.45;
  }
  50% {
    transform: scaleY(1);
    opacity: 1;
  }
}
@media (prefers-reduced-motion: reduce) {
  .pulse span {
    animation: none;
  }
}
@media (max-width: 1200px) {
  .row3 {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
  .row3 > :last-child {
    grid-column: 1 / -1;
  }
}
@media (max-width: 900px) {
  .row3,
  .row2 {
    grid-template-columns: minmax(0, 1fr);
  }
}
</style>
