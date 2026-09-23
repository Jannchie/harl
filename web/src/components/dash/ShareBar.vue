<script setup lang="ts">
// 一行：标题 + 圆角横向堆叠条 + 图例列表，纯 HTML。名字只在图例里，条本身只表示份额。
// total 大于各段之和时，右侧留空（用来表现"这一行只是上一行的某一段"）。
import { computed } from 'vue'
import { fmtPct } from '../../lib/format'

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

const props = withDefaults(
  defineProps<{
    /** 行标题；amount 是标题旁的合计 */
    label: string
    amount: string
    segments: ShareSeg[]
    /** 条的分母；缺省用各段之和 */
    total?: number
    fmt: (v: number) => string
    /** 图例最多几项，其余折成 "+N" */
    legendMax?: number
  }>(),
  { legendMax: 12 },
)

const sum = computed(() => props.segments.reduce((a, s) => a + Math.max(0, s.value), 0))
const denom = computed(() => Math.max(props.total ?? 0, sum.value))
const filler = computed(() => Math.max(0, denom.value - sum.value))
const pct = (v: number) => (denom.value > 0 ? v / denom.value : 0)
const pctText = (v: number) => fmtPct(pct(v), pct(v) < 0.1 ? 1 : 0)

const legend = computed(() => props.segments.slice(0, props.legendMax))
const legendRest = computed(() => props.segments.length - legend.value.length)
</script>

<template>
  <div class="min-w-0">
    <div class="mb-1.5 flex items-baseline gap-2 text-sm">
      <span class="font-medium">{{ label }}</span>
      <span class="text-xs ink-soft num">{{ amount }}</span>
    </div>
    <div class="meter-track h-2 flex gap-0.5" role="img">
      <div
        v-for="s in segments"
        :key="s.key"
        class="min-w-0.5 shrink basis-0 rounded-full transition-opacity hover:opacity-80"
        :style="{ flexGrow: Math.max(0, s.value), background: s.color }"
        :title="s.tip ?? `${s.name} · ${fmt(s.value)} · ${pctText(s.value)}`"
      ></div>
      <div v-if="filler > 0" class="shrink basis-0" :style="{ flexGrow: filler }"></div>
    </div>
    <ul class="m-0 mt-2 grid list-none grid-cols-1 gap-y-0.5 p-0 text-xs">
      <li v-for="s in legend" :key="s.key" class="min-w-0 flex items-center gap-2 leading-relaxed" :title="s.tip">
        <span class="h-2 w-2 shrink-0 rounded-full" :style="{ background: s.color }"></span>
        <span class="min-w-0 flex-1 truncate ink-strong">{{ s.name }}</span>
        <span v-if="s.hint" class="shrink-0 ink-soft num">{{ s.hint }}</span>
        <span class="w-20 shrink-0 text-right num ink-strong">{{ fmt(s.value) }}</span>
        <span class="w-10 shrink-0 text-right ink-soft num">{{ pctText(s.value) }}</span>
      </li>
      <li v-if="legendRest > 0" class="ink-soft">+{{ legendRest }} 项</li>
    </ul>
  </div>
</template>
