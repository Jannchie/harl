<script setup lang="ts">
import { computed } from 'vue'
import type { ToolAgg } from '../../api'
import { useChartTheme } from '../../lib/chartTheme'
import { fmtBig, fmtInt, fmtMs, fmtPct } from '../../lib/format'
import { categoryColor, categoryLabel } from '../../lib/summaryModel'
import { useSort } from '../../lib/sort'
import DashCard from './DashCard.vue'
import SortTh from './SortTh.vue'

const props = defineProps<{ byTool: Record<string, ToolAgg> }>()
const t = useChartTheme()

const TOP = 15

interface Row {
  name: string
  category: string
  color: string
  calls: number
  failures: number
  failRate: number
  avgMs: number
}

const all = computed<Row[]>(() =>
  Object.entries(props.byTool)
    .map(([name, x]) => ({
      name,
      category: x.category,
      color: categoryColor(x.category, t.value),
      calls: x.calls,
      failures: x.failures,
      failRate: x.calls > 0 ? x.failures / x.calls : 0,
      avgMs: x.calls > 0 ? x.ms / x.calls : 0,
    }))
    .sort((a, b) => a.name.localeCompare(b.name)),
)
/** 先按当前列排好全部工具，再取前 TOP 个：按失败率排看到的就是失败率最高的那些 */
const { sort, sorted } = useSort(
  all,
  { name: (r) => r.name, calls: (r) => r.calls, failRate: (r) => r.failRate, avgMs: (r) => r.avgMs },
  'calls',
)
const rows = computed(() => sorted.value.slice(0, TOP))

const totalTools = computed(() => Object.keys(props.byTool).length)
const maxCalls = computed(() => Math.max(1, ...rows.value.map((r) => r.calls)))

function tip(r: Row): string {
  const user = r.category === 'user'
  const avg = user ? '平均耗时（含等待用户）' : '平均耗时'
  return `${r.name}\n类别 ${r.category} · ${categoryLabel(r.category)}\n调用 ${fmtInt(r.calls)} · 失败 ${fmtInt(r.failures)}（${fmtPct(r.failRate)}）\n${avg} ${fmtMs(r.avgMs)} · 总 ${fmtMs(r.avgMs * r.calls)}`
}
</script>

<template>
  <DashCard
    icon="i-carbon-tool-box"
    title="工具"
    :caption="`共 ${totalTools} 个工具`"
  >
    <template v-if="$slots.controls" #controls><slot name="controls" /></template>
    <div v-if="rows.length === 0" class="py-8 text-center text-sm ink-soft">没有工具调用。</div>
    <table v-else class="w-full table-fixed border-collapse text-sm">
      <thead>
        <tr class="whitespace-nowrap text-xs ink-soft">
          <SortTh :sort="sort" k="name" align="left" class="w-2/5">工具</SortTh>
          <SortTh :sort="sort" k="calls" align="left">调用</SortTh>
          <SortTh :sort="sort" k="failRate" class="w-16">失败率</SortTh>
          <SortTh :sort="sort" k="avgMs" class="w-20">平均耗时</SortTh>
        </tr>
      </thead>
      <tbody>
        <tr v-for="r in rows" :key="r.name" :title="tip(r)" class="surface-hover">
          <td class="rounded-l-lg py-1 pl-1 pr-3">
            <div class="flex items-center gap-2">
              <span class="h-2 w-2 shrink-0 rounded-full" :style="{ background: r.color }"></span>
              <span class="min-w-0 truncate font-mono text-xs">{{ r.name }}</span>
            </div>
          </td>
          <td class="py-1 pr-3">
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
          <td class="rounded-r-lg py-1 pr-1 text-right text-xs num">{{ fmtMs(r.avgMs) }}</td>
        </tr>
      </tbody>
    </table>
  </DashCard>
</template>
