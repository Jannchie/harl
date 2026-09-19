<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { type DateRange, presetRange, RANGE_PRESETS, sameRange } from '../../lib/summaryModel'

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
</script>

<template>
  <div class="picker" :class="{ disabled }">
    <div class="presets">
      <button
        v-for="p in RANGE_PRESETS"
        :key="p.key"
        type="button"
        :class="{ active: activePreset === p.key }"
        :disabled="disabled"
        @click="emit('update:modelValue', presetRange(p.key))"
      >
        {{ p.label }}
      </button>
    </div>
    <label class="custom mono">
      <input v-model="from" type="date" :min="min ?? undefined" :max="max ?? undefined" :disabled="disabled" @change="commit" />
      <span class="muted">→</span>
      <input v-model="to" type="date" :min="min ?? undefined" :max="max ?? undefined" :disabled="disabled" @change="commit" />
    </label>
  </div>
</template>

<style scoped>
.picker {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
}
.presets {
  display: flex;
}
.presets button {
  font-size: var(--fs-xs);
  padding: 1px 8px;
  border-radius: 0;
  margin-left: -1px;
}
.presets button:first-child {
  border-radius: var(--radius) 0 0 var(--radius);
  margin-left: 0;
}
.presets button:last-child {
  border-radius: 0 var(--radius) var(--radius) 0;
}
.presets button.active {
  position: relative;
  z-index: 1;
}
.custom {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: var(--fs-xs);
}
.custom input {
  background: var(--bg);
  border: 1px solid var(--line);
  border-radius: var(--radius);
  padding: 1px 4px;
  font-size: var(--fs-xs);
  color: var(--fg-1);
  min-width: 0;
  width: 118px;
}
.custom input:focus {
  outline: 1px solid var(--accent);
  border-color: var(--accent);
}
.custom input::-webkit-calendar-picker-indicator {
  opacity: 0.5;
  cursor: pointer;
}
</style>
