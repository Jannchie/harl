<script setup lang="ts">
// 会话列表：卡片和表格两种视图（偏好记在 localStorage），共用搜索和点选筛选（工具 / skill / 命令，AND）。
// 卡片按下拉的排序分页；表格自己点列头排序、前 15 行可展开
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import type { PricedSessionRow } from '../../lib/pricing'
import { usePref } from '../../lib/ctxView'
import { useNow } from '../../lib/now'
import { bySession, sessions as sessionList } from '../../lib/sessions'
import { fmtInt } from '../../lib/format'
import Segmented from '../Segmented.vue'
import DashCard from './DashCard.vue'
import SessionCards from './SessionCards.vue'
import SessionTable from './SessionTable.vue'
import { buildRows, matchQuery, type ListRow } from './listRow'
import { anyPick, matchPicks, type Picks } from './picks'

const props = defineProps<{
  sessions: PricedSessionRow[]
  picks: Picks
}>()
const { t } = useI18n()

type View = 'cards' | 'table'
const view = usePref<View>('dash.sessionView', 'cards')
const VIEWS = computed(() => [
  { value: 'cards' as const, label: t('dashMore.sessions.cards'), icon: 'i-jannchie-layout-grid', compact: true },
  { value: 'table' as const, label: t('dashMore.sessions.table'), icon: 'i-jannchie-list', compact: true },
])

type SortKey = 'recent' | 'cost' | 'tokens' | 'ctx' | 'turns'
const sortKey = usePref<SortKey>('dash.sessionSort', 'recent')
const SORTS = computed(() =>
  (['recent', 'cost', 'tokens', 'ctx', 'turns'] as const).map((k) => ({ value: k, label: t(`dashMore.sessions.sort.${k}`) })),
)
const SORT_BY: Record<SortKey, (r: ListRow) => number> = {
  recent: (r) => (r.s.last_ts ? Date.parse(r.s.last_ts) : 0),
  cost: (r) => r.s.cost_usd,
  tokens: (r) => r.tokens,
  ctx: (r) => r.s.ctx ?? 0,
  turns: (r) => r.s.turns,
}

const query = ref('')

/** 「进行中」和「几分钟前」要往前走：每 30 秒换一次此刻 */
const now = useNow(30_000)

const rows = computed(() => buildRows(props.sessions, sessionList.value, bySession.value, now.value))
const narrowed = computed(() => anyPick(props.picks) || query.value.trim() !== '')
const filtered = computed(() => rows.value.filter((r) => matchPicks(r.s, props.picks) && matchQuery(r, query.value)))

const sorted = computed(() => {
  const get = SORT_BY[sortKey.value] ?? SORT_BY.recent
  return [...filtered.value].sort((a, b) => get(b) - get(a))
})

const PAGE = 12
const page = ref(0)
const pages = computed(() => Math.max(1, Math.ceil(sorted.value.length / PAGE)))
const paged = computed(() => sorted.value.slice(page.value * PAGE, (page.value + 1) * PAGE))
// 筛选、搜索、排序一变就回第一页；数据刷新后页数变少时别停在空页
watch([() => props.picks, query, sortKey, view], () => (page.value = 0), { deep: true })
watch(pages, (n) => {
  if (page.value >= n) page.value = n - 1
})
</script>

<template>
  <DashCard
    icon="i-jannchie-chat"
    :title="t('common.metric.sessions')"
    :title-hint="t('cards.sessionTable.titleHint')"
    :caption="narrowed ? t('dashMore.sessions.matched', { n: fmtInt(filtered.length), total: fmtInt(rows.length) }) : undefined"
  >
    <template #controls>
      <Segmented v-model="view" :label="t('dashMore.sessions.view')" :options="VIEWS" />
    </template>
    <div class="mb-3 flex flex-wrap items-center gap-2">
      <div class="relative min-w-48 max-w-80 flex-1">
        <i class="i-jannchie-search pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 ink-soft" aria-hidden="true" />
        <input v-model="query" type="search" class="field py-1 pl-8 text-xs" :placeholder="t('dashMore.sessions.search')" :aria-label="t('dashMore.sessions.search')" />
      </div>
      <Segmented v-if="view === 'cards'" v-model="sortKey" :label="t('dashMore.sessions.sortLabel')" :options="SORTS" />
    </div>

    <div v-if="rows.length > 0 && filtered.length === 0" class="py-8 text-center text-sm ink-soft">{{ t('dashMore.sessions.noMatch') }}</div>
    <template v-else-if="view === 'cards'">
      <div v-if="rows.length === 0" class="py-8 text-center text-sm ink-soft">{{ t('cards.common.noSessions') }}</div>
      <SessionCards v-else :rows="paged" />
    </template>
    <SessionTable v-else :rows="filtered" />

    <template v-if="view === 'cards' && pages > 1" #footer>
      <nav class="flex items-center justify-end gap-2" :aria-label="t('dashMore.sessions.pager')">
        <button
          type="button"
          class="icon-button-sm border-0 bg-transparent disabled:opacity-40"
          :disabled="page === 0"
          :title="t('dashMore.sessions.prev')"
          :aria-label="t('dashMore.sessions.prev')"
          @click="page--"
        >
          <i class="i-jannchie-chevron-left h-4 w-4" />
        </button>
        <span class="num" aria-live="polite">{{ t('dashMore.sessions.page', { n: fmtInt(page + 1), total: fmtInt(pages) }) }}</span>
        <button
          type="button"
          class="icon-button-sm border-0 bg-transparent disabled:opacity-40"
          :disabled="page >= pages - 1"
          :title="t('dashMore.sessions.next')"
          :aria-label="t('dashMore.sessions.next')"
          @click="page++"
        >
          <i class="i-jannchie-chevron-right h-4 w-4" />
        </button>
      </nav>
    </template>
  </DashCard>
</template>
