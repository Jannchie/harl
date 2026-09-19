<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRouter } from 'vue-router'
import type { SessionAgg } from '../../api'
import EChart from '../EChart.vue'
import type { EChartsOption } from '../../lib/echarts'
import { tooltipBase, ttRow, ttTitle, useChartTheme } from '../../lib/chartTheme'
import { fmtBig, fmtDateTime, fmtInt, fmtMoney, fmtMoneyExact, fmtMs } from '../../lib/format'
import { inkOn } from '../../lib/sunburst'
import { providerColor } from '../../lib/summaryModel'
import DashCard from './DashCard.vue'

const props = defineProps<{ sessions: SessionAgg[] }>()
const t = useChartTheme()
const router = useRouter()

type Metric = 'cost' | 'turns'
const metric = ref<Metric>('cost')
const pick = computed(() => (metric.value === 'cost' ? (s: SessionAgg) => s.cost_usd : (s: SessionAgg) => s.turns))
const fmt = computed(() => (metric.value === 'cost' ? fmtMoney : fmtInt))

interface Leaf {
  name: string
  value: number
  id: string
  session: SessionAgg
  itemStyle: { color: string }
  label: { color: string; show: boolean }
}
interface Group {
  name: string
  children: Leaf[]
  total: number
  color: string
  itemStyle: { color: string }
}
/** 面积占比低于这个的块不画标签（画了也只剩半个字） */
const LABEL_MIN_SHARE = 0.008

const data = computed<Group[]>(() => {
  const th = t.value
  const groups = new Map<string, Group>()
  for (const s of props.sessions) {
    const v = pick.value(s)
    if (v <= 0) continue
    let g = groups.get(s.provider)
    if (!g) {
      const c = providerColor(s.provider, th)
      g = { name: s.provider, children: [], total: 0, color: c, itemStyle: { color: c } }
      groups.set(s.provider, g)
    }
    g.total += v
    g.children.push({
      name: `${s.parent_session ? '↳ ' : ''}${s.session.slice(0, 8)}`,
      value: v,
      id: s.id,
      session: s,
      itemStyle: { color: g.color },
      label: { color: inkOn(g.color), show: true },
    })
  }
  const all = [...groups.values()].reduce((a, g) => a + g.total, 0)
  for (const g of groups.values()) for (const c of g.children) c.label.show = c.value / all >= LABEL_MIN_SHARE
  return [...groups.values()].sort((a, b) => a.name.localeCompare(b.name))
})

const shown = computed(() => data.value.reduce((a, g) => a + g.children.length, 0))

const option = computed<EChartsOption>(() => {
  const th = t.value
  return {
    tooltip: {
      ...tooltipBase(th),
      formatter: (p: unknown) => {
        const q = p as { name: string; value: number; data: Leaf | Group; treePathInfo: { name: string }[] }
        const leaf = q.data as Leaf
        if (!leaf.session) return ttTitle(q.name) + ttRow(null, metric.value === 'cost' ? '费用' : 'turns', fmt.value(q.value))
        const s = leaf.session
        return (
          ttTitle(s.session) +
          ttRow(null, 'provider', s.provider, true) +
          (s.parent_session ? ttRow(null, '父会话', s.parent_session.slice(0, 12), true) : '') +
          ttRow(null, '费用', fmtMoneyExact(s.cost_usd)) +
          ttRow(null, 'turns / steps / 调用', `${fmtInt(s.turns)} / ${fmtInt(s.steps)} / ${fmtInt(s.tool_calls)}`) +
          ttRow(null, 'tokens out / cache', `${fmtBig(s.tokens.output)} / ${fmtBig(s.tokens.cache_read)}`) +
          ttRow(null, '模型 / 工具 / 墙钟', `${fmtMs(s.model_ms)} / ${fmtMs(s.tool_ms)} / ${fmtMs(s.wall_ms)}`) +
          ttRow(null, '开始', fmtDateTime(s.first_ts), true) +
          `<div style="margin-top:4px;opacity:.6;font-size:11px">点击打开会话</div>`
        )
      },
    },
    series: [
      {
        type: 'treemap',
        data: data.value as never,
        width: '100%',
        height: '100%',
        top: 0,
        left: 0,
        roam: false,
        nodeClick: false,
        breadcrumb: { show: false },
        squareRatio: 1,
        visibleMin: 12,
        itemStyle: { borderColor: th.bg, borderWidth: 0, gapWidth: 2 },
        label: {
          show: true,
          fontSize: 10,
          lineHeight: 13,
          fontFamily: th.monoFamily,
          formatter: (q: { name: string; value: unknown }) => `${q.name}\n${fmt.value(Number(q.value))}`,
        },
        upperLabel: { show: false },
        // provider 组之间留 6px 空隙，组名放在图上方的 HTML 图例里
        levels: [{ itemStyle: { gapWidth: 6 } }, { itemStyle: { gapWidth: 6 } }, { itemStyle: { gapWidth: 2 } }],
        emphasis: { itemStyle: { opacity: 0.85 } },
      },
    ],
  }
})

function onClick(p: { data?: unknown }) {
  const d = p.data as Partial<Leaf> | undefined
  if (d?.id) router.push({ name: 'session', params: { id: d.id } })
}
</script>

<template>
  <DashCard n="09" title="会话" :caption="`${shown} 个会话（含子 agent，↳ 标记），块面积 = ${metric === 'cost' ? '费用' : 'turns'}；点击打开`">
    <template #controls>
      <button type="button" :class="{ active: metric === 'cost' }" @click="metric = 'cost'">费用</button>
      <button type="button" :class="{ active: metric === 'turns' }" @click="metric = 'turns'">turns</button>
    </template>
    <div v-if="data.length === 0" class="state">没有会话。</div>
    <template v-else>
      <div class="legend">
        <span v-for="g in data" :key="g.name" class="lg">
          <span class="sw" :style="{ background: g.color }"></span>
          <span class="mono">{{ g.name }}</span>
          <span class="muted mono">{{ g.children.length }} · {{ fmt(g.total) }}</span>
        </span>
      </div>
      <EChart :option="option" :height="440" class="tm" @click="onClick" />
    </template>
  </DashCard>
</template>

<style scoped>
.tm {
  cursor: pointer;
}
.legend {
  display: flex;
  gap: 16px;
  flex-wrap: wrap;
  padding: 2px 4px 6px;
  font-size: var(--fs-xs);
}
.lg {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
.sw {
  width: 10px;
  height: 10px;
  display: inline-block;
}
</style>
