<script setup lang="ts">
import { computed } from 'vue'
import { useChartTheme } from '../../lib/chartTheme'
import { fmtInt, fmtMoneyExact } from '../../lib/format'
import type { PricedModelAgg, PricedProviderAgg } from '../../lib/pricing'
import { modelOrder, modelPalette } from '../../lib/summaryModel'
import DashCard from './DashCard.vue'
import ShareBar, { type ShareSeg } from './ShareBar.vue'

const props = defineProps<{
  byProvider: Record<string, PricedProviderAgg>
  byModel: Record<string, PricedModelAgg>
  totalCost: number
}>()
const t = useChartTheme()

const unpricedModels = computed(() => modelOrder(props.byModel).filter((m) => props.byModel[m]?.unpriced))

interface Row {
  provider: string
  cost: number
  segments: ShareSeg[]
}

/** 每个 provider 一行，行内按模型堆叠；多 provider 时共用总费用做分母，行长 = 该 provider 的占比 */
const rows = computed<Row[]>(() => {
  const pal = modelPalette(props.byModel, t.value)
  const out: Row[] = []
  for (const [p, agg] of Object.entries(props.byProvider).sort(([a], [b]) => a.localeCompare(b))) {
    const segments = modelOrder(agg.by_model)
      .filter((m) => (agg.by_model[m]?.cost_usd ?? 0) > 0)
      .map((m) => {
        const a = agg.by_model[m]!
        return {
          key: m,
          name: m,
          value: a.cost_usd,
          color: pal.color(m),
          hint: `${fmtInt(a.steps)} steps`,
          tip: `${p} › ${m}\n${fmtMoneyExact(a.cost_usd)} · ${fmtInt(a.steps)} steps`,
        }
      })
    if (segments.length) out.push({ provider: p, cost: agg.cost_usd, segments })
  }
  return out
})
</script>

<template>
  <DashCard icon="i-carbon-currency" title="费用" caption="每行一个 provider，行内按模型堆叠">
    <div v-if="rows.length === 0" class="py-8 text-center text-sm ink-soft">没有可定价的步数，费用全为 0。</div>
    <div v-else class="flex flex-col gap-5">
      <ShareBar
        v-for="r in rows"
        :key="r.provider"
        :label="r.provider"
        :amount="fmtMoneyExact(r.cost)"
        :segments="r.segments"
        :total="rows.length > 1 ? totalCost : undefined"
        :fmt="fmtMoneyExact"
      />
    </div>
    <template #footer>
      <template v-if="unpricedModels.length">
        未定价（计 $0，不入图）：{{ unpricedModels.join(', ') }}
      </template>
    </template>
  </DashCard>
</template>
