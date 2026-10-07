<script setup lang="ts">
import { computed } from 'vue'
import { useChartTheme } from '../../lib/chartTheme'
import { fmtInt, fmtMoneyExact } from '../../lib/format'
import { providerName } from '../../lib/providers'
import { modelLabel, type PricedModelAgg, type PricedProviderAgg } from '../../lib/pricing'
import { modelPalette, providerColor } from '../../lib/summaryModel'
import DashCard from './DashCard.vue'
import IcicleBar, { type IcicleGroup } from './IcicleBar.vue'

const props = defineProps<{
  byProvider: Record<string, PricedProviderAgg>
  byModel: Record<string, PricedModelAgg>
  totalCost: number
}>()
const t = useChartTheme()

/** 每个 provider 一组，组内按模型拆；组和组内都按费用降序 */
const groups = computed<IcicleGroup[]>(() => {
  const th = t.value
  const pal = modelPalette(props.byModel, th)
  return Object.entries(props.byProvider)
    .map(([p, agg]) => ({
      key: p,
      name: providerName(p),
      value: agg.cost_usd,
      cap: providerColor(p, th),
      segs: Object.entries(agg.by_model)
        .filter(([, a]) => a.cost_usd > 0)
        .sort(([, a], [, b]) => b.cost_usd - a.cost_usd)
        .map(([m, a]) => ({
          key: m,
          name: modelLabel(m),
          value: a.cost_usd,
          color: pal.color(m),
          hint: `${fmtInt(a.steps)} steps`,
          tip: `${providerName(p)} › ${modelLabel(m)}（${m}）
${fmtMoneyExact(a.cost_usd)} · ${fmtInt(a.steps)} steps`,
        })),
    }))
    .filter((g) => g.segs.length)
    .sort((a, b) => b.value - a.value)
})
</script>

<template>
  <DashCard icon="i-jannchie-coins" title="费用">
    <div v-if="groups.length === 0" class="py-8 text-center text-sm ink-soft">没有可定价的步数，费用全为 0。</div>
    <IcicleBar v-else label="总计" :amount="fmtMoneyExact(totalCost)" :groups="groups" :total="totalCost" :fmt="fmtMoneyExact" of-label="占总费用" />
  </DashCard>
</template>
