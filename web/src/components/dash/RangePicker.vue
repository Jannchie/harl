<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { type DateRange, presetRange, RANGE_PRESETS, sameRange } from '../../lib/summaryModel'
import SegButton from './SegButton.vue'

const props = defineProps<{
  modelValue: DateRange
  /** 全部数据的本地日期边界，给 date input 的 min / max */
  min?: string | null
  max?: string | null
  disabled?: boolean
}>()
const emit = defineEmits<{ 'update:modelValue': [r: DateRange] }>()

const activePreset = computed(() => RANGE_PRESETS.find((p) => sameRange(presetRange(p.key), props.modelValue))?.key ?? null)

// 自定义输入：本地草稿，change 时才提交，避免输到一半就打请求
const from = ref(props.modelValue.from ?? '')
const to = ref(props.modelValue.to ?? '')
watch(
  () => props.modelValue,
  (r) => {
    from.value = r.from ?? ''
    to.value = r.to ?? ''
  },
)

function commit() {
  let f = from.value || null
  let t = to.value || null
  if (f && t && f > t) [f, t] = [t, f]
  emit('update:modelValue', { from: f, to: t })
}

const input =
  'w-32 rounded-lg border border-neutral-300 bg-white px-2 py-1 text-xs num kbd-ring-flush disabled:opacity-50 dark:border-neutral-700 dark:bg-neutral-950'
</script>

<template>
  <div class="flex flex-wrap items-center gap-2" :class="{ 'opacity-50 pointer-events-none': disabled }">
    <div class="segmented">
      <SegButton
        v-for="p in RANGE_PRESETS"
        :key="p.key"
        :active="activePreset === p.key"
        :disabled="disabled"
        @click="emit('update:modelValue', presetRange(p.key))"
      >
        {{ p.label }}
      </SegButton>
    </div>
    <label class="flex items-center gap-1.5">
      <input v-model="from" type="date" :class="input" :min="min ?? undefined" :max="max ?? undefined" :disabled="disabled" aria-label="开始日期" @change="commit" />
      <i class="i-carbon-arrow-right h-3.5 w-3.5 ink-soft" />
      <input v-model="to" type="date" :class="input" :min="min ?? undefined" :max="max ?? undefined" :disabled="disabled" aria-label="结束日期" @change="commit" />
    </label>
  </div>
</template>
