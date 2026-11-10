<script setup lang="ts">
// 会话卡片：一张卡一个会话。计费输入的组成条、费用、计费 tokens、轮 / 步、当前上下文、项目、最后一条消息；
// 正在写的标「进行中」，属于某个会话家族（父会话 + 子 agent）时给出家族在范围内的合计。整张卡是去会话页的链接
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { RouterLink } from 'vue-router'
import { useChartTheme } from '../../lib/chartTheme'
import { fmtAgo, fmtBig, fmtInt, fmtMoneyExact } from '../../lib/format'
import { providerDot, providerName } from '../../lib/providers'
import { mixAria, mixSegs, mixTotal } from './mix'
import type { ListRow } from './listRow'

const props = defineProps<{ rows: ListRow[] }>()
const theme = useChartTheme()
const { t } = useI18n()

const cards = computed(() =>
  props.rows.map((r) => {
    const segs = mixSegs(r.s.composition, theme.value)
    const compTotal = mixTotal(r.s.composition)
    return { r, segs: segs.filter((x) => x.value > 0), compTotal, aria: compTotal > 0 ? t('dashMore.card.compAria', { items: mixAria(segs) }) : t('dashMore.card.noComp') }
  }),
)
</script>

<template>
  <ul class="m-0 grid list-none grid-cols-1 gap-2 p-0 md:grid-cols-2 xl:grid-cols-3">
    <li v-for="c in cards" :key="c.r.s.id" class="min-w-0">
      <RouterLink
        :to="{ name: 'session', params: { id: c.r.s.id } }"
        class="h-full flex flex-col gap-2 rounded-xl p-3 text-inherit no-underline surface-strong kbd-ring transition-colors duration-150 hover:bg-black/8 dark:hover:bg-white/9"
      >
        <!-- 标题行：provider、子 agent、标题、进行中 -->
        <div class="min-w-0 flex items-center gap-2">
          <span class="h-2 w-2 shrink-0 rounded-full" :class="providerDot(c.r.s.provider)" :title="providerName(c.r.s.provider)"></span>
          <span class="sr-only">{{ providerName(c.r.s.provider) }}</span>
          <i
            v-if="c.r.s.parent_session"
            class="i-jannchie-tree-view h-3.5 w-3.5 shrink-0 ink-soft"
            :title="t('dash.category.agent')"
            role="img"
            :aria-label="t('dash.category.agent')"
          />
          <span class="min-w-0 flex-1 truncate text-sm font-medium" :class="c.r.s.title ? 'ink-strong' : 'ink-soft'" :title="c.r.s.title ?? undefined">
            {{ c.r.s.title ?? t('cards.common.untitled') }}
          </span>
          <span
            v-if="c.r.running"
            class="shrink-0 flex items-center gap-1 rounded-full bg-emerald-500/12 px-2 py-0.5 text-[11px] ink-success"
            :title="t('dashMore.card.runningHint')"
          >
            <span class="h-1.5 w-1.5 animate-pulse rounded-full bg-current"></span>{{ t('dashMore.card.running') }}
          </span>
        </div>
        <div class="min-w-0 flex items-center gap-2 text-xs ink-soft">
          <span class="min-w-0 truncate" :title="c.r.project.path">{{ c.r.project.label }}</span>
          <span v-if="c.r.s.last_ts" class="shrink-0 num">· {{ fmtAgo(c.r.s.last_ts, c.r.now) }}</span>
        </div>

        <!-- 计费输入的组成：和上面的组成环图同一套类别和颜色 -->
        <div class="h-1.5 w-full flex overflow-hidden rounded-full surface" role="img" :aria-label="c.aria">
          <span v-for="g in c.segs" :key="g.key" class="h-full" :style="{ width: `${g.share * 100}%`, background: g.color }" :title="g.tip"></span>
        </div>

        <dl class="m-0 grid grid-cols-2 gap-x-3 gap-y-0.5 text-xs">
          <div class="min-w-0 flex items-baseline justify-between gap-2">
            <dt class="ink-soft">{{ t('dashMore.card.cost') }}</dt>
            <dd class="m-0 font-medium num">{{ fmtMoneyExact(c.r.s.cost_usd) }}</dd>
          </div>
          <div class="min-w-0 flex items-baseline justify-between gap-2">
            <dt class="ink-soft">{{ t('dashMore.card.billed') }}</dt>
            <dd class="m-0 num" :title="fmtInt(c.r.tokens)">{{ fmtBig(c.r.tokens) }}</dd>
          </div>
          <div class="min-w-0 flex items-baseline justify-between gap-2">
            <dt class="ink-soft">turns</dt>
            <dd class="m-0 truncate num">{{ t('dashMore.card.turns', { turns: fmtInt(c.r.s.turns), steps: fmtInt(c.r.s.steps) }) }}</dd>
          </div>
          <div class="min-w-0 flex items-baseline justify-between gap-2" :title="`${t('dashMore.card.ctxHint')}${c.r.s.ctx_estimated ? `\n${t('dashMore.card.ctxEstimated')}` : ''}`">
            <dt class="ink-soft">{{ t('dashMore.card.ctx') }}</dt>
            <dd class="m-0 num">{{ c.r.s.ctx_estimated ? '≈' : '' }}{{ fmtBig(c.r.s.ctx) }}</dd>
          </div>
        </dl>

        <p v-if="c.r.s.last_user" class="m-0 line-clamp-2 break-words text-xs ink-muted" :title="c.r.s.last_user">
          <span class="sr-only">{{ t('dashMore.card.lastUser') }}: </span>
          <i class="i-jannchie-user mr-1 h-3 w-3 ink-soft" aria-hidden="true" />{{ c.r.s.last_user }}
        </p>

        <div
          v-if="c.r.family"
          class="mt-auto flex items-center gap-1.5 text-xs ink-soft"
          :title="t('dashMore.card.familyHint', { tokens: fmtInt(c.r.family.tokens) })"
        >
          <i class="i-jannchie-network h-3.5 w-3.5 shrink-0" aria-hidden="true" />
          <span class="num">{{ t('dashMore.card.family', { cost: fmtMoneyExact(c.r.family.cost), n: fmtInt(c.r.family.n) }) }}</span>
        </div>
      </RouterLink>
    </li>
  </ul>
</template>
