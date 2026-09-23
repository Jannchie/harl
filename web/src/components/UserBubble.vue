<script setup lang="ts">
import { computed, ref } from 'vue'
import { fmtInt } from '../lib/format'

/** 用户发的话：页面上唯一属于读者自己的东西，靠右的气泡。太长先折起来 */
const props = defineProps<{ text: string }>()

const LIMIT = 1500
const open = ref(false)
const long = computed(() => props.text.length > LIMIT)
const shown = computed(() => (long.value && !open.value ? `${props.text.slice(0, LIMIT)}…` : props.text))
</script>

<template>
  <div class="flex flex-col items-end gap-1">
    <div class="max-w-[85%] whitespace-pre-wrap break-words rounded-2xl rounded-tr-md px-4 py-2.5 chat-text leading-chat surface">
      {{ shown }}
    </div>
    <div v-if="$slots.default || long" class="flex items-center gap-2 px-1 text-xs ink-soft">
      <slot />
      <button v-if="long" type="button" class="rounded-md px-1.5 py-0.5 surface-hover kbd-ring" @click="open = !open">
        {{ open ? '收起' : `展开全部 ${fmtInt(text.length)} 字` }}
      </button>
    </div>
  </div>
</template>
