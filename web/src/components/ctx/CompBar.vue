<script setup lang="ts">
// 一条横向堆叠条：每段一种类别；可以在末尾画斜纹的保留区（输出预留、自动压缩余量），再画几条竖线标记（自动压缩触发点）。
// 分母大于各段之和时右边留空（上下文窗口没用满）；给了 freeLabel 这段空白也能悬停看剩多少。
// 保留区和标记是叠在条上的（绝对定位），用量超过保留区起点时会盖在类别段上，和 dsh-context 一样。
import { computed } from 'vue'
import { fmtInt, fmtPct } from '../../lib/format'

export interface BarSeg {
  key: string
  label: string
  value: number
  color: string
}

/** 保留区：按数组顺序从左往右排，最后一项贴着右端 */
export interface BarReserve {
  key: string
  label: string
  value: number
  /** 悬停说明的补充一行 */
  tip?: string
}

/** 竖线标记：画在 value / 分母 的位置 */
export interface BarMarker {
  key: string
  label: string
  value: number
  tip?: string
}

const props = defineProps<{
  segs: BarSeg[]
  /** 分母；不给就是各段之和 */
  total?: number
  /** 末尾的保留区（斜纹） */
  reserve?: BarReserve[]
  /** 竖线标记（如自动压缩触发线） */
  markers?: BarMarker[]
  /** 给了就让右边空白可悬停，标题用它 */
  freeLabel?: string
  height?: number
}>()
const emit = defineEmits<{ pick: [key: string] }>()

const used = computed(() => props.segs.reduce((a, s) => a + s.value, 0))
const denom = computed(() => props.total ?? used.value)
const ratio = (v: number) => (denom.value > 0 ? Math.max(0, Math.min(1, v / denom.value)) : 0)
const pct = (v: number) => `${ratio(v) * 100}%`

const free = computed(() => Math.max(0, denom.value - used.value))

/** 保留区从右端往左累加出各自的 left */
const reserveBoxes = computed(() => {
  const list = (props.reserve ?? []).filter((x) => x.value > 0)
  let right = denom.value
  const out: (BarReserve & { left: string; width: string })[] = []
  for (let i = list.length - 1; i >= 0; i--) {
    const r = list[i]!
    right -= r.value
    out.unshift({ ...r, left: pct(Math.max(0, right)), width: pct(r.value) })
  }
  return out
})

const markerLines = computed(() =>
  (props.markers ?? []).filter((m) => m.value > 0 && m.value < denom.value).map((m) => ({ ...m, left: pct(m.value) })),
)

const lines = (...xs: (string | undefined)[]) => xs.filter(Boolean).join('\n')
</script>

<template>
  <div class="relative flex w-full overflow-hidden rounded-md surface" :style="{ height: `${height ?? 14}px` }">
    <button
      v-for="s in segs.filter((x) => x.value > 0)"
      :key="s.key"
      type="button"
      class="h-full shrink-0 border-0 p-0 kbd-ring-flush"
      :style="{ width: pct(s.value), background: s.color }"
      :title="`${s.label}\n${fmtInt(s.value)} · ${fmtPct(denom ? s.value / denom : 0)}`"
      :aria-label="`${s.label} ${fmtInt(s.value)}`"
      @click="emit('pick', s.key)"
    />
    <div
      class="flex-1"
      :class="{ 'free-hover': freeLabel && free > 0 }"
      :title="freeLabel && free > 0 ? `${freeLabel}\n${fmtInt(free)} · ${fmtPct(ratio(free))}` : undefined"
    />
    <div
      v-for="r in reserveBoxes"
      :key="r.key"
      class="hatch absolute inset-y-0"
      :style="{ left: r.left, width: r.width }"
      :title="lines(r.label, `${fmtInt(r.value)} · ${fmtPct(ratio(r.value))}`, r.tip)"
    />
    <div
      v-for="m in markerLines"
      :key="m.key"
      class="marker absolute inset-y-0"
      :style="{ left: m.left }"
      :title="lines(m.label, `${fmtInt(m.value)} · ${fmtPct(ratio(m.value))}`, m.tip)"
    />
  </div>
</template>

<style scoped>
.hatch {
  background: repeating-linear-gradient(135deg, rgb(127 127 127 / 28%) 0 3px, transparent 3px 7px);
  border-left: 1px solid rgb(127 127 127 / 40%);
}
.free-hover {
  cursor: help;
}
.free-hover:hover {
  background: rgb(127 127 127 / 12%);
}
/* 9px 宽的命中区，中间 2px 的线 */
.marker {
  z-index: 1;
  width: 9px;
  transform: translateX(-50%);
  cursor: help;
}
.marker::after {
  content: '';
  position: absolute;
  inset-block: 0;
  left: 50%;
  width: 2px;
  margin-left: -1px;
  background: var(--warning-ink);
}
</style>
