<script setup lang="ts">
import { computed } from 'vue'
import type { ModelAgg } from '../../api'
import { fmtBig, fmtInt, fmtMoneyExact, fmtMs, fmtPct } from '../../lib/format'
import { useChartTheme } from '../../lib/chartTheme'
import { cacheHitRate, modelOrder, modelPalette } from '../../lib/summaryModel'
import DashCard from './DashCard.vue'

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
</script>

<template>
  <DashCard n="10" title="模型" caption="按费用排序；色块是各图里该模型的颜色">
    <div v-if="rows.length === 0" class="state">没有模型数据。</div>
    <div v-else class="wrap">
      <table class="models">
        <thead>
          <tr>
            <th>model</th>
            <th class="num">steps</th>
            <th class="num">input</th>
            <th class="num">output</th>
            <th class="num">cache read</th>
            <th class="num">cache write</th>
            <th class="num" title="cache_read ÷ (input + cache_read + cache_write)">命中率</th>
            <th class="num">费用（API 等价）</th>
            <th class="num">占比</th>
            <th class="num" title="model_ms ÷ steps">每步模型时间</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="r in rows" :key="r.name">
            <td class="mono name">
              <span class="sw" :style="{ background: r.color ?? 'transparent', borderColor: r.color ?? 'var(--line)' }"></span>
              {{ r.name }}
              <span v-if="r.m.unpriced" class="badge badge-danger" title="定价表里没有这个模型，费用计 0">未定价</span>
            </td>
            <td class="num mono">{{ fmtInt(r.m.steps) }}</td>
            <td class="num mono" :title="fmtInt(r.m.tokens.input)">{{ fmtBig(r.m.tokens.input) }}</td>
            <td class="num mono" :title="fmtInt(r.m.tokens.output)">{{ fmtBig(r.m.tokens.output) }}</td>
            <td class="num mono" :title="fmtInt(r.m.tokens.cache_read)">{{ fmtBig(r.m.tokens.cache_read) }}</td>
            <td class="num mono" :title="fmtInt(r.m.tokens.cache_write)">{{ fmtBig(r.m.tokens.cache_write) }}</td>
            <td class="num mono">{{ fmtPct(r.hit) }}</td>
            <td class="num mono" :class="{ muted: r.m.unpriced }">{{ r.m.unpriced ? '—' : fmtMoneyExact(r.m.cost_usd) }}</td>
            <td class="num mono muted">{{ r.m.unpriced ? '' : fmtPct(r.share, 1) }}</td>
            <td class="num mono">{{ fmtMs(r.perStep) }}</td>
          </tr>
        </tbody>
        <tfoot>
          <tr>
            <td class="mono dim">合计</td>
            <td class="num mono">{{ fmtInt(sum.steps) }}</td>
            <td class="num mono">{{ fmtBig(sum.input) }}</td>
            <td class="num mono">{{ fmtBig(sum.output) }}</td>
            <td class="num mono">{{ fmtBig(sum.cr) }}</td>
            <td class="num mono">{{ fmtBig(sum.cw) }}</td>
            <td class="num mono">{{ fmtPct(sum.input + sum.cr + sum.cw > 0 ? sum.cr / (sum.input + sum.cr + sum.cw) : null) }}</td>
            <td class="num mono">{{ fmtMoneyExact(sum.cost) }}</td>
            <td class="num"></td>
            <td class="num mono">{{ fmtMs(sum.steps > 0 ? sum.ms / sum.steps : null) }}</td>
          </tr>
        </tfoot>
      </table>
    </div>
  </DashCard>
</template>

<style scoped>
.wrap {
  overflow-x: auto;
}
.models td {
  padding-top: 3px;
  padding-bottom: 3px;
  white-space: nowrap;
}
.models tfoot td {
  border-top: 1px solid var(--line);
  border-bottom: 0;
  font-weight: 600;
}
.name {
  min-width: 200px;
}
.sw {
  display: inline-block;
  width: 9px;
  height: 9px;
  border: 1px solid;
  margin-right: 7px;
  vertical-align: -1px;
}
.badge {
  margin-left: 6px;
}
</style>
