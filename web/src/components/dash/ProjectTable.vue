<script setup lang="ts">
// 按项目（会话的工作目录）汇总：费用由会话累加，会话按自己的首尾时间定价（见 lib/pricing.ts）。点一行就只看这个项目。
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { fmtAgo, fmtBig, fmtInt, fmtMoneyExact, fmtMs, fmtPct, tn } from '../../lib/format'
import type { PricedSessionRow } from '../../lib/pricing'
import { projectOf } from '../../lib/project'
import { totalTokens } from '../../lib/summaryModel'
import { useSort, useTopN } from '../../lib/sort'
import DashCard from './DashCard.vue'
import SortTh from './SortTh.vue'

const props = defineProps<{ sessions: PricedSessionRow[] }>()
const emit = defineEmits<{ pick: [key: string] }>()
const { t } = useI18n()

interface Row {
  key: string
  label: string
  path: string
  sessions: number
  turns: number
  tokens: number
  cost: number
  wall: number
  tools: number
  last: string | null
}

const rows = computed<Row[]>(() => {
  const m = new Map<string, Row>()
  for (const s of props.sessions) {
    const p = projectOf(s)
    let r = m.get(p.key)
    if (!r) {
      r = { key: p.key, label: p.label, path: p.path, sessions: 0, turns: 0, tokens: 0, cost: 0, wall: 0, tools: 0, last: null }
      m.set(p.key, r)
    }
    r.sessions += 1
    r.turns += s.turns
    r.tokens += totalTokens(s.tokens)
    r.cost += s.cost_usd
    r.wall += s.wall_ms
    r.tools += s.tool_calls
    if (s.last_ts && (!r.last || s.last_ts > r.last)) r.last = s.last_ts
  }
  return [...m.values()]
})
const totalCost = computed(() => rows.value.reduce((a, r) => a + r.cost, 0))

const { sort, sorted } = useSort(
  rows,
  {
    name: (r) => r.label,
    sessions: (r) => r.sessions,
    turns: (r) => r.turns,
    tokens: (r) => r.tokens,
    cost: (r) => r.cost,
    wall: (r) => r.wall,
    tools: (r) => r.tools,
    last: (r) => (r.last ? Date.parse(r.last) : null),
  },
  'cost',
)
const { shown, showAll, more } = useTopN(sorted, 12)

const td = 'py-1.5 pl-4 text-right num'
</script>

<template>
  <DashCard icon="i-jannchie-folder" :title="t('common.metric.project')" :title-hint="t('cards.projectTable.titleHint')">
    <div v-if="rows.length === 0" class="py-8 text-center text-sm ink-soft">{{ t('cards.common.noSessions') }}</div>
    <div v-else class="-mx-1 overflow-x-auto px-1">
      <table class="w-full border-collapse whitespace-nowrap text-sm">
        <thead>
          <tr class="text-xs ink-soft">
            <SortTh :sort="sort" k="name" align="left">{{ t('common.metric.project') }}</SortTh>
            <SortTh :sort="sort" k="cost" class="pl-4">{{ t('common.metric.cost') }}</SortTh>
            <SortTh :sort="sort" k="sessions" class="pl-4">{{ t('common.metric.sessions') }}</SortTh>
            <SortTh :sort="sort" k="turns" class="pl-4">turns</SortTh>
            <SortTh :sort="sort" k="tokens" class="pl-4">tokens</SortTh>
            <SortTh :sort="sort" k="tools" class="pl-4 max-md:hidden">{{ t('common.metric.toolCalls') }}</SortTh>
            <SortTh :sort="sort" k="wall" class="pl-4">{{ t('common.metric.wall') }}</SortTh>
            <SortTh :sort="sort" k="last" class="pl-4 max-sm:hidden">{{ t('cards.common.last') }}</SortTh>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="r in shown"
            :key="r.key"
            class="cursor-pointer surface-hover"
            :title="r.path || t('cards.projectTable.noCwd')"
            @click="emit('pick', r.key)"
          >
            <td class="max-w-60 truncate rounded-l-lg py-1.5 pl-1 pr-4" :class="r.key ? '' : 'ink-soft'">
              <!-- 整行可点是给鼠标的；键盘和读屏走这个按钮 -->
              <button
                type="button"
                class="max-w-full truncate rounded border-0 bg-transparent p-0 text-left text-inherit kbd-ring"
                :aria-label="t('cards.projectTable.only', { label: r.label })"
                @click.stop="emit('pick', r.key)"
              >
                <i class="i-jannchie-folder mr-1.5 h-3.5 w-3.5 ink-soft" aria-hidden="true" />{{ r.label }}
              </button>
            </td>
            <td :class="td">
              <span class="font-medium">{{ fmtMoneyExact(r.cost) }}</span>
              <span class="ml-1.5 inline-block w-10 text-xs ink-soft">{{ fmtPct(totalCost > 0 ? r.cost / totalCost : null, 0) }}</span>
            </td>
            <td :class="td">{{ fmtInt(r.sessions) }}</td>
            <td :class="td">{{ fmtInt(r.turns) }}</td>
            <td :class="td" :title="fmtInt(r.tokens)">{{ fmtBig(r.tokens) }}</td>
            <td :class="[td, 'max-md:hidden']">{{ fmtInt(r.tools) }}</td>
            <td :class="td">{{ fmtMs(r.wall) }}</td>
            <td :class="[td, 'rounded-r-lg pr-1 ink-soft max-sm:hidden']">{{ fmtAgo(r.last) }}</td>
          </tr>
        </tbody>
      </table>
    </div>
    <template v-if="more" #footer>
      <button type="button" class="rounded kbd-ring hover:underline" @click="showAll = !showAll">
        {{ showAll ? t('common.collapse') : tn('cards.projectTable.showAll', rows.length) }}
      </button>
    </template>
  </DashCard>
</template>
