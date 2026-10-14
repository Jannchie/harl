<script setup lang="ts">
import { computed, nextTick, onUnmounted, ref, useId, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  addDays,
  CALENDAR_PRESETS,
  type DateRange,
  localDate,
  monthStart,
  presetLabel,
  presetRange,
  presetShortLabel,
  type RangePreset,
  ROLLING,
} from '../../lib/summaryModel'
import { fmtDayFull, fmtMonthYear, weekdayNames } from '../../lib/format'

// 时间范围：一个触发器 + 一块面板。左边是分组的预设（日历上的今天 / 本周 / 本月 / 今年和上一个，滚动的最近 N 小时 / 天），
// 右边是两个月的日历，点两下选一段（第二下点同一天就是单日）。选中的范围不管来自预设还是自定义都画在日历上。
//
// 键盘：触发器 ↓ / Enter / Space 打开，焦点落到当前选中的预设上；Tab 在预设和日历之间走。
// 日历里 ←→ 一天、↑↓ 一周、PageUp / PageDown 一个月（加 Shift 一年）、Home / End 到周一 / 周日，Enter / Space 点选。
// Esc 先取消选到一半的起点，再关闭并把焦点还给触发器。
const props = defineProps<{
  modelValue: DateRange
  /** 全部数据的本地日期边界：早于 min 的日子不能点 */
  min?: string | null
  max?: string | null
  disabled?: boolean
}>()
const emit = defineEmits<{ 'update:modelValue': [r: DateRange] }>()

const { t } = useI18n()
const uid = useId()
const open = ref(false)
const root = ref<HTMLElement | null>(null)
const trigger = ref<HTMLButtonElement | null>(null)
const panel = ref<HTMLElement | null>(null)

const today = ref(localDate(new Date()))
const short = (d: string) => (d.slice(0, 4) === today.value.slice(0, 4) ? d.slice(5) : d)

/** 触发器上的字：预设给名字，自定义给日期 */
const summary = computed(() => {
  const r = props.modelValue
  if (r.preset) return presetLabel(r.preset)
  if (r.from && r.to) return r.from === r.to ? short(r.from) : `${short(r.from)} → ${short(r.to)}`
  return `${r.from ? short(r.from) : '…'} → ${r.to ? short(r.to) : '…'}`
})

function pickPreset(p: RangePreset) {
  emit('update:modelValue', presetRange(p))
  closePanel(true)
}

// ---- 日历 ----

/** 右边那个月的 1 号；左边是它的上个月。窄屏只显示右边的 */
const view = ref(monthStart(today.value))
/** 选到一半的起点 */
const anchor = ref<string | null>(null)
/** 鼠标悬停 / 键盘停在哪天：起点选好后用来预览 */
const hover = ref<string | null>(null)
/** 日历里唯一的 Tab 停靠点 */
const cursor = ref(today.value)

/** 加 n 个月，日子超出就夹到月底（1-31 加一个月是 2-28 / 29） */
function addMonths(day: string, n: number): string {
  const d = new Date(`${day}T00:00:00`)
  const last = new Date(d.getFullYear(), d.getMonth() + n + 1, 0).getDate()
  return localDate(new Date(d.getFullYear(), d.getMonth() + n, Math.min(d.getDate(), last)))
}

const clamp = (d: string) => (props.min && d < props.min ? props.min : d > today.value ? today.value : d)
const disabledDay = (d: string) => (props.min != null && d < props.min) || d > today.value

/** 日历表头：周一到周日的单字 */
const weekHead = computed(() => weekdayNames('narrow'))

const months = computed(() =>
  [addMonths(view.value, -1), view.value].map((m) => {
    const first = new Date(`${m}T00:00:00`)
    const lead = (first.getDay() + 6) % 7
    const n = new Date(first.getFullYear(), first.getMonth() + 1, 0).getDate()
    const cells: (string | null)[] = Array.from({ length: lead }, () => null)
    for (let i = 0; i < n; i++) cells.push(addDays(m, i))
    while (cells.length % 7) cells.push(null)
    return { key: m, title: fmtMonthYear(first), cells }
  }),
)

/** 要画出来的那一段：选到一半时是起点到悬停处，否则是当前范围（单边时另一边用数据边界补） */
const shown = computed<[string, string] | null>(() => {
  if (anchor.value) {
    const h = hover.value ?? anchor.value
    return anchor.value <= h ? [anchor.value, h] : [h, anchor.value]
  }
  const r = props.modelValue
  const from = r.from ?? props.min
  const to = r.to ?? props.max
  return from && to ? [from, to] : null
})

const isEdge = (d: string) => shown.value != null && (d === shown.value[0] || d === shown.value[1])

function dayClass(d: string) {
  const s = shown.value
  const edge = isEdge(d)
  const inside = s && d > s[0] && d < s[1]
  // 选中段画成一条连续的带子：中间不圆角，两头各圆一边
  const start = s && d === s[0]
  const end = s && d === s[1]
  return [
    edge ? 'bg-blue-600 text-white' : inside ? 'bg-blue-500/12' : '',
    inside ? 'rounded-none' : start && !end ? 'rounded-r-none' : end && !start ? 'rounded-l-none' : '',
    !edge && !disabledDay(d) ? 'hover:bg-black/6 dark:hover:bg-white/10' : '',
    d === today.value && !edge ? 'font-semibold ink-accent' : '',
    disabledDay(d) ? 'opacity-30 cursor-not-allowed' : 'cursor-pointer',
  ]
}

function dayLabel(d: string) {
  return fmtDayFull(new Date(`${d}T00:00:00`))
}

function clickDay(d: string) {
  if (disabledDay(d)) return
  cursor.value = d
  if (!anchor.value) {
    anchor.value = d
    hover.value = d
    return
  }
  const [from, to] = anchor.value <= d ? [anchor.value, d] : [d, anchor.value]
  anchor.value = null
  emit('update:modelValue', { from, to, since: null, preset: null })
  closePanel(true)
}

/** 键盘把光标挪出可见的两个月时翻页，然后把焦点给那一天 */
async function moveCursor(d: string) {
  d = clamp(d)
  cursor.value = d
  if (anchor.value) hover.value = d
  const m = monthStart(d)
  if (m > view.value) view.value = m
  else if (m < addMonths(view.value, -1)) view.value = addMonths(m, 1)
  await nextTick()
  panel.value?.querySelector<HTMLButtonElement>(`[data-day="${d}"]`)?.focus()
}

function onDayKey(e: KeyboardEvent, d: string) {
  const dow = (new Date(`${d}T00:00:00`).getDay() + 6) % 7
  const step: Record<string, () => string> = {
    ArrowLeft: () => addDays(d, -1),
    ArrowRight: () => addDays(d, 1),
    ArrowUp: () => addDays(d, -7),
    ArrowDown: () => addDays(d, 7),
    PageUp: () => addMonths(d, e.shiftKey ? -12 : -1),
    PageDown: () => addMonths(d, e.shiftKey ? 12 : 1),
    Home: () => addDays(d, -dow),
    End: () => addDays(d, 6 - dow),
  }
  const f = step[e.key]
  if (!f) return
  e.preventDefault()
  void moveCursor(f())
}

/** 光标所在的月不可见时（翻了页），Tab 停靠点挪到可见范围里 */
const tabDay = computed(() => {
  const lo = addMonths(view.value, -1)
  const hi = addMonths(view.value, 1)
  if (cursor.value >= lo && cursor.value < hi) return cursor.value
  return clamp(view.value)
})

// ---- 开关 ----

async function openPanel() {
  if (props.disabled) return
  today.value = localDate(new Date())
  const r = props.modelValue
  const end = clamp(r.to ?? props.max ?? today.value)
  view.value = monthStart(end)
  cursor.value = end
  anchor.value = null
  hover.value = null
  open.value = true
  await nextTick()
  ;(panel.value?.querySelector<HTMLElement>('[aria-pressed="true"]') ?? panel.value?.querySelector<HTMLElement>('button'))?.focus()
}

function closePanel(returnFocus: boolean) {
  if (!open.value) return
  open.value = false
  anchor.value = null
  if (returnFocus) trigger.value?.focus()
}

function onTriggerKey(e: KeyboardEvent) {
  if (e.key === 'ArrowDown' || e.key === 'Enter' || e.key === ' ') {
    e.preventDefault()
    void openPanel()
  }
}

function onPanelKey(e: KeyboardEvent) {
  if (e.key !== 'Escape') return
  e.preventDefault()
  e.stopPropagation()
  if (anchor.value) {
    anchor.value = null
    hover.value = null
  } else closePanel(true)
}

function onFocusOut(e: FocusEvent) {
  const next = e.relatedTarget as Node | null
  if (next && root.value?.contains(next)) return
  closePanel(false)
}
function onOutside(e: PointerEvent) {
  if (root.value && !root.value.contains(e.target as Node)) closePanel(false)
}
watch(open, (v) => {
  if (v) document.addEventListener('pointerdown', onOutside)
  else document.removeEventListener('pointerdown', onOutside)
})
watch(
  () => props.disabled,
  (d) => d && closePanel(false),
)
onUnmounted(() => document.removeEventListener('pointerdown', onOutside))

const active = (p: RangePreset) => props.modelValue.preset === p
const groups = computed<{ id: string; title: string; items: RangePreset[] }[]>(() => [
  { id: 'cal', title: t('dash.common.calendar'), items: CALENDAR_PRESETS },
  { id: 'roll', title: t('dash.rangePicker.groupRolling'), items: ROLLING.map((r) => r.key) },
])
const presetBtn = (p: RangePreset) => [
  'rounded-md border-0 px-2.5 py-1.5 text-left text-sm kbd-ring transition-quick',
  active(p) ? 'pill-selected' : 'bg-transparent surface-hover',
]
const navBtn = 'icon-button h-8 w-8 border-0 bg-transparent disabled:opacity-30'
const atLatest = computed(() => view.value >= monthStart(today.value))
</script>

<template>
  <div ref="root" class="relative" :class="{ 'opacity-50 pointer-events-none': disabled }" @focusout="onFocusOut">
    <button
      ref="trigger"
      type="button"
      class="flex items-center gap-1.5 rounded-lg px-2.5 py-1 text-xs kbd-ring transition-quick surface surface-hover ink-strong disabled:opacity-50"
      :disabled="disabled"
      aria-haspopup="dialog"
      :aria-expanded="open"
      :aria-controls="open ? `${uid}-panel` : undefined"
      :aria-label="t('dash.rangePicker.triggerAria', { summary })"
      @click="open ? closePanel(false) : openPanel()"
      @keydown="onTriggerKey"
    >
      <i class="i-jannchie-calendar h-3.5 w-3.5 ink-soft" aria-hidden="true" />
      <span class="num" aria-hidden="true">{{ summary }}</span>
      <i class="i-jannchie-chevron-down h-3 w-3 opacity-70 transition-transform duration-150" :class="open ? 'rotate-180' : ''" aria-hidden="true" />
    </button>

    <div
      v-if="open"
      :id="`${uid}-panel`"
      ref="panel"
      role="dialog"
      :aria-label="t('dash.rangePicker.dialog')"
      class="popover absolute right-0 top-full mt-2 flex gap-2 p-3 max-sm:fixed max-sm:inset-x-4 max-sm:top-16 max-sm:flex-col sm:w-max"
      @keydown="onPanelKey"
    >
      <!-- 预设 -->
      <div class="flex flex-col gap-3 sm:w-44">
        <button type="button" :class="presetBtn('all')" :aria-pressed="active('all')" @click="pickPreset('all')">
          {{ t('dash.rangePicker.allTime') }}
        </button>
        <section v-for="g in groups" :key="g.id" :aria-labelledby="`${uid}-${g.id}`">
          <h3 :id="`${uid}-${g.id}`" class="m-0 mb-1 px-2.5 text-xs font-normal ink-soft">{{ g.title }}</h3>
          <div class="grid grid-cols-2 gap-0.5">
            <button
              v-for="p in g.items"
              :key="p"
              type="button"
              :class="presetBtn(p)"
              :aria-pressed="active(p)"
              :aria-label="presetLabel(p)"
              @click="pickPreset(p)"
            >
              {{ presetShortLabel(p) }}
            </button>
          </div>
        </section>
      </div>

      <!-- 自定义：两个月的日历，点两下选一段 -->
      <div class="flex gap-6 sm:border-l sm:border-l-solid sm:border-black/8 sm:pl-4 dark:sm:border-white/8" @pointerleave="anchor && (hover = anchor)">
        <div v-for="(m, mi) in months" :key="m.key" :class="mi === 0 ? 'max-sm:hidden' : 'max-sm:flex-1'">
          <div class="mb-1 flex h-8 items-center">
            <button
              type="button"
              :class="[navBtn, mi === 0 ? '' : 'sm:invisible']"
              :aria-label="t('dash.rangePicker.prevMonth')"
              :title="t('dash.rangePicker.prevMonth')"
              @click="view = addMonths(view, -1)"
            >
              <i class="i-jannchie-chevron-left h-4 w-4" />
            </button>
            <p class="m-0 flex-1 text-center text-sm font-medium ink-strong">{{ m.title }}</p>
            <button
              type="button"
              :class="[navBtn, mi === 0 ? 'invisible' : '']"
              :aria-label="t('dash.rangePicker.nextMonth')"
              :title="t('dash.rangePicker.nextMonth')"
              :disabled="atLatest"
              @click="view = addMonths(view, 1)"
            >
              <i class="i-jannchie-chevron-right h-4 w-4" />
            </button>
          </div>
          <div class="grid grid-cols-[repeat(7,2.25rem)] gap-y-0.5 max-sm:grid-cols-7" role="group" :aria-label="m.title">
            <span v-for="(w, wi) in weekHead" :key="wi" class="h-8 text-center text-xs leading-8 ink-soft" aria-hidden="true">{{ w }}</span>
            <template v-for="(d, i) in m.cells" :key="d ?? `pad-${i}`">
              <span v-if="!d" aria-hidden="true" />
              <button
                v-else
                type="button"
                :data-day="d"
                class="h-9 w-full rounded-lg border-0 bg-transparent p-0 text-sm num kbd-ring-flush"
                :class="dayClass(d)"
                :aria-label="dayLabel(d)"
                :aria-pressed="isEdge(d)"
                :aria-disabled="disabledDay(d) || undefined"
                :aria-current="d === today ? 'date' : undefined"
                :tabindex="d === tabDay ? 0 : -1"
                @click="clickDay(d)"
                @pointerenter="anchor && !disabledDay(d) && (hover = d)"
                @keydown="onDayKey($event, d)"
              >
                {{ Number(d.slice(8)) }}
              </button>
            </template>
          </div>
        </div>
      </div>
      <p class="sr-only" aria-live="polite">{{ anchor ? t('dash.rangePicker.anchorHint', { date: anchor }) : '' }}</p>
    </div>
  </div>
</template>
