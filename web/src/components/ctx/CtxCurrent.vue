<script setup lang="ts">
// 当前上下文：下一次请求会带上的内容，画在整个上下文窗口上；右边空白是空闲窗口（可悬停）。
// 窗口：dsh 的 request/context 报了就用它，否则按模型 id 查 models.dev 目录（lib/modelWindow），都没有才显示"没记"。
// 只有 dsh 画保留区（斜纹：自动压缩余量 + 输出预留）和自动压缩触发线，规则抄 dsh-context：
//   阈值 = min(floor(80% 窗口), 窗口 − 输出预留 − 64k)；余量固定 64k，紧挨在输出预留左边；
//   阈值落在余量左边（80% 那一项更小）时才单独画触发线，否则触发点就是余量的左沿。
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useChartTheme } from '../../lib/chartTheme'
import { ALL_CATS, type Cat, type Parts } from '../../lib/context'
import { catColor, catLabel } from '../../lib/ctxView'
import { fmtBig, fmtInt, fmtPct } from '../../lib/format'
import { useModelLimit } from '../../lib/modelWindow'
import DashCard from '../dash/DashCard.vue'
import CompBar, { type BarMarker, type BarReserve, type BarSeg } from './CompBar.vue'

const props = withDefaults(
  defineProps<{
    parts: Parts
    tokens: number
    /** 没有 provider 报的 usage，全是估算 */
    estimated: boolean
    /** 日志里报的窗口（dsh 的 request/context）；有就优先于目录 */
    window: number | null
    /** 单次最大输出（dsh header 的 maxTokens） */
    maxOutput: number | null
    /** 当前模型 id（最后一次请求的模型），日志没报窗口时拿它查目录；不给就不查 */
    model?: string | null
    /** 会话的 provider。不给时按"报了 maxOutput 的就是 dsh"推断（只有 dsh 的折叠会填 maxOutput） */
    provider?: string | null
  }>(),
  { model: null, provider: undefined },
)
const emit = defineEmits<{ pick: [cat: Cat] }>()
const { t } = useI18n()
const theme = useChartTheme()

/** dsh 的自动压缩：余量固定 64k */
const HEADROOM = 65536

const segs = computed<BarSeg[]>(() =>
  ALL_CATS.map((c) => ({ key: c, label: catLabel(c), value: props.parts[c], color: catColor(c, theme.value) })),
)

const catalog = useModelLimit(() => (props.window ? null : props.model))
/** 日志报的窗口优先；其次目录 */
const win = computed(() => props.window ?? catalog.value?.context ?? null)
const fromCatalog = computed(() => !props.window && win.value != null)

const isDsh = computed(() => (props.provider === undefined ? props.maxOutput != null : props.provider === 'dsh'))

/** dsh 的压缩几何：阈值、输出预留、余量起点；窗口太小（阈值 ≤ 0）就没有 */
const compaction = computed(() => {
  const w = win.value
  if (!isDsh.value || !w) return null
  const output = Math.min(Math.max(props.maxOutput ?? 0, 0), w)
  const bufferStart = w - output - HEADROOM
  if (bufferStart <= 0) return null
  const threshold = Math.min(Math.floor(w * 0.8), bufferStart)
  return { threshold, output, bufferStart, separate: threshold < bufferStart }
})

const triggerPct = computed(() => (compaction.value && win.value ? compaction.value.threshold / win.value : null))

const reserve = computed<BarReserve[]>(() => {
  const c = compaction.value
  if (!c) return []
  return [
    {
      key: 'headroom',
      label: t('ctx.current.reserveHeadroom'),
      value: HEADROOM,
      tip: c.separate ? t('ctxCurrent.headroomBuffer') : t('ctxCurrent.headroomTip', { pct: fmtPct(triggerPct.value) }),
    },
    { key: 'output', label: t('ctx.current.reserveOutput'), value: c.output, tip: t('ctxCurrent.outputTip') },
  ]
})

const markers = computed<BarMarker[]>(() => {
  const c = compaction.value
  if (!c?.separate) return []
  return [{ key: 'trigger', label: t('ctxCurrent.trigger'), value: c.threshold, tip: t('ctxCurrent.triggerTip') }]
})

const pct = computed(() => (win.value ? Math.min(1, props.tokens / win.value) : null))
</script>

<template>
  <DashCard icon="i-jannchie-layers" :title="t('ctx.current.title')" :caption="t('ctx.current.caption')">
    <template #controls>
      <div class="text-right">
        <div class="text-xl font-semibold leading-tight num" :title="fmtInt(tokens)">
          <span v-if="estimated" class="mr-1 text-xs font-normal ink-soft">{{ t('ctx.current.estimated') }}</span>{{ fmtBig(tokens) }}
        </div>
        <div class="text-xs ink-soft num">
          <template v-if="win">
            {{ t('ctx.current.ofWindow', { pct: fmtPct(pct) }) }} ·
            <span v-if="fromCatalog" :title="t('ctxCurrent.windowCatalogHint', { model: model ?? '' })">{{ t('ctxCurrent.windowCatalog', { n: fmtBig(win) }) }}</span>
            <template v-else>{{ t('ctx.current.window', { n: fmtBig(win) }) }}</template>
            <template v-if="compaction">
              ·
              <span :class="{ 'ink-warning': tokens >= compaction.threshold }" :title="t('ctxCurrent.triggerAtHint', { n: fmtInt(compaction.threshold) })">{{
                t('ctxCurrent.triggerAt', { pct: fmtPct(triggerPct) })
              }}</span>
            </template>
          </template>
          <template v-else>{{ t('ctx.current.noWindow') }}</template>
        </div>
      </div>
    </template>
    <div class="flex flex-col gap-3">
      <CompBar
        :segs="segs"
        :total="win ?? undefined"
        :reserve="reserve"
        :markers="markers"
        :free-label="win ? t('ctxCurrent.free') : undefined"
        :height="18"
        @pick="(k) => emit('pick', k as Cat)"
      />
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
