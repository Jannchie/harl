<script setup lang="ts">
import { computed, ref } from 'vue'
import type { Event } from '../api'
import { summarize } from '../lib/group'
import JsonBlock from './JsonBlock.vue'
import TimeStamp from './TimeStamp.vue'

/** 平铺视图的一行：原始顺序里的一个事件，点开看完整 JSON */
const props = defineProps<{ ev: Event }>()
const open = ref(false)
const summary = computed(() => summarize(props.ev, 200))
const scope = computed(() => {
  const t = props.ev.turn ? `t${props.ev.turn.n ?? '?'}` : ''
  const s = props.ev.step ? `s${props.ev.step.n ?? '?'}` : ''
  return [t, s].filter(Boolean).join('/')
})
const tags = computed(() => {
  const out: string[] = []
  for (const l of props.ev.links ?? []) out.push(`${l.rel}${l.inferred ? '?' : ''}`)
  if (props.ev.inferred?.length) out.push(`推断 ${props.ev.inferred.join(',')}`)
  return out
})
/** 结构性的和隐藏的事件退后一档，对话内容的正常 */
const quiet = computed(() => ['request_header', 'hook', 'meta', 'step_started', 'step_completed'].includes(props.ev.kind))
</script>

<template>
  <div>
    <button
      type="button"
      class="w-full flex items-center gap-3 rounded-lg px-2.5 py-1 text-left text-xs kbd-ring"
      :class="open ? 'surface-strong row-pin' : 'surface-hover'"
      :aria-expanded="open"
      @click="open = !open"
    >
      <span class="w-10 shrink-0 text-right num ink-soft">{{ ev.seq }}</span>
      <TimeStamp :ts="ev.ts" class="w-16 shrink-0 ink-soft" />
      <span class="w-28 shrink-0 truncate" :class="quiet ? 'ink-soft' : 'ink-strong'">{{ ev.kind }}</span>
      <span class="w-16 shrink-0 truncate ink-soft max-sm:hidden">{{ ev.role ?? '' }}</span>
      <span class="w-12 shrink-0 num ink-soft max-sm:hidden" :title="[ev.turn?.id, ev.step?.id].filter(Boolean).join('\n')">{{ scope }}</span>
      <span class="min-w-0 flex-1 truncate" :class="quiet ? 'ink-soft' : 'ink-muted'">
        <span v-for="t in tags" :key="t" class="mr-1.5 rounded px-1 py-px surface">{{ t }}</span>{{ summary }}
      </span>
    </button>
    <div v-if="open" class="px-2.5 pb-3 pt-1"><JsonBlock :value="ev" /></div>
  </div>
</template>
