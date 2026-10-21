<script setup lang="ts">
// 当前上下文：下一次请求会带上的内容，画在整个上下文窗口上；末尾斜纹是输出预留和自动压缩余量。
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useChartTheme } from '../../lib/chartTheme'
import { ALL_CATS, type Cat, type Parts } from '../../lib/context'
import { catColor, catLabel } from '../../lib/ctxView'
import { fmtBig, fmtInt, fmtPct } from '../../lib/format'
import DashCard from '../dash/DashCard.vue'
import CompBar, { type BarSeg } from './CompBar.vue'

const props = defineProps<{
  parts: Parts
  tokens: number
  /** 没有 provider 报的 usage，全是估算 */
  estimated: boolean
  window: number | null
  maxOutput: number | null
}>()
const emit = defineEmits<{ pick: [cat: Cat] }>()
const { t } = useI18n()
const theme = useChartTheme()

const segs = computed<BarSeg[]>(() =>
  ALL_CATS.map((c) => ({ key: c, label: catLabel(c), value: props.parts[c], color: catColor(c, theme.value) })),
)

/** 保留区：输出预留 = min(maxTokens, window)；自动压缩阈值 = min(80% 窗口, 窗口 − 输出 − 64k) */
const reserve = computed(() => {
  const w = props.window
  if (!w) return []
  const output = Math.min(props.maxOutput ?? 0, w)
  const threshold = Math.min(Math.floor(w * 0.8), w - output - 65536)
  if (threshold <= 0 || !output) return []
  return [
    { key: 'headroom', label: t('ctx.current.reserveHeadroom'), value: w - output - threshold },
    { key: 'output', label: t('ctx.current.reserveOutput'), value: output },
  ]
})

const pct = computed(() => (props.window ? Math.min(1, props.tokens / props.window) : null))
</script>

<template>
  <DashCard icon="i-jannchie-layers" :title="t('ctx.current.title')" :caption="t('ctx.current.caption')">
    <template #controls>
      <div class="text-right">
        <div class="text-xl font-semibold leading-tight num" :title="fmtInt(tokens)">
          <span v-if="estimated" class="mr-1 text-xs font-normal ink-soft">{{ t('ctx.current.estimated') }}</span>{{ fmtBig(tokens) }}
        </div>
        <div class="text-xs ink-soft num">
          <template v-if="window">{{ t('ctx.current.ofWindow', { pct: fmtPct(pct) }) }} · {{ t('ctx.current.window', { n: fmtBig(window) }) }}</template>
          <template v-else>{{ t('ctx.current.noWindow') }}</template>
        </div>
      </div>
    </template>
    <div class="flex flex-col gap-3">
      <CompBar :segs="segs" :total="window ?? undefined" :reserve="reserve" :height="18" @pick="(k) => emit('pick', k as Cat)" />
      <ul class="m-0 grid list-none grid-cols-1 gap-x-6 gap-y-1 p-0 text-xs sm:grid-cols-2 lg:grid-cols-4">
        <li v-for="seg in segs.filter((x) => x.value > 0)" :key="seg.key">
          <button
            type="button"
            class="w-full flex items-center gap-1.5 rounded-md border-0 bg-transparent px-1 py-0.5 text-left text-xs surface-hover kbd-ring"
            :title="seg.key === 'other' ? t('ctx.catHint.other') : undefined"
            @click="emit('pick', seg.key as Cat)"
          >
            <span class="h-2 w-2 shrink-0 rounded-full" :style="{ background: seg.color }" />
            <span class="min-w-0 flex-1 truncate ink-muted">{{ seg.label }}</span>
            <span class="num">{{ fmtBig(seg.value) }}</span>
            <span class="w-12 text-right num ink-soft">{{ fmtPct(tokens ? seg.value / tokens : 0) }}</span>
          </button>
        </li>
      </ul>
    </div>
  </DashCard>
</template>
