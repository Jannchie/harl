<script setup lang="ts">
// 一条横向堆叠条：每段一种类别；可以在末尾画斜纹的保留区（输出预留、自动压缩余量）。
// 分母大于各段之和时右边留空（上下文窗口没用满）。
import { computed } from 'vue'
import { fmtInt, fmtPct } from '../../lib/format'

export interface BarSeg {
  key: string
  label: string
  value: number
  color: string
}

const props = defineProps<{
  segs: BarSeg[]
  /** 分母；不给就是各段之和 */
  total?: number
  /** 末尾的保留区（斜纹），从右往左排 */
  reserve?: { key: string; label: string; value: number }[]
  height?: number
  /** 悬停 / 聚焦的类别：其余段变淡 */
  focus?: string | null
}>()
const emit = defineEmits<{ pick: [key: string] }>()

const denom = computed(() => props.total ?? props.segs.reduce((a, s) => a + s.value, 0))
const pct = (v: number) => (denom.value > 0 ? `${(v / denom.value) * 100}%` : '0%')
</script>

<template>
  <div class="relative flex w-full overflow-hidden rounded-md surface" :style="{ height: `${height ?? 14}px` }">
    <button
      v-for="s in segs.filter((x) => x.value > 0)"
      :key="s.key"
      type="button"
      class="h-full shrink-0 border-0 p-0 kbd-ring-flush transition-opacity duration-150"
      :style="{ width: pct(s.value), background: s.color, opacity: focus && focus !== s.key ? 0.3 : 1 }"
      :title="`${s.label}\n${fmtInt(s.value)} · ${fmtPct(denom ? s.value / denom : 0)}`"
      :aria-label="`${s.label} ${fmtInt(s.value)}`"
      @click="emit('pick', s.key)"
    />
    <div class="flex-1" />
    <div
      v-for="r in reserve?.filter((x) => x.value > 0) ?? []"
      :key="r.key"
      class="hatch h-full shrink-0"
      :style="{ width: pct(r.value) }"
      :title="`${r.label}\n${fmtInt(r.value)}`"
    />
  </div>
</template>

<style scoped>
.hatch {
  background: repeating-linear-gradient(135deg, rgb(127 127 127 / 28%) 0 3px, transparent 3px 7px);
  border-left: 1px solid rgb(127 127 127 / 40%);
}
</style>
