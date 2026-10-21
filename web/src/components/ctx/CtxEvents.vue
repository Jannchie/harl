<script setup lang="ts">
// 上下文事件：注入、压缩、修剪、换模型、换模式。种类 chip 带整场计数，也是过滤开关。
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import type { CtxEvent, CtxEventKind, CtxFold } from '../../lib/context'
import { fmtBig, fmtInt } from '../../lib/format'
import DashCard from '../dash/DashCard.vue'
import TimeStamp from '../TimeStamp.vue'

const props = defineProps<{ fold: CtxFold }>()
const emit = defineEmits<{ pick: [e: CtxEvent] }>()
const { t } = useI18n()

const KINDS: CtxEventKind[] = ['inject', 'compaction', 'prune', 'model', 'mode']
const ICON: Record<CtxEventKind, string> = {
  inject: 'i-jannchie-plus-circle',
  compaction: 'i-jannchie-scissors',
  prune: 'i-jannchie-minus-circle',
  model: 'i-jannchie-arrows-horizontal',
  mode: 'i-jannchie-flag',
}

const picked = ref<CtxEventKind | null>(null)
const limit = ref(60)

const counts = computed(() => {
  const out = Object.fromEntries(KINDS.map((k) => [k, 0])) as Record<CtxEventKind, number>
  for (const e of props.fold.events) out[e.kind]++
  return out
})
/** 最新的在上 */
const list = computed(() => props.fold.events.filter((e) => !picked.value || e.kind === picked.value).reverse())
</script>

<template>
  <DashCard icon="i-jannchie-history" :title="t('ctx.events.title')" :caption="t('ctx.events.caption')">
    <div class="flex flex-col gap-2">
      <div class="flex flex-wrap gap-1">
        <button
          v-for="k in KINDS.filter((x) => counts[x])"
          :key="k"
          type="button"
          class="segment border-0"
          :class="picked === k ? 'pill-selected' : 'surface ink-muted'"
          :aria-pressed="picked === k"
          @click="picked = picked === k ? null : k"
        >
          <i class="h-3.5 w-3.5" :class="ICON[k]" aria-hidden="true" />{{ t(`ctx.events.kinds.${k}`) }} <span class="num">{{ fmtInt(counts[k]) }}</span>
        </button>
      </div>
      <p v-if="!list.length" class="m-0 py-6 text-center text-xs ink-soft">{{ t('ctx.events.none') }}</p>
      <ul class="m-0 flex max-h-[32rem] flex-col gap-px overflow-y-auto p-0">
        <li v-for="e in list.slice(0, limit)" :key="`${e.kind}:${e.seq}`" class="list-none">
          <button
            type="button"
            class="w-full flex items-center gap-2 rounded-lg border-0 bg-transparent px-2 py-1 text-left text-xs surface-hover kbd-ring"
            @click="emit('pick', e)"
          >
            <i
              class="h-3.5 w-3.5 shrink-0"
              :class="[ICON[e.kind], e.kind === 'compaction' || e.kind === 'prune' ? 'ink-danger' : 'ink-soft']"
              aria-hidden="true"
            />
            <span class="min-w-0 flex-1 truncate">
              <span class="ink-strong">{{ e.name }}</span>
              <span v-if="e.form && e.form !== e.name" class="ml-1.5 ink-soft">{{ e.form }}</span>
            </span>
            <span v-if="e.count" class="shrink-0 ink-soft num">{{ t('ctx.events.removed', { n: fmtInt(e.count) }) }}</span>
            <span v-if="e.preTokens" class="shrink-0 ink-soft num">{{ t('ctx.events.before', { n: fmtBig(e.preTokens) }) }}</span>
            <span v-if="e.tokens" class="w-14 shrink-0 text-right num" :class="e.kind === 'inject' ? '' : 'ink-success'">
              {{ e.kind === 'inject' ? `+${fmtBig(e.tokens)}` : `−${fmtBig(e.tokens)}` }}
            </span>
            <span class="w-10 shrink-0 text-right num ink-soft" :title="t('ctx.events.at', { n: e.step + 1 })">#{{ e.step + 1 }}</span>
            <TimeStamp v-if="e.ts" :ts="e.ts" class="w-16 shrink-0 text-right ink-soft" />
          </button>
        </li>
      </ul>
      <button
        v-if="list.length > limit"
        type="button"
        class="self-start rounded-md border-0 px-2 py-1 text-xs surface surface-hover kbd-ring"
        @click="limit += 100"
      >
        {{ t('ctx.events.more', { n: fmtInt(Math.min(100, list.length - limit)) }) }}
      </button>
    </div>
  </DashCard>
</template>
