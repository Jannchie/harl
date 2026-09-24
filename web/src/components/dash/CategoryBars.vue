<script setup lang="ts">
import { computed } from 'vue'
import type { ToolAgg, ToolCategory } from '../../api'
import { useChartTheme } from '../../lib/chartTheme'
import { fmtBig, fmtInt, fmtMs, fmtPct } from '../../lib/format'
import { categoryColor, categoryLabel } from '../../lib/summaryModel'
import { useSort } from '../../lib/sort'
import DashCard from './DashCard.vue'
import SortTh from './SortTh.vue'

const props = defineProps<{ byCategory: Partial<Record<ToolCategory, ToolAgg>> }>()
const t = useChartTheme()

interface Row {
  key: string
  label: string
  color: string
  calls: number
  failures: number
  failRate: number
  ms: number
}

/** 颜色跟着类别走；默认按调用数排，点列头换 */
const rows = computed<Row[]>(() =>
  Object.keys(props.byCategory).map((k) => {
    const x = props.byCategory[k as ToolCategory]!
    return {
      key: k,
      label: categoryLabel(k),
      color: categoryColor(k, t.value),
      calls: x.calls,
      failures: x.failures,
      failRate: x.calls > 0 ? x.failures / x.calls : 0,
      ms: x.ms,
    }
  }),
)
const { sort, sorted } = useSort(
  rows,
  { label: (r) => r.label, calls: (r) => r.calls, failRate: (r) => r.failRate, ms: (r) => r.ms },
  'calls',
)
const maxCalls = computed(() => Math.max(1, ...rows.value.map((r) => r.calls)))
const maxMs = computed(() => Math.max(1, ...rows.value.map((r) => r.ms)))

function tip(r: Row): string {
  const avg = fmtMs(r.calls > 0 ? r.ms / r.calls : null)
  const total = r.key === 'user' ? '总耗时（含等待用户）' : '总耗时'
  return `${r.key} · ${r.label}\n调用 ${fmtInt(r.calls)} · 失败 ${fmtInt(r.failures)}（${fmtPct(r.failRate)}）\n${total} ${fmtMs(r.ms)} · 平均 ${avg}`
}
</script>

<template>
  <DashCard
    icon="i-carbon-tool-box"
    title="工具"
    title-hint="「等用户」类别的耗时是在等人回答，不是执行时间"
  >
    <template v-if="$slots.controls" #controls><slot name="controls" /></template>
    <div v-if="rows.length === 0" class="py-8 text-center text-sm ink-soft">没有工具调用。</div>
    <table v-else class="w-full border-collapse text-sm">
      <thead>
        <tr class="whitespace-nowrap text-xs ink-soft">
          <SortTh :sort="sort" k="label" align="left">类别</SortTh>
          <SortTh :sort="sort" k="calls" align="left">调用</SortTh>
          <SortTh :sort="sort" k="failRate" class="w-16">失败率</SortTh>
          <SortTh :sort="sort" k="ms" align="left" class="pl-3 max-sm:hidden">总耗时</SortTh>
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
          <td class="w-1/4 rounded-r-lg py-1 pl-3 pr-1 max-sm:hidden">
            <div class="flex items-center gap-2">
              <div class="meter-track h-1.5 flex-1">
                <span class="meter-fill" :style="{ width: `${(r.ms / maxMs) * 100}%`, background: r.color }"></span>
              </div>
              <span class="w-16 text-right text-xs num">{{ fmtMs(r.ms) }}</span>
            </div>
          </td>
        </tr>
      </tbody>
    </table>
  </DashCard>
</template>
