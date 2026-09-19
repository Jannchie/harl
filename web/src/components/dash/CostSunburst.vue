<script setup lang="ts">
import { computed } from 'vue'
import type { ModelAgg, ProviderAgg } from '../../api'
import EChart from '../EChart.vue'
import type { EChartsOption } from '../../lib/echarts'
import { tooltipBase, ttRow, ttTitle, useChartTheme } from '../../lib/chartTheme'
import { fmtInt, fmtMoneyExact, fmtPct } from '../../lib/format'
import { colored, sunburstSeries, type SunNode } from '../../lib/sunburst'
import { modelOrder, modelPalette, providerColor } from '../../lib/summaryModel'
import DashCard from './DashCard.vue'

const props = defineProps<{
  byProvider: Record<string, ProviderAgg>
  byModel: Record<string, ModelAgg>
  totalCost: number
}>()
const t = useChartTheme()

const unpricedModels = computed(() => modelOrder(props.byModel).filter((m) => props.byModel[m]?.unpriced))

const data = computed<SunNode[]>(() => {
  const th = t.value
  const providers = Object.keys(props.byProvider).sort()
  const pal = modelPalette(props.byModel, th)
  // provider → model 的交叉表由 by_provider[*].by_model 给出；同一个模型在两个 provider 下各自成节点
  const out: SunNode[] = []
  for (const p of providers) {
    const byModel = props.byProvider[p]?.by_model ?? {}
    const kids = modelOrder(byModel)
      .filter((m) => (byModel[m]?.cost_usd ?? 0) > 0)
      .map((m) => {
        const agg = byModel[m]!
        return colored({ name: m, value: agg.cost_usd, meta: { steps: fmtInt(agg.steps) } }, pal.color(m), 0.92)
      })
    if (!kids.length) continue
    out.push(colored({ name: p, children: kids }, providerColor(p, th)))
  }
  return out
})

const option = computed<EChartsOption>(() => {
  const th = t.value
  const total = props.totalCost
  return {
    tooltip: {
      ...tooltipBase(th),
      formatter: (p: unknown) => {
        const q = p as { name: string; value: number; data: SunNode; treePathInfo: { name: string }[] }
        const path = q.treePathInfo
          .slice(1)
          .map((x) => x.name)
          .join(' › ')
        return (
          ttTitle(path || q.name) +
          ttRow(null, 'API 等价费用', fmtMoneyExact(q.value)) +
          ttRow(null, '占总费用', fmtPct(total > 0 ? q.value / total : null), true) +
          (q.data.meta?.steps ? ttRow(null, 'steps', q.data.meta.steps, true) : '')
        )
      },
    },
    series: [sunburstSeries(th, data.value)],
  }
})
</script>

<template>
  <DashCard n="04" title="费用 · provider › model" caption="内环 provider，外环模型">
    <div v-if="data.length === 0" class="state">没有可定价的步数，费用全为 0。</div>
    <EChart v-else :option="option" :height="300" />
    <template #footer>
      <template v-if="unpricedModels.length">
        未定价（计 $0，不入图）：<code>{{ unpricedModels.join(', ') }}</code>
      </template>
    </template>
  </DashCard>
</template>
