<script setup lang="ts">
// SVG 环图：echarts 没注册饼图，这里几段就够了，自己画。每段一条带 stroke-dasharray 的圆弧，从 12 点钟顺时针排。
// 中间的内容走默认插槽；悬停一段时 hover 事件把它的 key 交给父组件（图例跟着高亮）。
import { computed } from 'vue'

export interface DonutSeg {
  key: string
  label: string
  value: number
  color: string
  /** 悬停提示（原生 title） */
  tip?: string
}

const props = withDefaults(
  defineProps<{
    segs: DonutSeg[]
    /** 外径（px） */
    size?: number
    /** 环宽（px） */
    thickness?: number
    /** 高亮的一段，其余变淡 */
    active?: string | null
    /** 给读屏的一句概括 */
    label?: string
  }>(),
  { size: 160, thickness: 18, active: null, label: undefined },
)
const emit = defineEmits<{ hover: [key: string | null] }>()

const r = computed(() => (props.size - props.thickness) / 2)
const circ = computed(() => 2 * Math.PI * r.value)

/** 各段的弧长和起点；段之间留一点缝（段很小时不留，免得整段被缝吃掉） */
const arcs = computed(() => {
  const list = props.segs.filter((s) => s.value > 0)
  const total = list.reduce((a, s) => a + s.value, 0)
  if (total <= 0) return []
  const gap = list.length > 1 ? Math.min(2, circ.value * 0.004) : 0
  let at = 0
  return list.map((s) => {
    const len = (s.value / total) * circ.value
    const visible = Math.max(0, len - (len > gap * 2 ? gap : 0))
    const arc = { ...s, dash: `${visible} ${circ.value - visible}`, offset: -at }
    at += len
    return arc
  })
})
</script>

<template>
  <div class="relative shrink-0" :style="{ width: `${size}px`, height: `${size}px` }">
    <svg :width="size" :height="size" :viewBox="`0 0 ${size} ${size}`" role="img" :aria-label="label" class="block -rotate-90">
      <circle :cx="size / 2" :cy="size / 2" :r="r" fill="none" class="stroke-black/8 dark:stroke-white/8" :stroke-width="thickness" />
      <circle
        v-for="a in arcs"
        :key="a.key"
        :cx="size / 2"
        :cy="size / 2"
        :r="r"
        fill="none"
        :stroke="a.color"
        :stroke-width="thickness"
        :stroke-dasharray="a.dash"
        :stroke-dashoffset="a.offset"
        class="transition-opacity duration-150"
        :style="{ opacity: active && active !== a.key ? 0.3 : 1 }"
        @mouseenter="emit('hover', a.key)"
        @mouseleave="emit('hover', null)"
      >
        <title v-if="a.tip">{{ a.tip }}</title>
      </circle>
    </svg>
    <div v-if="$slots.default" class="pointer-events-none absolute inset-0 flex flex-col items-center justify-center text-center">
      <slot />
    </div>
  </div>
</template>
