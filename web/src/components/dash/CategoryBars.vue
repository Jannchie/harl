<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { ToolAgg, ToolCategory } from '../../api'
import { useChartTheme } from '../../lib/chartTheme'
import { fmtBig, fmtInt, fmtMs, fmtPct } from '../../lib/format'
import { categoryColor, categoryLabel } from '../../lib/summaryModel'
import { useSort } from '../../lib/sort'
import SortTh from './SortTh.vue'

const props = defineProps<{ byCategory: Partial<Record<ToolCategory, ToolAgg>> }>()
const theme = useChartTheme()
const { t } = useI18n()

interface Row {
  key: string
  label: string
  color: string
  calls: number
  failures: number
  failRate: number
  avgMs: number
  ms: number
}

/** 颜色跟着类别走；默认按调用数排，点列头换 */
const rows = computed<Row[]>(() =>
  Object.keys(props.byCategory).map((k) => {
    const x = props.byCategory[k as ToolCategory]!
    return {
      key: k,
      label: categoryLabel(k),
      color: categoryColor(k, theme.value),
      calls: x.calls,
      failures: x.failures,
      failRate: x.calls > 0 ? x.failures / x.calls : 0,
      avgMs: x.calls > 0 ? x.ms / x.calls : 0,
      ms: x.ms,
    }
  }),
)
const { sort, sorted } = useSort(
  rows,
  { label: (r) => r.label, calls: (r) => r.calls, failRate: (r) => r.failRate, avgMs: (r) => r.avgMs, ms: (r) => r.ms },
  'calls',
)
const maxCalls = computed(() => Math.max(1, ...rows.value.map((r) => r.calls)))

function tip(r: Row): string {
  const avg = fmtMs(r.calls > 0 ? r.avgMs : null)
  const total = r.key === 'user' ? t('cards.tools.totalMsUser') : t('cards.tools.totalMs')
  return [
    `${r.key} · ${r.label}`,
    t('cards.tools.tipCalls', { calls: fmtInt(r.calls), failures: fmtInt(r.failures), pct: fmtPct(r.failRate) }),
    t('cards.tools.tipTotalAvg', { totalLabel: total, total: fmtMs(r.ms), avg }),
  ].join('\n')
}
</script>

<template>
  <!-- 卡片外壳在 Dashboard：和 ToolRank 共用一张卡，切换器不随视图重建 -->
  <div>
    <div v-if="rows.length === 0" class="py-8 text-center text-sm ink-soft">{{ t('cards.tools.empty') }}</div>
    <table v-else class="w-full border-collapse text-sm">
      <thead>
        <tr class="whitespace-nowrap text-xs ink-soft">
          <SortTh :sort="sort" k="label" align="left">{{ t('cards.tools.category') }}</SortTh>
          <SortTh :sort="sort" k="calls" align="left">{{ t('cards.tools.calls') }}</SortTh>
          <SortTh :sort="sort" k="failRate" class="w-16">{{ t('cards.tools.failRate') }}</SortTh>
          <SortTh :sort="sort" k="avgMs" class="w-20">{{ t('cards.tools.avgMs') }}</SortTh>
          <SortTh :sort="sort" k="ms" class="w-20 max-sm:hidden">{{ t('cards.tools.totalMs') }}</SortTh>
        </tr>
      </thead>
      <tbody>
        <tr v-for="r in sorted" :key="r.key" :title="tip(r)" class="surface-hover">
          <td class="whitespace-nowrap rounded-l-lg py-1 pl-1 pr-3">
            <span class="mr-2 inline-block h-2 w-2 rounded-full align-middle" :style="{ background: r.color }"></span>{{ r.label }}
          </td>
          <td class="w-2/5 py-1 pr-3">
            <div class="flex items-center gap-2">
              <div class="meter-track h-1.5 flex-1">
                <span class="meter-fill" :style="{ width: `${(r.calls / maxCalls) * 100}%`, background: r.color }"></span>
              </div>
              <span class="w-12 text-right text-xs num">{{ fmtBig(r.calls) }}</span>
            </div>
          </td>
          <td class="py-1 text-right text-xs num" :class="r.failRate > 0.05 ? 'ink-danger' : 'ink-soft'">
            {{ fmtPct(r.failRate, 1) }}
          </td>
          <td class="py-1 text-right text-xs num max-sm:rounded-r-lg max-sm:pr-1">{{ fmtMs(r.avgMs) }}</td>
          <td class="rounded-r-lg py-1 pr-1 text-right text-xs num max-sm:hidden">{{ fmtMs(r.ms) }}</td>
        </tr>
      </tbody>
    </table>
  </div>
</template>
