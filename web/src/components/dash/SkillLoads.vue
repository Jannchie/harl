<script setup lang="ts">
// Skill 加载排行：次数、用过的会话数、最近一次、来源和路径。点一行只看加载过它的会话（再点取消）。
// 和 EffortSkill 的「按步归属」不是一个口径：这里数的是加载动作本身
import { computed, onBeforeUnmount, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import type { UseAgg } from '../../api'
import { useChartTheme } from '../../lib/chartTheme'
import { catColor } from '../../lib/ctxView'
import { fmtAgo, fmtBig, fmtInt } from '../../lib/format'
import { useSort, useTopN } from '../../lib/sort'
import DashCard from './DashCard.vue'
import SortTh from './SortTh.vue'
import { withSelected } from './picks'

const props = defineProps<{
  skills: Record<string, UseAgg>
  selected?: string | null
}>()
const emit = defineEmits<{ pick: [name: string] }>()
const theme = useChartTheme()
const { t } = useI18n()

interface Row {
  name: string
  n: number
  sessions: number
  last: string | null
  path: string | null
  source: UseAgg['source'] | null
}

const all = computed<Row[]>(() =>
  Object.entries(props.skills)
    .map(([name, x]) => ({ name, n: x.n, sessions: x.sessions ?? 0, last: x.last_ts ?? null, path: x.path ?? null, source: x.source ?? null }))
    .sort((a, b) => a.name.localeCompare(b.name)),
)
const loads = computed(() => all.value.reduce((a, r) => a + r.n, 0))

const { sort, sorted } = useSort(
  all,
  {
    name: (r) => r.name,
    n: (r) => r.n,
    sessions: (r) => r.sessions,
    last: (r) => (r.last ? Date.parse(r.last) : null),
    source: (r) => r.source,
  },
  'n',
)
const { shown: top, showAll, more } = useTopN(sorted, 12)
const shown = computed(() => withSelected(top.value, sorted.value, props.selected, (r) => r.name))
const maxN = computed(() => Math.max(1, ...shown.value.map((r) => r.n)))
const color = computed(() => catColor('skill', theme.value))

function tip(r: Row): string {
  return [
    r.name,
    `${t('dashMore.skills.loads')} ${fmtInt(r.n)} · ${t('dashMore.rank.sessions')} ${fmtInt(r.sessions)}`,
    r.last ? `${t('dashMore.rank.last')} ${fmtAgo(r.last)}` : '',
    r.source ? `${t('dashMore.skills.source')} ${t(`dashMore.skills.src.${r.source}`)}` : '',
    r.path ? `${t('dashMore.skills.path')} ${r.path}` : '',
    props.selected === r.name ? t('dashMore.rank.clickRelease') : t('dashMore.rank.clickFilter'),
  ]
    .filter(Boolean)
    .join('\n')
}

/** 复制路径：只记最近复制的那一行，1.5 秒后恢复图标 */
const copied = ref<string | null>(null)
let copyTimer: ReturnType<typeof setTimeout> | undefined
async function copy(r: Row) {
  if (!r.path) return
  try {
    await navigator.clipboard.writeText(r.path)
    copied.value = r.name
    clearTimeout(copyTimer)
    copyTimer = setTimeout(() => (copied.value = null), 1500)
  } catch {
    // 剪贴板不可用（非安全上下文）就算了
  }
}
onBeforeUnmount(() => clearTimeout(copyTimer))
</script>

<template>
  <DashCard
    icon="i-jannchie-puzzle"
    :title="t('dashMore.skills.title')"
    :title-hint="t('dashMore.skills.titleHint')"
    :caption="all.length ? t('dashMore.skills.summary', { skills: fmtInt(all.length), loads: fmtInt(loads) }) : undefined"
  >
    <div v-if="all.length === 0" class="py-8 text-center text-sm ink-soft">{{ t('dashMore.skills.empty') }}</div>
    <table v-else class="w-full table-fixed border-collapse text-sm">
      <thead>
        <tr class="whitespace-nowrap text-xs ink-soft">
          <SortTh :sort="sort" k="name" align="left" class="w-1/2">Skill</SortTh>
          <SortTh :sort="sort" k="n" align="left">{{ t('dashMore.skills.loads') }}</SortTh>
          <SortTh :sort="sort" k="sessions" class="w-16 max-sm:hidden">{{ t('dashMore.rank.sessions') }}</SortTh>
          <SortTh :sort="sort" k="last" class="w-18 max-md:hidden">{{ t('dashMore.rank.last') }}</SortTh>
        </tr>
      </thead>
      <tbody>
        <tr
          v-for="r in shown"
          :key="r.name"
          :title="tip(r)"
          class="group cursor-pointer"
          :class="selected === r.name ? 'row-selected' : 'surface-hover'"
          @click="emit('pick', r.name)"
        >
          <td class="rounded-l-lg py-1 pl-1 pr-3">
            <div class="min-w-0 flex items-center gap-2">
              <button
                type="button"
                class="min-w-0 flex items-center gap-2 rounded border-0 bg-transparent p-0 text-left kbd-ring"
                :aria-pressed="selected === r.name"
                @click.stop="emit('pick', r.name)"
              >
                <span class="h-2 w-2 shrink-0 rounded-full" :style="{ background: color }"></span>
                <span class="min-w-0 truncate font-mono text-xs">{{ r.name }}</span>
              </button>
              <span v-if="r.source" class="shrink-0 rounded px-1.5 text-[11px] surface-strong ink-muted">{{ t(`dashMore.skills.src.${r.source}`) }}</span>
              <button
                v-if="r.path"
                type="button"
                class="row-action icon-button-sm h-5 w-5 border-0 bg-transparent"
                :title="copied === r.name ? t('dashMore.skills.copied') : `${t('dashMore.skills.copyPath')}\n${r.path}`"
                :aria-label="copied === r.name ? t('dashMore.skills.copied') : t('dashMore.skills.copyPath')"
                @click.stop="copy(r)"
              >
                <i class="h-3.5 w-3.5" :class="copied === r.name ? 'i-jannchie-check ink-success' : 'i-jannchie-copy'" />
              </button>
            </div>
            <div v-if="r.path" class="truncate pl-4 font-mono text-[11px] ink-soft">{{ r.path }}</div>
          </td>
          <td class="py-1 pr-3">
            <div class="flex items-center gap-2">
              <div class="meter-track h-1.5 flex-1">
                <span class="meter-fill" :style="{ width: `${(r.n / maxN) * 100}%`, background: color }"></span>
              </div>
              <span class="w-10 text-right text-xs num">{{ fmtBig(r.n) }}</span>
            </div>
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
