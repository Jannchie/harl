<script setup lang="ts">
// 薄封装：init / setOption / resize / dispose，主题色由调用方在 option 里给（见 lib/chartTheme.ts）。
import { onBeforeUnmount, onMounted, ref, shallowRef, watch } from 'vue'
import { echarts, type ECharts, type EChartsOption } from '../lib/echarts'

const props = withDefaults(
  defineProps<{
    option: EChartsOption
    /** 不合并，整个 option 替换（切换指标时用，避免旧 series 残留） */
    replace?: boolean
    height?: number | string
  }>(),
  { replace: true, height: 280 },
)

const emit = defineEmits<{
  click: [params: { data?: unknown; name?: string; seriesType?: string; treePathInfo?: unknown }]
  ready: [chart: ECharts]
}>()

const el = ref<HTMLDivElement | null>(null)
const chart = shallowRef<ECharts | null>(null)
let ro: ResizeObserver | null = null

function apply() {
  const c = chart.value
  if (!c) return
  // 不要入场动画（首屏一堆图同时长出来很吵），只保留切换指标时的短过渡
  c.setOption({ animationDuration: 0, animationDurationUpdate: 200, ...props.option }, { notMerge: props.replace, lazyUpdate: true })
}

onMounted(() => {
  if (!el.value) return
  const c = echarts.init(el.value, undefined, { renderer: 'canvas' })
  chart.value = c
  c.on('click', (p) => emit('click', p as never))
  apply()
  ro = new ResizeObserver(() => c.resize())
  ro.observe(el.value)
  emit('ready', c)
})

watch(() => props.option, apply, { deep: false })

onBeforeUnmount(() => {
  ro?.disconnect()
  chart.value?.dispose()
  chart.value = null
})

defineExpose({ chart })
</script>

<template>
  <div ref="el" class="w-full min-w-0" :style="{ height: typeof height === 'number' ? `${height}px` : height }"></div>
</template>
