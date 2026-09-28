<script setup lang="ts">
import { computed } from 'vue'
import type { ReviewAgg } from '../../api'
import { fmtBig, fmtInt, fmtPct } from '../../lib/format'
import DashCard from './DashCard.vue'

const props = defineProps<{ review: ReviewAgg; toolCalls: number }>()

const ratio = (a: number, b: number) => (b > 0 ? a / b : 0)

const stats = computed(() => [
  {
    key: 'reviews',
    label: '审查次数',
    value: fmtBig(props.review.reviews),
    sub: `占工具调用 ${fmtPct(ratio(props.review.reviews, props.toolCalls))}`,
    title: `送给分类器审查的工具调用 ${fmtInt(props.review.reviews)} 次 / 全部工具调用 ${fmtInt(props.toolCalls)} 次`,
  },
  {
    key: 'blocked',
    label: '拦截率',
    value: fmtPct(ratio(props.review.blocked, props.review.reviews), 2),
    sub: `拦下 ${fmtInt(props.review.blocked)} 次`,
    title: '分类器判定为不安全、没有执行的调用 / 审查次数',
  },
  {
    key: 'unavailable',
    label: '无结论',
    value: fmtPct(ratio(props.review.unavailable, props.review.reviews), 2),
    sub: `${fmtInt(props.review.unavailable)} 次`,
    title: '分类器出错或超时、没给出结论的调用 / 审查次数；这些调用同样没有执行',
  },
])

const reasons = computed(() => {
  const rows = Object.entries(props.review.by_reason)
    .map(([reason, n]) => ({ reason: reason || '（未注明原因）', n }))
    .sort((a, b) => b.n - a.n || a.reason.localeCompare(b.reason))
  const max = Math.max(1, ...rows.map((x) => x.n))
  return rows.map((x) => ({ ...x, share: x.n / max }))
})
</script>

<template>
  <DashCard
    icon="i-carbon-security"
    title="auto 模式审查"
    title-hint="Claude Code auto 模式下送给服务端分类器判定的工具调用。Read / Grep 等本地直接放行的不算；权限规则和用户拒绝不是分类器的判定，也不算。分类器本身的 token 和耗时日志里没有记录。"
  >
    <div class="grid grid-cols-3 gap-3 max-sm:grid-cols-1">
      <div v-for="s in stats" :key="s.key" class="flex flex-col gap-0.5" :title="s.title">
        <div class="text-xs ink-soft">{{ s.label }}</div>
        <div class="text-xl font-semibold leading-tight num">{{ s.value }}</div>
        <div class="text-xs ink-muted num">{{ s.sub }}</div>
      </div>
    </div>
    <div v-if="reasons.length" class="mt-4">
      <div class="mb-1 text-xs ink-soft">拦截原因</div>
      <table class="w-full border-collapse text-sm">
        <tbody>
          <tr v-for="x in reasons" :key="x.reason" class="surface-hover">
            <td class="w-2/5 rounded-l-lg py-1 pl-1 pr-3">
              <span class="block truncate text-xs">{{ x.reason }}</span>
            </td>
            <td class="rounded-r-lg py-1 pr-1">
              <div class="flex items-center gap-2">
                <div class="meter-track h-1.5 flex-1">
                  <span class="meter-fill bg-red-500/70" :style="{ width: `${x.share * 100}%` }"></span>
                </div>
                <span class="w-10 text-right text-xs num">{{ fmtInt(x.n) }}</span>
              </div>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </DashCard>
</template>
