<script setup lang="ts">
import { computed } from 'vue'
import type { PricedModelAgg as ModelAgg } from '../../lib/pricing'
import { fmtBig, fmtInt, fmtMoneyExact, fmtMs, fmtPct } from '../../lib/format'
import { useChartTheme } from '../../lib/chartTheme'
import { cacheHitRate, modelOrder, modelPalette } from '../../lib/summaryModel'
import { useSort } from '../../lib/sort'
import DashCard from './DashCard.vue'
import SortTh from './SortTh.vue'

const props = defineProps<{ byModel: Record<string, ModelAgg>; totalCost: number }>()
const t = useChartTheme()

const pal = computed(() => modelPalette(props.byModel, t.value))
const rows = computed(() =>
  modelOrder(props.byModel).map((name) => {
    const m = props.byModel[name]!
    return {
      name,
      m,
      color: pal.value.slots.includes(name) ? pal.value.color(name) : null,
      hit: cacheHitRate(m.tokens),
      perStep: m.steps > 0 ? m.model_ms / m.steps : null,
      share: props.totalCost > 0 ? m.cost_usd / props.totalCost : null,
    }
  }),
)
const { sort, sorted } = useSort(
  rows,
  {
    name: (r) => r.name,
    steps: (r) => r.m.steps,
    input: (r) => r.m.tokens.input,
    output: (r) => r.m.tokens.output,
    cr: (r) => r.m.tokens.cache_read,
    cw: (r) => r.m.tokens.cache_write,
    hit: (r) => r.hit,
    cost: (r) => (r.m.unpriced ? null : r.m.cost_usd),
    perStep: (r) => r.perStep,
  },
  'cost',
)
const sum = computed(() => {
  const z = { steps: 0, input: 0, output: 0, cr: 0, cw: 0, cost: 0, ms: 0 }
  for (const r of rows.value) {
    z.steps += r.m.steps
    z.input += r.m.tokens.input
    z.output += r.m.tokens.output
    z.cr += r.m.tokens.cache_read
    z.cw += r.m.tokens.cache_write
    z.cost += r.m.cost_usd
    z.ms += r.m.model_ms
  }
  return z
})

// 数字格 / 合计行共用的几组类
const td = 'py-1.5 pl-4 text-right num'
const foot = 'border-t border-neutral-200 py-2 dark:border-neutral-800'
</script>

<template>
  <DashCard icon="i-carbon-chip" title="模型">
    <div v-if="rows.length === 0" class="py-8 text-center text-sm ink-soft">没有模型数据。</div>
    <div v-else class="-mx-1 overflow-x-auto px-1">
      <table class="w-full border-collapse whitespace-nowrap text-sm">
        <thead>
          <tr class="text-xs ink-soft">
            <SortTh :sort="sort" k="name" align="left">模型</SortTh>
            <SortTh :sort="sort" k="steps" class="pl-4">steps</SortTh>
            <SortTh :sort="sort" k="input" class="pl-4">输入</SortTh>
            <SortTh :sort="sort" k="output" class="pl-4">输出</SortTh>
            <SortTh :sort="sort" k="cr" class="pl-4">缓存读</SortTh>
            <SortTh :sort="sort" k="cw" class="pl-4">缓存写</SortTh>
            <SortTh :sort="sort" k="hit" class="pl-4" title="cache_read ÷ (input + cache_read + cache_write)">命中率</SortTh>
            <SortTh :sort="sort" k="cost" class="pl-4">费用</SortTh>
            <th class="pb-1.5 pl-4 text-right font-normal">占比</th>
            <SortTh :sort="sort" k="perStep" class="pl-4" title="model_ms ÷ steps">每步时间</SortTh>
          </tr>
        </thead>
        <tbody>
          <tr v-for="r in sorted" :key="r.name" class="surface-hover">
            <td class="rounded-l-lg py-1.5 pl-1 pr-4">
              <span
                class="mr-2 inline-block h-2 w-2 rounded-full align-middle"
                :class="r.color ? '' : 'ring-1 ring-inset ring-neutral-400'"
                :style="r.color ? { background: r.color } : undefined"
              ></span>
              <span class="font-mono text-xs">{{ r.name }}</span>
              <span
                v-if="r.m.unpriced"
                class="ml-2 rounded-full bg-red-500/10 px-1.5 py-0.5 text-xs ink-danger"
                title="价格目录（models.dev）里没有这个模型，费用计 0"
                >未定价</span
              >
            </td>
            <td :class="td">{{ fmtInt(r.m.steps) }}</td>
            <td :class="td" :title="fmtInt(r.m.tokens.input)">{{ fmtBig(r.m.tokens.input) }}</td>
            <td :class="td" :title="fmtInt(r.m.tokens.output)">{{ fmtBig(r.m.tokens.output) }}</td>
            <td :class="td" :title="fmtInt(r.m.tokens.cache_read)">{{ fmtBig(r.m.tokens.cache_read) }}</td>
            <td :class="td" :title="fmtInt(r.m.tokens.cache_write)">{{ fmtBig(r.m.tokens.cache_write) }}</td>
            <td :class="td">{{ fmtPct(r.hit) }}</td>
            <td :class="[td, r.m.unpriced ? 'ink-soft' : 'font-medium']">
              {{ r.m.unpriced ? '—' : fmtMoneyExact(r.m.cost_usd) }}
            </td>
            <td :class="[td, 'ink-soft']">{{ r.m.unpriced ? '' : fmtPct(r.share, 1) }}</td>
            <td :class="[td, 'rounded-r-lg pr-1']">{{ fmtMs(r.perStep) }}</td>
          </tr>
        </tbody>
        <tfoot>
          <tr class="font-medium">
            <td :class="foot" class="pl-1">合计</td>
            <td :class="[td, foot]">{{ fmtInt(sum.steps) }}</td>
            <td :class="[td, foot]">{{ fmtBig(sum.input) }}</td>
            <td :class="[td, foot]">{{ fmtBig(sum.output) }}</td>
            <td :class="[td, foot]">{{ fmtBig(sum.cr) }}</td>
            <td :class="[td, foot]">{{ fmtBig(sum.cw) }}</td>
            <td :class="[td, foot]">
              {{ fmtPct(sum.input + sum.cr + sum.cw > 0 ? sum.cr / (sum.input + sum.cr + sum.cw) : null) }}
            </td>
            <td :class="[td, foot]">{{ fmtMoneyExact(sum.cost) }}</td>
            <td :class="[td, foot]"></td>
            <td :class="[td, foot, 'pr-1']">{{ fmtMs(sum.steps > 0 ? sum.ms / sum.steps : null) }}</td>
          </tr>
        </tfoot>
      </table>
    </div>
  </DashCard>
</template>
