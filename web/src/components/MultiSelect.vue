<script setup lang="ts">
import { computed, nextTick, onUnmounted, ref, watch } from 'vue'

// 照 pfh 的 MultiSelect：点选不关菜单（选八个就是八下，不是二十四下），选项多时带过滤框。
// 触发器是一个小胶囊：没选时安静，选了就用选中色，一眼看出哪些维度在过滤。
export interface Option {
  id: string
  label: string
  /** 选项前的小圆点（颜色类） */
  dot?: string
}

const props = defineProps<{
  options: readonly Option[]
  /** 维度名：「Agent」「模型」 */
  label: string
  disabled?: boolean
}>()

const selected = defineModel<string[]>({ required: true })

const open = ref(false)
const root = ref<HTMLElement | null>(null)
const search = ref<HTMLInputElement | null>(null)
const query = ref('')

const chosen = computed(() => new Set(selected.value))
const summary = computed(() => {
  const n = selected.value.length
  if (n === 0) return '全部'
  if (n === 1) return props.options.find((o) => o.id === selected.value[0])?.label ?? selected.value[0]
  return `${n} 项`
})
const visible = computed(() => {
  const q = query.value.trim().toLowerCase()
  return q ? props.options.filter((o) => o.label.toLowerCase().includes(q)) : props.options
})

function toggle(id: string) {
  const next = new Set(chosen.value)
  if (!next.delete(id)) next.add(id)
  // 按选项自己的顺序存，而不是点的顺序
  selected.value = props.options.filter((o) => next.has(o.id)).map((o) => o.id)
}

function onOutside(e: PointerEvent) {
  if (root.value && !root.value.contains(e.target as Node)) open.value = false
}

watch(open, async (v) => {
  if (v) {
    document.addEventListener('pointerdown', onOutside)
    await nextTick()
    search.value?.focus()
  } else {
    document.removeEventListener('pointerdown', onOutside)
    query.value = ''
  }
})
onUnmounted(() => document.removeEventListener('pointerdown', onOutside))
</script>

<template>
  <div ref="root" class="relative" @keydown.esc.stop="open = false">
    <button
      type="button"
      class="flex items-center gap-1.5 rounded-full px-3 py-1 text-xs kbd-ring transition-quick disabled:opacity-50"
      :class="selected.length ? 'pill-selected' : 'surface surface-hover ink-muted'"
      :disabled="disabled"
      :aria-expanded="open"
      @click="open = !open"
    >
      <span :class="selected.length ? 'opacity-75' : 'ink-soft'">{{ label }}</span>
      <span class="max-w-40 truncate">{{ summary }}</span>
      <i class="i-carbon-chevron-down h-3 w-3 opacity-70" />
    </button>

    <div v-if="open" class="popover absolute left-0 top-full mt-1 w-64 flex flex-col">
      <label v-if="options.length > 8" class="relative mb-1 block">
        <i class="i-carbon-search pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 ink-soft" />
        <input ref="search" v-model="query" type="search" :placeholder="`搜索${label}`" class="field py-1.5 pl-8 text-xs" />
      </label>
      <div class="max-h-72 overflow-y-auto">
        <button
          v-for="o in visible"
          :key="o.id"
          type="button"
          class="menu-item text-xs"
          :aria-pressed="chosen.has(o.id)"
          @click="toggle(o.id)"
        >
          <i class="h-4 w-4 shrink-0" :class="chosen.has(o.id) ? 'i-carbon-checkbox-checked text-blue-500' : 'i-carbon-checkbox ink-soft'" />
          <span v-if="o.dot" class="h-1.5 w-1.5 shrink-0 rounded-full" :class="o.dot" />
          <span class="min-w-0 flex-1 truncate">{{ o.label }}</span>
        </button>
        <p v-if="visible.length === 0" class="m-0 px-2 py-3 text-center text-xs ink-soft">没有匹配的{{ label }}</p>
      </div>
      <button
        v-if="selected.length"
        type="button"
        class="menu-item mt-1 justify-center text-xs ink-soft"
        @click="selected = []"
      >
        清除选择
      </button>
    </div>
  </div>
</template>
