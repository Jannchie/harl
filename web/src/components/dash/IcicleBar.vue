<script setup lang="ts">
// 两层冰柱条 + 分组图例：每组一列，上面一条细的组色，下面是组内各项；列宽按 total 的占比，所以上下同一刻度。
// 条和图例悬停联动：悬停一组或一项，其余变淡。
import { computed, ref } from 'vue'
import { useChartTheme } from '../../lib/chartTheme'
import { fmtPct } from '../../lib/format'
import { NARROW, useMediaQuery, WIDE } from '../../lib/media'

export interface ShareSeg {
  key: string
  name: string
  value: number
  color: string
  /** 图例里名字后面的小字 */
  hint?: string
  /** hover 时的说明（title） */
  tip?: string
}

export interface IcicleGroup {
  key: string
  name: string
  value: number
  /** 上层色块 / 图例方块的颜色 */
  cap: string
  segs: ShareSeg[]
}

const props = defineProps<{
  /** 标题行：label + amount */
  label: string
  amount: string
  /** 调用方已经排好序 */
  groups: IcicleGroup[]
  /** 分母；比各组之和大时，差额画在最右 */
  total: number
  /** 给差额起名（如"其他等待"）就画成斜线并在标题行标出；不给就是留白 */
  rest?: { name: string; tip?: string }
  fmt: (v: number) => string
  /** 组占比前的说明，如"占墙钟" */
  ofLabel: string
}>()
const t = useChartTheme()

const shown = computed(() => props.groups.filter((g) => g.value > 0))
const sum = computed(() => shown.value.reduce((a, g) => a + g.value, 0))
const denom = computed(() => Math.max(props.total, sum.value))
const filler = computed(() => denom.value - sum.value)
const breakdowns = computed(() => shown.value.filter((g) => g.segs.length))

// 图例按瀑布流排：组的长短差很多（一组十几个模型、一组三四个），按行对齐会在短组下面留一大块空白。
// 依次把每组放进当前最矮的一列；平局取最左，所以第一行仍是从大到小。列数跟原来的 md / xl 断点一致。
const narrow = useMediaQuery(NARROW)
const wide = useMediaQuery(WIDE)
const columns = computed(() => {
  const max = narrow.value ? 1 : wide.value && breakdowns.value.length > 2 ? 3 : 2
  const cols = Array.from({ length: Math.min(max, breakdowns.value.length) }, () => ({ h: 0, groups: [] as IcicleGroup[] }))
  for (const g of breakdowns.value) {
    const col = cols.reduce((a, b) => (b.h < a.h ? b : a))
    col.groups.push(g)
    // 组标题行比条目行高，再加上组间距，约等于两行
    col.h += g.segs.length + 2
  }
  return cols.map((c) => c.groups)
})
/** 差额那一段在悬停联动里的键 */
const REST = 'rest'

const pct = (v: number, of: number) => (of > 0 ? v / of : 0)
const pctText = (v: number, of: number) => fmtPct(pct(v, of), pct(v, of) < 0.1 ? 1 : 0)
const stripes = computed(() => `repeating-linear-gradient(135deg, ${t.value.other} 0 2px, transparent 2px 5px)`)

/** 'g' 高亮整组，'g\u0000k' 高亮一项 */
const SEP = '\u0000'
const active = ref<string | null>(null)
const dimGroup = (g: string) => active.value != null && active.value !== g && !active.value.startsWith(g + SEP)
/** 条本身是图：读屏读一句概括，细分在下面的图例里（真文字） */
const ariaLabel = computed(
  () => `${props.label} ${props.amount}：` + shown.value.map((g) => `${g.name} ${pctText(g.value, denom.value)}`).join('，'),
)
const dimSeg = (g: string, k: string) => active.value != null && active.value !== g && active.value !== g + SEP + k
</script>

<template>
  <div class="flex flex-col gap-5">
    <div>
      <div class="mb-2 flex flex-wrap items-baseline gap-x-2 text-sm">
        <span class="font-medium">{{ label }}</span>
        <span class="num ink-strong">{{ amount }}</span>
        <span
          v-if="rest && filler > 0"
          class="ml-auto flex items-center gap-1.5 text-xs ink-soft transition-opacity"
          :class="{ 'opacity-40': dimGroup(REST) }"
          :title="rest.tip"
          @mouseenter="active = REST"
          @mouseleave="active = null"
        >
          <span class="h-2.5 w-2.5 self-center rounded-sm" :style="{ background: stripes }"></span>
          {{ rest.name }} <span class="num ink-strong">{{ fmt(filler) }}</span>
          <span class="num">{{ pctText(filler, denom) }}</span>
        </span>
      </div>
      <div class="flex gap-0.5 overflow-hidden rounded-md" role="img" :aria-label="ariaLabel" @mouseleave="active = null">
        <div v-for="g in shown" :key="g.key" class="min-w-1 flex shrink basis-0 flex-col gap-0.5" :style="{ flexGrow: g.value }">
          <div
            class="h-1.5 transition-opacity"
            :class="{ 'opacity-30': dimGroup(g.key) }"
            :style="{ background: g.cap }"
            :title="`${g.name} · ${fmt(g.value)} · ${pctText(g.value, denom)}`"
            @mouseenter="active = g.key"
          ></div>
          <div class="h-5 flex gap-px">
            <div
              v-for="s in g.segs"
              :key="s.key"
              class="min-w-px shrink basis-0 transition-opacity"
              :class="{ 'opacity-30': dimSeg(g.key, s.key) }"
              :style="{ flexGrow: s.value, background: s.color }"
              :title="`${s.name} · ${fmt(s.value)} · 占${g.name} ${pctText(s.value, g.value)}`"
              @mouseenter="active = g.key + SEP + s.key"
            ></div>
          </div>
        </div>
        <!-- 差额：有名字画斜线，没有就留白 -->
        <div
          v-if="filler > 0"
          class="min-w-1 shrink basis-0 transition-opacity"
          :class="{ 'opacity-30': rest && dimGroup(REST) }"
          :style="{ flexGrow: filler, background: rest ? stripes : undefined }"
          :title="rest ? `${rest.name} · ${fmt(filler)} · ${pctText(filler, denom)}` : undefined"
          @mouseenter="active = rest ? REST : null"
        ></div>
      </div>
    </div>

    <div class="grid items-start gap-x-10" :style="{ gridTemplateColumns: `repeat(${columns.length}, minmax(0, 1fr))` }">
      <div v-for="(col, ci) in columns" :key="ci" class="min-w-0 flex flex-col gap-5">
        <div v-for="g in col" :key="g.key" class="min-w-0">
          <div
            class="mb-1.5 flex items-center gap-2 text-sm transition-opacity"
            :class="{ 'opacity-40': dimGroup(g.key) }"
            @mouseenter="active = g.key"
            @mouseleave="active = null"
          >
            <span class="h-2.5 w-2.5 shrink-0 rounded-sm" :style="{ background: g.cap }"></span>
            <span class="min-w-0 truncate font-medium">{{ g.name }}</span>
            <span class="shrink-0 num ink-strong">{{ fmt(g.value) }}</span>
            <span class="ml-auto shrink-0 text-xs num ink-soft">{{ ofLabel }} {{ pctText(g.value, denom) }}</span>
          </div>
          <ul class="m-0 list-none p-0 text-xs">
            <li
              v-for="s in g.segs"
              :key="s.key"
              class="min-w-0 flex items-center gap-2 leading-relaxed transition-opacity"
              :class="{ 'opacity-40': dimSeg(g.key, s.key) }"
              :title="s.tip"
              @mouseenter="active = g.key + SEP + s.key"
              @mouseleave="active = null"
            >
              <span class="h-2 w-2 shrink-0 rounded-full" :style="{ background: s.color }"></span>
              <span class="min-w-0 flex-1 truncate ink-strong">{{ s.name }}</span>
              <span v-if="s.hint" class="shrink-0 ink-soft num">{{ s.hint }}</span>
              <span class="w-20 shrink-0 text-right num ink-strong">{{ fmt(s.value) }}</span>
              <span class="w-10 shrink-0 text-right ink-soft num">{{ pctText(s.value, g.value) }}</span>
            </li>
          </ul>
        </div>
      </div>
    </div>
  </div>
</template>
