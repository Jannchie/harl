<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { ToolAgg } from '../../api'
import { useChartTheme } from '../../lib/chartTheme'
import { fmtAgo, fmtBig, fmtInt, fmtMs, fmtPct } from '../../lib/format'
import { categoryColor, categoryLabel } from '../../lib/summaryModel'
import { useSort } from '../../lib/sort'
import SortTh from './SortTh.vue'

const props = defineProps<{
  byTool: Record<string, ToolAgg>
  /** 正在按它筛会话的工具 */
  selected?: string | null
}>()
/** 点一行：父组件切换「只看用过它的会话」 */
const emit = defineEmits<{ pick: [name: string] }>()
const theme = useChartTheme()
const { t } = useI18n()

const TOP = 15

interface Row {
  name: string
  category: string
  color: string
  calls: number
  failures: number
  failRate: number
  avgMs: number
  ms: number
  sessions: number
  last: string | null
}

const all = computed<Row[]>(() =>
  Object.entries(props.byTool)
    .map(([name, x]) => ({
      name,
      category: x.category,
      color: categoryColor(x.category, theme.value),
      calls: x.calls,
      failures: x.failures,
      failRate: x.calls > 0 ? x.failures / x.calls : 0,
      avgMs: x.calls > 0 ? x.ms / x.calls : 0,
      ms: x.ms,
      sessions: x.sessions ?? 0,
      last: x.last_ts ?? null,
    }))
    .sort((a, b) => a.name.localeCompare(b.name)),
)
/** 先按当前列排好全部工具，再取前 TOP 个：按失败率排看到的就是失败率最高的那些 */
const { sort, sorted } = useSort(
  all,
  {
    name: (r) => r.name,
    calls: (r) => r.calls,
    failRate: (r) => r.failRate,
    avgMs: (r) => r.avgMs,
    ms: (r) => r.ms,
    sessions: (r) => r.sessions,
    last: (r) => (r.last ? Date.parse(r.last) : null),
  },
  'calls',
)
/** 选中的工具不在前 TOP 里时补在末尾，筛选状态总能看见、能点掉 */
const rows = computed(() => {
  const top = sorted.value.slice(0, TOP)
  const sel = props.selected
  if (sel && !top.some((r) => r.name === sel)) {
    const r = sorted.value.find((x) => x.name === sel)
    if (r) top.push(r)
  }
  return top
})

const maxCalls = computed(() => Math.max(1, ...rows.value.map((r) => r.calls)))

function tip(r: Row): string {
  const user = r.category === 'user'
  const avg = user ? t('cards.tools.avgMsUser') : t('cards.tools.avgMs')
  return [
    r.name,
    t('cards.tools.tipCategory', { key: r.category, label: categoryLabel(r.category) }),
    t('cards.tools.tipCalls', { calls: fmtInt(r.calls), failures: fmtInt(r.failures), pct: fmtPct(r.failRate) }),
    t('cards.tools.tipAvgTotal', { avgLabel: avg, avg: fmtMs(r.avgMs), total: fmtMs(r.ms) }),
    `${t('dashMore.rank.sessions')} ${fmtInt(r.sessions)}${r.last ? ` · ${t('dashMore.rank.last')} ${fmtAgo(r.last)}` : ''}`,
    props.selected === r.name ? t('dashMore.rank.clickRelease') : t('dashMore.rank.clickFilter'),
  ].join('\n')
}
</script>

<template>
  <!-- 卡片外壳在 Dashboard：和 CategoryBars 共用一张卡，切换器不随视图重建 -->
  <div>
    <div v-if="rows.length === 0" class="py-8 text-center text-sm ink-soft">{{ t('cards.tools.empty') }}</div>
    <table v-else class="w-full table-fixed border-collapse text-sm">
      <thead>
        <tr class="whitespace-nowrap text-xs ink-soft">
          <SortTh :sort="sort" k="name" align="left" class="w-2/5">{{ t('common.metric.tool') }}</SortTh>
          <SortTh :sort="sort" k="calls" align="left">{{ t('cards.tools.calls') }}</SortTh>
          <SortTh :sort="sort" k="failRate" class="w-16">{{ t('cards.tools.failRate') }}</SortTh>
          <SortTh :sort="sort" k="avgMs" class="w-20">{{ t('cards.tools.avgMs') }}</SortTh>
          <SortTh :sort="sort" k="ms" class="w-20 max-lg:hidden">{{ t('cards.tools.totalMs') }}</SortTh>
          <SortTh :sort="sort" k="sessions" class="w-16 max-sm:hidden" :title="t('dashMore.tools.sessionsHint')">{{ t('dashMore.rank.sessions') }}</SortTh>
          <SortTh :sort="sort" k="last" class="w-18 max-md:hidden">{{ t('dashMore.rank.last') }}</SortTh>
        </tr>
      </thead>
      <tbody>
        <tr
          v-for="r in rows"
          :key="r.name"
          :title="tip(r)"
          class="cursor-pointer"
          :class="selected === r.name ? 'row-selected' : 'surface-hover'"
          @click="emit('pick', r.name)"
        >
          <td class="rounded-l-lg py-1 pl-1 pr-3">
            <!-- 整行可点是给鼠标的；键盘走这个按钮 -->
            <button
              type="button"
              class="max-w-full flex items-center gap-2 rounded border-0 bg-transparent p-0 text-left kbd-ring"
              :aria-pressed="selected === r.name"
              @click.stop="emit('pick', r.name)"
            >
              <span class="h-2 w-2 shrink-0 rounded-full" :style="{ background: r.color }"></span>
              <span class="min-w-0 truncate font-mono text-xs">{{ r.name }}</span>
            </button>
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
          <td class="py-1 text-right text-xs num max-sm:rounded-r-lg max-sm:pr-1">{{ fmtMs(r.avgMs) }}</td>
          <td class="py-1 text-right text-xs num max-lg:hidden">{{ fmtMs(r.ms) }}</td>
          <td class="py-1 text-right text-xs num max-sm:hidden max-md:rounded-r-lg max-md:pr-1">{{ fmtInt(r.sessions) }}</td>
          <td class="rounded-r-lg py-1 pr-1 text-right text-xs num ink-soft max-md:hidden">{{ fmtAgo(r.last) }}</td>
        </tr>
      </tbody>
    </table>
  </div>
</template>
