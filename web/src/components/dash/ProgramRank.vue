<script setup lang="ts">
// 命令排行：shell 调用里用到的程序（后端 programs.rs 拆出来的），次数、失败率、会话数、最近一次。
// 点一行只看用过它的会话（再点取消）
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { UseAgg } from '../../api'
import { useChartTheme } from '../../lib/chartTheme'
import { fmtAgo, fmtBig, fmtInt, fmtPct } from '../../lib/format'
import { categoryColor } from '../../lib/summaryModel'
import { useSort, useTopN } from '../../lib/sort'
import DashCard from './DashCard.vue'
import SortTh from './SortTh.vue'
import { withSelected } from './picks'

const props = defineProps<{
  programs: Record<string, UseAgg>
  selected?: string | null
}>()
const emit = defineEmits<{ pick: [name: string] }>()
const theme = useChartTheme()
const { t } = useI18n()

interface Row {
  name: string
  n: number
  failures: number
  failRate: number
  sessions: number
  last: string | null
}

const all = computed<Row[]>(() =>
  Object.entries(props.programs)
    .map(([name, x]) => {
      const failures = x.failures ?? 0
      return { name, n: x.n, failures, failRate: x.n > 0 ? failures / x.n : 0, sessions: x.sessions ?? 0, last: x.last_ts ?? null }
    })
    .sort((a, b) => a.name.localeCompare(b.name)),
)
const runs = computed(() => all.value.reduce((a, r) => a + r.n, 0))

const { sort, sorted } = useSort(
  all,
  {
    name: (r) => r.name,
    n: (r) => r.n,
    failRate: (r) => r.failRate,
    sessions: (r) => r.sessions,
    last: (r) => (r.last ? Date.parse(r.last) : null),
  },
  'n',
)
const { shown: top, showAll, more } = useTopN(sorted, 15)
const shown = computed(() => withSelected(top.value, sorted.value, props.selected, (r) => r.name))
const maxN = computed(() => Math.max(1, ...shown.value.map((r) => r.n)))
// 和工具排行里 shell 类别同色
const color = computed(() => categoryColor('shell', theme.value))

function tip(r: Row): string {
  return [
    r.name,
    `${t('dashMore.programs.runs')} ${fmtInt(r.n)} · ${t('dashMore.programs.failRate')} ${fmtPct(r.failRate)} (${fmtInt(r.failures)})`,
    `${t('dashMore.rank.sessions')} ${fmtInt(r.sessions)}${r.last ? ` · ${t('dashMore.rank.last')} ${fmtAgo(r.last)}` : ''}`,
    props.selected === r.name ? t('dashMore.rank.clickRelease') : t('dashMore.rank.clickFilter'),
  ].join('\n')
}
</script>

<template>
  <DashCard
    icon="i-jannchie-terminal"
    :title="t('dashMore.programs.title')"
    :title-hint="t('dashMore.programs.titleHint')"
    :caption="all.length ? t('dashMore.programs.summary', { n: fmtInt(all.length), runs: fmtInt(runs) }) : undefined"
  >
    <div v-if="all.length === 0" class="py-8 text-center text-sm ink-soft">{{ t('dashMore.programs.empty') }}</div>
    <table v-else class="w-full table-fixed border-collapse text-sm">
      <thead>
        <tr class="whitespace-nowrap text-xs ink-soft">
          <SortTh :sort="sort" k="name" align="left" class="w-2/5">{{ t('dashMore.programs.program') }}</SortTh>
          <SortTh :sort="sort" k="n" align="left">{{ t('dashMore.programs.runs') }}</SortTh>
          <SortTh :sort="sort" k="failRate" class="w-16">{{ t('dashMore.programs.failRate') }}</SortTh>
          <SortTh :sort="sort" k="sessions" class="w-16 max-sm:hidden">{{ t('dashMore.rank.sessions') }}</SortTh>
          <SortTh :sort="sort" k="last" class="w-18 max-md:hidden">{{ t('dashMore.rank.last') }}</SortTh>
        </tr>
      </thead>
      <tbody>
        <tr
          v-for="r in shown"
          :key="r.name"
          :title="tip(r)"
          class="cursor-pointer"
          :class="selected === r.name ? 'row-selected' : 'surface-hover'"
          @click="emit('pick', r.name)"
        >
          <td class="rounded-l-lg py-1 pl-1 pr-3">
            <button
              type="button"
              class="max-w-full flex items-center gap-2 rounded border-0 bg-transparent p-0 text-left kbd-ring"
              :aria-pressed="selected === r.name"
              @click.stop="emit('pick', r.name)"
            >
              <span class="h-2 w-2 shrink-0 rounded-full" :style="{ background: color }"></span>
              <span class="min-w-0 truncate font-mono text-xs">{{ r.name }}</span>
            </button>
          </td>
          <td class="py-1 pr-3">
            <div class="flex items-center gap-2">
              <div class="meter-track h-1.5 flex-1">
                <span class="meter-fill" :style="{ width: `${(r.n / maxN) * 100}%`, background: color }"></span>
              </div>
              <span class="w-12 text-right text-xs num">{{ fmtBig(r.n) }}</span>
            </div>
          </td>
          <td class="py-1 text-right text-xs num max-sm:rounded-r-lg max-sm:pr-1" :class="r.failRate > 0.05 ? 'ink-danger' : 'ink-soft'">
            {{ fmtPct(r.failRate, 1) }}
          </td>
          <td class="py-1 text-right text-xs num max-sm:hidden max-md:rounded-r-lg max-md:pr-1">{{ fmtInt(r.sessions) }}</td>
          <td class="rounded-r-lg py-1 pr-1 text-right text-xs num ink-soft max-md:hidden">{{ fmtAgo(r.last) }}</td>
        </tr>
      </tbody>
    </table>
    <template v-if="more" #footer>
      <button type="button" class="rounded border-0 bg-transparent p-0 text-xs ink-soft kbd-ring hover:underline" @click="showAll = !showAll">
        {{ showAll ? t('common.collapse') : t('dashMore.rank.showAll', { n: fmtInt(all.length) }) }}
      </button>
    </template>
  </DashCard>
</template>
