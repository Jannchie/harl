<script setup lang="ts" generic="K extends string">
// 可排序的表头格：整格是按钮，当前排序列显示方向箭头
import type { Sort } from '../../lib/sort'

const props = defineProps<{
  sort: Sort<K>
  k: K
  align?: 'left' | 'right'
}>()

const active = () => props.sort.key.value === props.k
</script>

<template>
  <th class="group/th pb-1.5 font-normal" :class="align === 'left' ? 'text-left' : 'text-right'" :aria-sort="active() ? (sort.dir.value === 'asc' ? 'ascending' : 'descending') : 'none'">
    <button
      type="button"
      class="inline-flex items-center gap-0.5 rounded kbd-ring hover:text-neutral-900 dark:hover:text-neutral-100"
      :class="[align === 'left' ? '' : 'flex-row-reverse', active() ? 'ink-strong' : '']"
      @click="sort.toggle(k)"
    >
      <slot />
      <i
        class="h-3 w-3 shrink-0"
        :class="active() ? (sort.dir.value === 'asc' ? 'i-carbon-arrow-up' : 'i-carbon-arrow-down') : 'i-carbon-arrows-vertical opacity-0 group-hover/th:opacity-40'"
      />
    </button>
  </th>
</template>
