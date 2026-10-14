<script setup lang="ts" generic="T extends string">
// 分段选择：几个互斥的选项，一次只选一个（指标切换、视图切换、时间预设……）。
// 语义按 WAI-ARIA APG 的 radio group：整组只占一个 Tab 停靠点（选中的那个，没选中就第一个），
// ←↑ / →↓ 在组内移动并直接选中，Home / End 到头尾。
import { computed, ref } from 'vue'

export interface SegOption<V extends string = string> {
  value: V
  label: string
  /** 图标类 */
  icon?: string
  /** 标签前的小圆点（颜色类） */
  dot?: string
  /** 窄屏只留图标（label 仍给读屏） */
  compact?: boolean
}

const props = defineProps<{
  options: readonly SegOption<T>[]
  /** 这组选的是什么，给读屏：「指标」「视图」 */
  label: string
  disabled?: boolean
}>()

/** null：一个都没选（时间范围是自定义的，不对应任何预设） */
const model = defineModel<T | null>({ required: true })

const buttons = ref<HTMLButtonElement[]>([])

/** 整组唯一的 Tab 停靠点：选中的那个，没选中就第一个 */
const tabStop = computed(() => Math.max(0, props.options.findIndex((o) => o.value === model.value)))

function pick(i: number) {
  const o = props.options[i]
  if (!o) return
  model.value = o.value
  buttons.value[i]?.focus()
}

function onKey(e: KeyboardEvent, i: number) {
  const n = props.options.length
  let next: number
  switch (e.key) {
    case 'ArrowLeft':
    case 'ArrowUp':
      next = (i - 1 + n) % n
      break
    case 'ArrowRight':
    case 'ArrowDown':
      next = (i + 1) % n
      break
    case 'Home':
      next = 0
      break
    case 'End':
      next = n - 1
      break
    default:
      return
  }
  e.preventDefault()
  pick(next)
}
</script>

<template>
  <div role="radiogroup" :aria-label="label" class="segmented">
    <button
      v-for="(o, i) in options"
      :key="o.value"
      ref="buttons"
      type="button"
      role="radio"
      class="segment border-0"
      :class="model === o.value ? 'segment-selected' : 'bg-transparent ink-muted hover:text-ink'"
      :aria-checked="model === o.value"
      :aria-label="o.compact ? o.label : undefined"
      :title="o.compact ? o.label : undefined"
      :tabindex="i === tabStop ? 0 : -1"
      :disabled="disabled"
      @click="pick(i)"
      @keydown="onKey($event, i)"
    >
      <i v-if="o.icon" class="h-3.5 w-3.5" :class="o.icon" aria-hidden="true" />
      <span v-if="o.dot" class="h-1.5 w-1.5 rounded-full" :class="o.dot" aria-hidden="true" />
      <span :class="o.compact ? 'max-sm:hidden' : undefined">{{ o.label }}</span>
    </button>
  </div>
</template>
