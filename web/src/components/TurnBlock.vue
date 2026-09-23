<script setup lang="ts">
import { computed, ref } from 'vue'
import type { Item, Turn } from '../lib/group'
import { fmtDuration } from '../lib/format'
import ItemView from './ItemView.vue'
import TimeStamp from './TimeStamp.vue'
import UsageChip from './UsageChip.vue'
import UserBubble from './UserBubble.vue'

const props = defineProps<{ turn: Turn }>()

const duration = computed(() => fmtDuration(props.turn.started?.ts ?? props.turn.firstTs, props.turn.completed?.ts ?? props.turn.lastTs))
const collapsed = ref(false)

/** step 只是分组用的，对话里不单独画：按顺序摊平成一列 */
const items = computed<Item[]>(() => props.turn.children.flatMap((c) => (c.type === 'step' ? c.step.items : [c.item])))
</script>

<template>
  <section :id="`turn-${turn.key}`" class="scroll-mt-[var(--pin-top,0px)]">
    <!-- 轮次的分隔线：这一轮的时间、耗时、用量，点一下折起整轮 -->
    <button
      type="button"
      class="group w-full flex items-center gap-2 rounded-md py-3 text-left text-xs ink-soft kbd-ring"
      :aria-expanded="!collapsed"
      @click="collapsed = !collapsed"
    >
      <i class="h-3 w-3 shrink-0 opacity-60 group-hover:opacity-100" :class="collapsed ? 'i-carbon-chevron-right' : 'i-carbon-chevron-down'" />
      <span class="num ink-muted">#{{ turn.n ?? '?' }}</span>
      <TimeStamp :ts="turn.started?.ts ?? turn.firstTs" mode="full" />
      <span v-if="duration" class="num">{{ duration }}</span>
      <span class="num">{{ turn.stepCount }} 步 · {{ turn.toolCount }} 次工具</span>
      <span v-if="turn.inferred" title="turn 边界是 normalizer 推断的">推断</span>
      <span v-if="!turn.completed" class="ink-warning" title="日志里没有这一轮的结束事件">未结束</span>
      <span class="h-px min-w-4 flex-1 bg-neutral-500/15" />
      <UsageChip v-if="turn.usage" :usage="turn.usage" />
      <UsageChip v-else-if="turn.usageSum" :usage="turn.usageSum" summed />
    </button>

    <div v-if="!collapsed" class="flex flex-col gap-1 pb-2">
      <UserBubble v-if="turn.userText != null" :text="turn.userText" class="mb-2" />
      <ItemView v-for="it in items" :key="it.ev.seq" :item="it" />
    </div>
  </section>
</template>
