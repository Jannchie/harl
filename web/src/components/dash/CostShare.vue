<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
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
const theme = useChartTheme()
const { t } = useI18n()

/** 每个 provider 一组，组内按模型拆；组和组内都按费用降序 */
const groups = computed<IcicleGroup[]>(() => {
  const th = theme.value
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
          tip: `${providerName(p)} › ${t('dash.costShare.modelWithId', { name: modelLabel(m), id: m })}
${fmtMoneyExact(a.cost_usd)} · ${fmtInt(a.steps)} steps`,
        })),
    }))
    .filter((g) => g.segs.length)
    .sort((a, b) => b.value - a.value)
})
</script>

<template>
  <DashCard icon="i-jannchie-coins" :title="t('common.metric.cost')">
    <div v-if="groups.length === 0" class="py-8 text-center text-sm ink-soft">{{ t('dash.costShare.none') }}</div>
    <IcicleBar
      v-else
      :label="t('common.metric.total')"
      :amount="fmtMoneyExact(totalCost)"
      :groups="groups"
      :total="totalCost"
      :fmt="fmtMoneyExact"
      :of-label="t('dash.costShare.ofTotal')"
    />
  </DashCard>
</template>
