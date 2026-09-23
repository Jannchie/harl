<script setup lang="ts">
import { computed } from 'vue'
import type { ToolAgg } from '../../api'
import { useChartTheme } from '../../lib/chartTheme'
import { fmtBig, fmtInt, fmtMs, fmtPct } from '../../lib/format'
import { categoryColor, categoryLabel } from '../../lib/summaryModel'
import DashCard from './DashCard.vue'

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

const rows = computed<Row[]>(() =>
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
    .sort((a, b) => b.calls - a.calls || a.name.localeCompare(b.name))
    .slice(0, TOP),
)

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
    icon="i-carbon-list-numbered"
    title="工具排行"
    :caption="`按调用次数取前 ${rows.length}，共 ${totalTools} 个工具；色点是类别`"
  >
    <div v-if="rows.length === 0" class="py-8 text-center text-sm ink-soft">没有工具调用。</div>
    <table v-else class="w-full table-fixed border-collapse text-sm">
      <thead>
        <tr class="whitespace-nowrap text-xs ink-soft">
          <th class="w-2/5 pb-1.5 text-left font-normal">工具</th>
          <th class="pb-1.5 text-left font-normal">调用</th>
          <th class="w-16 pb-1.5 text-right font-normal">失败率</th>
          <th class="w-20 pb-1.5 text-right font-normal">平均耗时</th>
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
