<script setup lang="ts">
import { computed } from 'vue'
import type { RouteLocationRaw } from 'vue-router'

// 照 pfh 的 ActivityRow：agent 在两条消息之间做的一切（思考、工具调用、通知）都是这一种行，
// 读的人顺着左边一列图标往下扫，不用每种事件重新认一次版式。颜色只给状态。
const props = withDefaults(
  defineProps<{
    icon: string
    tone?: 'neutral' | 'failed' | 'muted'
    /** 这一行说的是什么：工具名、「思考」 */
    label?: string
    /** 关于它的细节，放不下就截断 */
    detail?: string
    /** 结果，行尾一瞥 */
    meta?: string
    /** 耗时 */
    time?: string
    interactive?: boolean
    /** 展开着 */
    selected?: boolean
    /** 行的主体在站内的另一个地方（子会话） */
    to?: RouteLocationRaw
  }>(),
  { tone: 'neutral' },
)

defineEmits<{ activate: [] }>()

const TONES = {
  neutral: { icon: undefined, meta: 'ink-muted' },
  failed: { icon: 'text-red-500', meta: 'ink-danger' },
  muted: { icon: undefined, meta: 'text-neutral-500' },
} as const

const tone = computed(() => TONES[props.tone])
/** 图标跟它后面那段文字同一个墨色；状态色优先 */
const iconInk = computed(() => tone.value.icon ?? (props.label === undefined ? 'ink-soft' : 'ink-strong'))
const ground = computed(() => (props.selected ? 'surface-strong' : props.interactive ? 'surface-hover' : ''))
</script>

<template>
  <component
    :is="interactive ? 'button' : 'div'"
    :type="interactive ? 'button' : undefined"
    class="w-full flex items-center gap-2 overflow-hidden rounded-lg px-2.5 py-1.5 text-left text-xs kbd-ring"
    :class="ground"
    :aria-expanded="interactive ? selected : undefined"
    @click="interactive ? $emit('activate') : undefined"
  >
    <i class="h-3.5 w-3.5 shrink-0" :class="[icon, iconInk]" />
    <span v-if="label" class="shrink-0 ink-strong">{{ label }}</span>
    <span class="min-w-0 flex-1 truncate ink-soft">{{ detail }}</span>
    <RouterLink v-if="to" :to="to" title="打开子会话" class="shrink-0 rounded p-0.5 ink-soft hover:ink-accent" @click.stop>
      <i class="i-carbon-arrow-right block h-3.5 w-3.5" />
    </RouterLink>
    <span v-if="time" class="shrink-0 num ink-muted">{{ time }}</span>
    <span v-if="meta" class="min-w-0 max-w-[45%] shrink-0 truncate pl-2 text-right num" :class="tone.meta">{{ meta }}</span>
  </component>
</template>
