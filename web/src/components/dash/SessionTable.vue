<script setup lang="ts">
// 会话排行：范围内的会话按费用 / 时长 / turns 等排，点一行打开会话。子 agent 是单独的会话，单独成行。
import { computed } from 'vue'
import { useRouter } from 'vue-router'
import { fmtAgo, fmtBig, fmtInt, fmtMoneyExact, fmtMs } from '../../lib/format'
import type { PricedSessionRow } from '../../lib/pricing'
import { projectOf } from '../../lib/project'
import { providerDot, providerName } from '../../lib/providers'
import { totalTokens } from '../../lib/summaryModel'
import { useSort, useTopN } from '../../lib/sort'
import DashCard from './DashCard.vue'
import SortTh from './SortTh.vue'

const props = defineProps<{ sessions: PricedSessionRow[] }>()
const router = useRouter()


const rows = computed(() =>
  props.sessions.map((s) => ({
    s,
    project: projectOf(s),
    tokens: totalTokens(s.tokens),
  })),
)

const { sort, sorted } = useSort(
  rows,
  {
    title: (r) => r.s.title ?? '',
    cost: (r) => r.s.cost_usd,
    turns: (r) => r.s.turns,
    tokens: (r) => r.tokens,
    tools: (r) => r.s.tool_calls,
    wall: (r) => r.s.wall_ms,
    ctx: (r) => r.s.peak_ctx,
    last: (r) => (r.s.last_ts ? Date.parse(r.s.last_ts) : null),
  },
  'cost',
)
const { shown, showAll, more } = useTopN(sorted, 15)

function open(id: string) {
  router.push({ name: 'session', params: { id } })
}

/** 行里的链接：普通点击走路由；带修饰键（新标签 / 新窗口）交给浏览器。上千行时比 RouterLink 轻 */
function onLink(e: MouseEvent, id: string) {
  if (e.ctrlKey || e.metaKey || e.shiftKey || e.altKey || e.button !== 0) return
  e.preventDefault()
  open(id)
}

const td = 'py-1.5 pl-4 text-right num'
</script>

<template>
  <DashCard icon="i-jannchie-chat" title="会话" title-hint="范围内有活动的会话；子 agent 单独成行。点一行打开会话">
    <div v-if="rows.length === 0" class="py-8 text-center text-sm ink-soft">没有会话。</div>
    <div v-else class="-mx-1 overflow-x-auto px-1">
      <table class="w-full table-fixed border-collapse text-sm">
        <thead>
          <tr class="whitespace-nowrap text-xs ink-soft">
            <SortTh :sort="sort" k="title" align="left">会话</SortTh>
            <SortTh :sort="sort" k="cost" class="w-24 pl-4">费用</SortTh>
            <SortTh :sort="sort" k="turns" class="w-18 pl-4">turns</SortTh>
            <SortTh :sort="sort" k="tokens" class="w-20 pl-4 max-md:hidden">tokens</SortTh>
            <SortTh :sort="sort" k="tools" class="w-22 pl-4 max-lg:hidden">工具调用</SortTh>
            <SortTh :sort="sort" k="wall" class="w-22 pl-4">墙钟</SortTh>
            <SortTh :sort="sort" k="ctx" class="w-22 pl-4 max-xl:hidden" title="单步最大的 prompt（新输入 + 缓存读写），即上下文最满的时候">峰值上下文</SortTh>
            <SortTh :sort="sort" k="last" class="w-22 pl-4 max-sm:hidden">最近</SortTh>
          </tr>
        </thead>
        <tbody>
          <tr v-for="r in shown" :key="r.s.id" class="cursor-pointer surface-hover" @click="open(r.s.id)">
            <td class="rounded-l-lg py-1.5 pl-1 pr-2">
              <div class="min-w-0 flex items-center gap-2">
                <span class="h-2 w-2 shrink-0 rounded-full" :class="providerDot(r.s.provider)" :title="providerName(r.s.provider)"></span>
                <span class="sr-only">{{ providerName(r.s.provider) }}</span>
                <i v-if="r.s.parent_session" class="i-jannchie-tree-view h-3.5 w-3.5 shrink-0 ink-soft" title="子 agent" role="img" aria-label="子 agent" />
                <!-- 整行可点是给鼠标的；键盘和读屏走这个链接（也能中键 / 新标签打开） -->
                <a
                  :href="`/s/${r.s.id}`"
                  class="min-w-0 truncate rounded kbd-ring"
                  :class="r.s.title ? 'ink-strong' : 'ink-soft'"
                  :title="r.s.title ?? undefined"
                  @click.stop="onLink($event, r.s.id)"
                >
                  {{ r.s.title ?? '（无标题）' }}
                </a>
                <span class="shrink-0 text-xs ink-soft max-md:hidden" :title="r.project.path">{{ r.project.label }}</span>
              </div>
            </td>
            <td :class="[td, 'font-medium']">{{ fmtMoneyExact(r.s.cost_usd) }}</td>
            <td :class="td">{{ fmtInt(r.s.turns) }}</td>
            <td :class="[td, 'max-md:hidden']" :title="fmtInt(r.tokens)">{{ fmtBig(r.tokens) }}</td>
            <td :class="[td, 'max-lg:hidden']">{{ fmtInt(r.s.tool_calls) }}</td>
            <td :class="td">{{ fmtMs(r.s.wall_ms) }}</td>
            <td
              :class="[td, 'max-xl:hidden']"
              :title="`${fmtInt(r.s.peak_ctx)} tokens${r.s.compactions ? `
压缩过 ${r.s.compactions} 次` : ''}`"
            >
              {{ fmtBig(r.s.peak_ctx) }}<span v-if="r.s.compactions" class="ml-1 ink-soft" aria-label="压缩过">↻{{ r.s.compactions }}</span>
            </td>
            <td :class="[td, 'rounded-r-lg pr-1 ink-soft max-sm:hidden']">{{ fmtAgo(r.s.last_ts) }}</td>
          </tr>
        </tbody>
      </table>
    </div>
    <template v-if="more" #footer>
      <button type="button" class="rounded kbd-ring hover:underline" @click="showAll = !showAll">
        {{ showAll ? '收起' : `显示全部 ${rows.length} 个会话` }}
      </button>
    </template>
  </DashCard>
</template>
