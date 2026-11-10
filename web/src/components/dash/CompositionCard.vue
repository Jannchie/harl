<script setup lang="ts">
// 范围内计费输入（每次请求的 prompt）按上下文类别拆开：左边环图，右边图例带数量和占比。
// 类别和配色与会话页的上下文视图同一套（mix.ts → lib/ctxView.ts）
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import type { Mix } from '../../api'
import { useChartTheme } from '../../lib/chartTheme'
import { fmtBig, fmtInt, fmtPct } from '../../lib/format'
import DashCard from './DashCard.vue'
import Donut from './Donut.vue'
import { mixAria, mixSegs, mixTotal } from './mix'

const props = defineProps<{ mix: Mix }>()
const theme = useChartTheme()
const { t } = useI18n()

const segs = computed(() => mixSegs(props.mix, theme.value))
const total = computed(() => mixTotal(props.mix))
/** 图例按量从大到小；0 的放最后、变淡 */
const legend = computed(() => [...segs.value].sort((a, b) => b.value - a.value))
const active = ref<string | null>(null)
const aria = computed(() => t('dashMore.composition.aria', { items: mixAria(segs.value) }))
</script>

<template>
  <DashCard icon="i-jannchie-layers" :title="t('dashMore.composition.title')" :title-hint="t('dashMore.composition.titleHint')">
    <div v-if="total <= 0" class="py-8 text-center text-sm ink-soft">{{ t('dashMore.composition.empty') }}</div>
    <div v-else class="flex flex-wrap items-center gap-x-8 gap-y-4 max-sm:justify-center">
      <Donut :segs="segs" :size="168" :thickness="20" :active="active" :label="aria" @hover="active = $event">
        <span class="text-xs ink-soft">{{ t('dashMore.composition.total') }}</span>
        <span class="text-lg font-semibold leading-tight num ink-strong" :title="`${fmtInt(total)} tokens`">{{ fmtBig(total) }}</span>
        <span class="text-xs ink-soft">tokens</span>
      </Donut>
      <ul class="m-0 grid min-w-0 flex-1 list-none grid-cols-1 gap-x-6 gap-y-0.5 p-0 sm:grid-cols-2">
        <li
          v-for="s in legend"
          :key="s.key"
          class="flex items-center gap-2 rounded-lg px-1.5 py-1 text-sm transition-opacity duration-150"
          :class="[s.value > 0 ? '' : 'opacity-45', active && active !== s.key ? 'opacity-45' : '']"
          :title="s.tip"
          @mouseenter="active = s.value > 0 ? s.key : null"
          @mouseleave="active = null"
        >
          <span class="h-2.5 w-2.5 shrink-0 rounded-sm" :style="{ background: s.color }"></span>
          <span class="min-w-0 flex-1 truncate">{{ s.label }}</span>
          <span class="text-xs num" :title="`${fmtInt(s.value)} tokens`">{{ fmtBig(s.value) }}</span>
          <span class="w-12 text-right text-xs num ink-soft">{{ fmtPct(s.share) }}</span>
        </li>
      </ul>
    </div>
  </DashCard>
</template>
