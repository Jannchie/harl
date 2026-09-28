<script setup lang="ts">
import { computed, nextTick, ref } from 'vue'
import type { Item, Turn } from '../lib/group'
import { clusterItems } from '../lib/group'
import { fmtDuration } from '../lib/format'
import ItemView from './ItemView.vue'
import StepsCluster from './StepsCluster.vue'
import TimeStamp from './TimeStamp.vue'
import UsageChip from './UsageChip.vue'
import UserBubble from './UserBubble.vue'

const props = defineProps<{ turn: Turn }>()

const duration = computed(() => fmtDuration(props.turn.started?.ts ?? props.turn.firstTs, props.turn.completed?.ts ?? props.turn.lastTs))
const collapsed = ref(false)

/** step 只是分组用的，对话里不单独画：按顺序摊平成一列，连续的工具调用 / 思考再收成折叠组 */
const items = computed<Item[]>(() => props.turn.children.flatMap((c) => (c.type === 'step' ? c.step.items : [c.item])))
const clusters = computed(() => clusterItems(items.value))

/** 展开着的折叠组；默认全折 */
const openSteps = ref(new Set<string>())

/** 收起时组头如果正钉在顶上（内容已经滚过去了），收完滚回组头，不然视口停在后面不相干的地方 */
async function toggleSteps(key: string) {
  const next = new Set(openSteps.value)
  const closing = next.has(key)
  const el = document.getElementById(`${props.turn.key}-${key}`)
  const pinned = closing && el != null && el.getBoundingClientRect().top <= 60
  if (closing) next.delete(key)
  else next.add(key)
  openSteps.value = next
  if (pinned) {
    await nextTick()
    el?.scrollIntoView({ block: 'start' })
  }
}
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
      <span v-if="turn.efforts.length" class="num" title="推理强度">{{ turn.efforts.join(' → ') }}</span>
      <span v-for="sk in turn.skills" :key="sk" class="inline-flex items-center gap-1" :title="`这一轮有调用归属到 skill ${sk}`">
        <i class="i-carbon-magic-wand h-3 w-3" aria-hidden="true" />{{ sk }}
      </span>
      <span v-if="turn.inferred" title="turn 边界是 normalizer 推断的">推断</span>
      <span v-if="!turn.completed" class="ink-warning" title="日志里没有这一轮的结束事件">未结束</span>
      <span class="h-px min-w-4 flex-1 bg-neutral-500/15" />
      <UsageChip v-if="turn.usage" :usage="turn.usage" />
      <UsageChip v-else-if="turn.usageSum" :usage="turn.usageSum" summed />
    </button>

    <div v-if="!collapsed" class="flex flex-col gap-1 pb-2">
      <UserBubble v-if="turn.userText != null" :text="turn.userText" class="mb-2" />
      <template v-for="c in clusters" :key="c.type === 'steps' ? c.key : c.item.ev.seq">
        <ItemView v-if="c.type === 'item'" :item="c.item" />
        <div v-else class="flex flex-col gap-1">
          <!-- 展开时组头钉在顶上；组里展开的行钉在它下面一行（28px 是一行的高度） -->
          <div
            :id="`${turn.key}-${c.key}`"
            class="scroll-mt-[var(--pin-top,0px)]"
            :class="openSteps.has(c.key) ? 'row-pin' : undefined"
          >
            <StepsCluster :items="c.items" :expanded="openSteps.has(c.key)" @toggle="toggleSteps(c.key)" />
          </div>
          <div v-if="openSteps.has(c.key)" class="steps-body flex flex-col gap-1 border-l border-neutral-500/15 pl-2 ml-2">
            <ItemView v-for="it in c.items" :key="it.ev.seq" :item="it" />
          </div>
        </div>
      </template>
    </div>
  </section>
</template>

<style scoped>
.steps-body {
  --pin-top: calc(var(--pin-base, 0px) + 28px);
}
</style>
