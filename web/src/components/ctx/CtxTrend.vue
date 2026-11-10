<script setup lang="ts">
// 上下文趋势：每次请求（或每轮的最后一次请求）一根堆叠柱，按类别拆；变化口径画相对上一根的增减。
// ✂ 标出压缩发生在哪根柱子之前。悬停预览那一步（简报 + 通知父组件 preview），点击把它在浏览器里打开。
// 另外三种看法：
// - 总览：整段会话压进卡片宽度，画成连续的类别带（canvas，按像素列聚合，上千次请求也不卡）；
//   悬停高亮一条类别带，点击固定并通知父组件 focusCat
// - 自适应：柱高按可见范围重新缩放（跟着滑块走）
// - DNA：每根柱子拆成逐项色带（system → 工具 schema → 消息，按模型读取的顺序），
//   变化口径画逐项增减；悬停高亮同一项在各柱里的存续，点击通知父组件 locate（canvas）
// 总览与 DNA / 自适应互斥：总览打开时后两者不生效。
import { computed, onBeforeUnmount, ref, shallowRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { axisBase, esc, tooltipBase, ttRow, ttTitle, useChartTheme } from '../../lib/chartTheme'
import { ALL_CATS, briefOf, sumParts, turnSteps, zeroParts, type Cat, type CtxFold, type CtxNode, type Parts, type StepRec } from '../../lib/context'
import { catColor, catLabel, usePref } from '../../lib/ctxView'
import type { ECharts, EChartsOption } from '../../lib/echarts'
import { fmtBig, fmtInt, fmtMs, fmtSigned } from '../../lib/format'
import DashCard from '../dash/DashCard.vue'
import EChart from '../EChart.vue'
import Segmented from '../Segmented.vue'

const props = defineProps<{ fold: CtxFold; selected: number }>()
const emit = defineEmits<{
  select: [idx: number]
  focus: [node: CtxNode]
  /** 悬停到某根柱子：预览那次请求（step 下标；按轮时是那一轮的最后一次请求）；移开发 null，由父组件恢复原来的选中 */
  preview: [idx: number | null]
  /** 总览里点击固定了一条类别带：父组件在浏览器里展开这一类 */
  focusCat: [cat: Cat]
  /** DNA 里点了一项：节点 key + 所在的请求（step 下标），父组件在浏览器里打开那次请求并定位到这一项 */
  locate: [key: string, idx: number]
}>()
const { t } = useI18n()
const theme = useChartTheme()

const gran = usePref<'step' | 'turn'>('trend.gran', 'step')
const mode = usePref<'total' | 'delta'>('trend.mode', 'total')
const curve = usePref<boolean>('trend.curve', true)
/** 柱高按可见范围缩放 */
const adaptive = usePref<boolean>('trend.adaptive', false)
/** 柱子画成逐项色带 */
const dna = usePref<boolean>('trend.dna', false)
/** 请求数超过多少时直接进总览 */
const autoOverview = usePref<'100' | '200' | '300' | 'off'>('trend.overviewAuto', '200')
/** 总览不记偏好：每次打开按阈值决定，用户可以随时切回 */
const overview = ref(false)

const view = computed<'bars' | 'overview' | 'dna'>(() => (overview.value ? 'overview' : dna.value ? 'dna' : 'bars'))

// 首次拿到请求时按阈值决定是否进总览；阈值改了重新决定一次
let decided = false
watch(
  () => [props.fold.steps.length, autoOverview.value] as const,
  ([len, th], old) => {
    if (old && old[1] !== th) decided = false
    if (decided || len === 0) return
    decided = true
    overview.value = th !== 'off' && len > Number(th)
  },
  { immediate: true },
)

interface Row {
  /** 代表这根柱子的请求 */
  step: StepRec
  label: string
  title: string
  parts: Parts
  ms: number
}

/** 一次请求的活跃时长：fold 有 activeMs（去掉等用户决定 / 回答的时间）就用它，否则模型 + 工具 */
function activeOf(s: StepRec): number {
  const a = (s as StepRec & { activeMs?: number }).activeMs
  return typeof a === 'number' ? a : s.modelMs + s.toolMs
}

const rows = computed<Row[]>(() => {
  if (gran.value === 'turn')
    return turnSteps(props.fold).map((x, i) => {
      let ms = 0
      for (let k = x.turn.first; k <= x.turn.last; k++) {
        const s = props.fold.steps[k]
        if (s) ms += activeOf(s)
      }
      return {
        step: x.step,
        label: String(x.turn.n ?? i + 1),
        title: `${t('ctx.trend.turnN', { n: x.turn.n ?? i + 1 })} · ${t('ctx.trend.turnSteps', { n: x.count }, x.count)}`,
        parts: x.step.anchored,
        ms,
      }
    })
  return props.fold.steps.map((s) => ({
    step: s,
    label: String(s.idx + 1),
    title: `${t('ctx.trend.stepN', { n: s.idx + 1 })}${s.turnN != null ? ` · ${t('ctx.trend.turnN', { n: s.turnN })}` : ''}`,
    parts: s.anchored,
    ms: activeOf(s),
  }))
})

const cats = computed(() => ALL_CATS.filter((c) => rows.value.some((r) => r.parts[c] !== 0)))

/** 每类每根柱子的值：累计口径是校准值，变化口径是相对上一根的差 */
const values = computed(() => {
  const out = {} as Record<Cat, number[]>
  for (const c of cats.value)
    out[c] = rows.value.map((r, i) => {
      if (mode.value === 'total') return r.parts[c]
      const prev = rows.value[i - 1]
      return r.parts[c] - (prev ? prev.parts[c] : 0)
    })
  return out
})

/** 每根柱子正向、负向各自的堆叠高度（y 轴范围用） */
const extents = computed(() => {
  const n = rows.value.length
  const up = new Float64Array(n)
  const down = new Float64Array(n)
  for (const c of cats.value) {
    const v = values.value[c]!
    for (let i = 0; i < n; i++) {
      const x = v[i]!
      if (x > 0) up[i] = up[i]! + x
      else down[i] = down[i]! + x
    }
  }
  return { up, down }
})

/** 取整到 1 / 2 / 2.5 / 5 × 10ⁿ，刻度好读 */
function nice(v: number): number {
  if (v <= 0) return 0
  const p = 10 ** Math.floor(Math.log10(v))
  const m = v / p
  return (m <= 1 ? 1 : m <= 2 ? 2 : m <= 2.5 ? 2.5 : m <= 5 ? 5 : 10) * p
}

interface Scale {
  min: number
  max: number
}

function scaleOf(hi: number, lo: number): Scale {
  return { min: lo < 0 ? -nice(-lo) : 0, max: hi > 0 ? nice(hi) : 1 }
}

/** 第 [a, b) 根柱子的 y 范围 */
function yRange(a: number, b: number): Scale {
  const { up, down } = extents.value
  let hi = 0
  let lo = 0
  for (let i = Math.max(0, a); i < Math.min(b, up.length); i++) {
    if (up[i]! > hi) hi = up[i]!
    if (down[i]! < lo) lo = down[i]!
  }
  return scaleOf(hi, lo)
}

/** 压缩落在哪根柱子上：事件之后的第一次请求 */
const markers = computed(() => {
  const out = new Set<number>()
  for (const e of props.fold.events) {
    if (e.kind !== 'compaction' && e.kind !== 'prune') continue
    const i = rows.value.findIndex((r) => r.step.idx >= e.step)
    if (i >= 0) out.add(i)
  }
  return [...out]
})

const selectedRow = computed(() => rows.value.findIndex((r) => (gran.value === 'turn' ? r.step.turnId === props.fold.steps[props.selected]?.turnId : r.step.idx === props.selected)))

// ---- 可见窗口：柱状图的滑块和 DNA 的拖条共用 ----

const MAX_BARS = 160
/** 非响应式：拖滑块时改它不能触发 option 重建（重建会打断拖动）；DNA 靠 winTick 重画 */
const win = { start: 0, end: 0 }
const winTick = ref(0)
function setWin(start: number, end: number) {
  win.start = start
  win.end = end
  winTick.value++
}

watch(
  () => [rows.value.length, gran.value] as const,
  ([n, g], old) => {
    if (!old || old[1] !== g || win.end <= win.start) return setWin(Math.max(0, n - MAX_BARS), n)
    // 原来就停在末尾：跟着新请求往后走；否则原地不动
    if (win.end >= old[0]) {
      const size = win.end - win.start
      return setWin(Math.max(0, n - size), n)
    }
    setWin(Math.min(win.start, n), Math.min(win.end, n))
  },
  { immediate: true },
)

// ---- 柱状图（ECharts） ----

const option = computed<EChartsOption>(() => {
  const th = theme.value
  const ax = axisBase(th)
  const n = rows.value.length
  const zoom = n > MAX_BARS
  // 不自适应时固定全局范围，拖滑块柱高不跳；自适应时只看可见窗口
  const y = adaptive.value && zoom ? yRange(win.start, win.end) : yRange(0, n)
  const marks = [
    ...markers.value.map((i) => ({ xAxis: i, label: { formatter: '✂', color: th.danger }, lineStyle: { color: th.danger, type: 'dashed' as const } })),
    ...(selectedRow.value >= 0
      ? [{ xAxis: selectedRow.value, label: { show: false }, lineStyle: { color: th.accent, type: 'solid' as const, width: 2, opacity: 0.6 } }]
      : []),
  ]
  return {
    grid: { left: 8, right: curve.value ? 8 : 12, top: 18, bottom: zoom ? 48 : 8, containLabel: true },
    tooltip: {
      ...tooltipBase(th),
      trigger: 'axis',
      axisPointer: { type: 'shadow' },
      formatter: (ps: unknown) => {
        const list = ps as { dataIndex: number }[]
        const i = list[0]?.dataIndex ?? 0
        return tooltipHtml(i)
      },
    },
    dataZoom: zoom
      ? [
          // 只用滑块：inside 型会吃掉页面的滚轮
          {
            type: 'slider',
            height: 18,
            bottom: 8,
            startValue: win.start,
            endValue: Math.max(win.start, win.end - 1),
            borderColor: th.line,
            textStyle: { color: th.fg2 },
          },
        ]
      : [],
    xAxis: { type: 'category', data: rows.value.map((r) => r.label), ...ax, splitLine: { show: false } },
    yAxis: [
      { type: 'value', min: y.min, max: y.max, ...ax, axisLabel: { ...ax.axisLabel, formatter: (v: number) => fmtBig(v) } },
      { type: 'value', show: curve.value, ...ax, splitLine: { show: false }, axisLabel: { ...ax.axisLabel, formatter: (v: number) => fmtMs(v) } },
    ],
    series: [
      ...cats.value.map((c, k) => ({
        type: 'bar' as const,
        name: catLabel(c),
        stack: 'ctx',
        data: values.value[c],
        itemStyle: { color: catColor(c, th) },
        barMaxWidth: 22,
        barCategoryGap: '18%',
        emphasis: { focus: 'series' as const },
        markLine: k === 0 && marks.length ? { symbol: ['none', 'none'], silent: true, animation: false, data: marks } : undefined,
      })),
      ...(curve.value
        ? [
            {
              type: 'line' as const,
              name: t('ctx.trend.duration'),
              yAxisIndex: 1,
              data: rows.value.map((r) => r.ms),
              symbol: 'none',
              smooth: true,
              lineStyle: { color: th.fg2, width: 1, opacity: 0.7 },
              itemStyle: { color: th.fg2 },
              z: 5,
            },
          ]
        : []),
    ],
  }
})

function tooltipHtml(i: number): string {
  const th = theme.value
  const r = rows.value[i]
  if (!r) return ''
  let html = ttTitle(r.title)
  let net = 0
  let churn = 0
  for (const c of cats.value) {
    const v = values.value[c]![i]!
    if (mode.value === 'delta') {
      net += v
      churn += Math.abs(v)
    }
    if (v !== 0) html += ttRow(catColor(c, th), catLabel(c), mode.value === 'total' ? fmtInt(v) : fmtSigned(v))
  }
  html += '<div style="height:4px"></div>'
  if (mode.value === 'delta') {
    html += ttRow(null, t('ctx.trend.net'), fmtSigned(net))
    html += ttRow(null, t('ctx.trend.churn'), fmtInt(churn), true)
  } else html += ttRow(null, t('ctx.trend.estimate'), fmtInt(sumParts(r.parts)), true)
  const u = r.step.usage
  if (u) {
    html += ttRow(null, t('ctx.trend.prompt'), fmtInt(r.step.prompt ?? 0))
    html += ttRow(null, t('ctx.trend.cacheRead'), fmtInt(u.cache_read), true)
    html += ttRow(null, t('ctx.trend.output'), fmtInt(u.output), true)
  }
  if (r.ms) html += ttRow(null, t('ctx.trend.duration'), fmtMs(r.ms), true)
  if (r.step.model) html += `<div style="margin-top:4px;color:${th.fg2}">${esc(r.step.model)}</div>`
  return html
}

/** 悬停中的请求（step 下标）：简报跟着它走，也通过 preview 告诉父组件 */
const hovered = shallowRef<number | null>(null)
watch(hovered, (v) => emit('preview', v))

function onReady(c: ECharts) {
  c.on('updateAxisPointer', (e: unknown) => {
    const info = (e as { axesInfo?: { value?: number }[] }).axesInfo?.[0]
    const i = typeof info?.value === 'number' ? info.value : null
    hovered.value = i != null ? (rows.value[i]?.step.idx ?? null) : null
  })
  c.on('globalout', () => {
    hovered.value = null
  })
  c.on('datazoom', () => {
    const dz = (c.getOption() as { dataZoom?: { startValue?: number; endValue?: number }[] }).dataZoom?.[0]
    if (typeof dz?.startValue !== 'number' || typeof dz.endValue !== 'number') return
    setWin(dz.startValue, dz.endValue + 1)
    // 直接合并进图表，不经过 option（重建会打断拖动）
    if (adaptive.value) {
      const y = yRange(win.start, win.end)
      c.setOption({ yAxis: [{ min: y.min, max: y.max }, {}] }, { lazyUpdate: true })
    }
  })
  c.getZr().on('click', (ev: { offsetX: number; offsetY: number }) => {
    const p = [ev.offsetX, ev.offsetY]
    if (!c.containPixel('grid', p)) return
    // 类目轴按 xAxisIndex 换算会得到 null，按第一个系列换算拿 [类目序号, 值]
    const x = c.convertFromPixel({ seriesIndex: 0 }, p) as number[] | null
    const r = x ? rows.value[Math.round(x[0]!)] : undefined
    // 按轮时 r.step 已经是这一轮的最后一次请求
    if (r) emit('select', r.step.idx)
  })
}

// ---- canvas：总览与 DNA ----

const H = 260
const PAD = { l: 44, r: 10, t: 14, b: 24 }
const plotH = H - PAD.t - PAD.b

const wrap = ref<HTMLDivElement | null>(null)
const cvs = ref<HTMLCanvasElement | null>(null)
const cw = ref(0)
watch(wrap, (el, _old, onCleanup) => {
  if (!el) return
  const ro = new ResizeObserver(([e]) => {
    if (e) cw.value = e.contentRect.width
  })
  ro.observe(el)
  onCleanup(() => ro.disconnect())
})
const plotW = computed(() => Math.max(1, cw.value - PAD.l - PAD.r))

/** 悬停的那根柱子（rows 下标）、类别带（总览）、项（DNA） */
const hoverRow = shallowRef<number | null>(null)
const hoverCat = shallowRef<Cat | null>(null)
const hoverKey = shallowRef<string | null>(null)
/** 总览里点击固定的类别 */
const pinnedCat = shallowRef<Cat | null>(null)
const tip = shallowRef<{ x: number; y: number; html: string } | null>(null)

/** 最近一次绘制用的 y 范围：命中测试要用同一套换算 */
let lastScale: Scale = { min: 0, max: 1 }
const yOf = (v: number) => PAD.t + ((lastScale.max - v) / (lastScale.max - lastScale.min)) * plotH

// 总览：按像素列聚合，每列取所含请求的均值
interface Bucket {
  r0: number
  r1: number
  /** 按 cats 顺序的均值 */
  v: Float64Array
  ms: number
}

const buckets = computed<Bucket[]>(() => {
  const n = rows.value.length
  if (view.value !== 'overview' || !n) return []
  const B = Math.max(1, Math.min(n, Math.floor(plotW.value)))
  const cs = cats.value
  const vals = cs.map((c) => values.value[c]!)
  const out: Bucket[] = []
  for (let b = 0; b < B; b++) {
    const r0 = Math.floor((b * n) / B)
    const r1 = Math.max(r0 + 1, Math.floor(((b + 1) * n) / B))
    const v = new Float64Array(cs.length)
    let ms = 0
    for (let r = r0; r < r1; r++) {
      for (let k = 0; k < cs.length; k++) v[k] = v[k]! + vals[k]![r]!
      ms += rows.value[r]!.ms
    }
    const m = r1 - r0
    for (let k = 0; k < cs.length; k++) v[k] = v[k]! / m
    out.push({ r0, r1, v, ms: ms / m })
  }
  return out
})

// DNA：一根柱子的逐项色带。数据全部来自现有 fold（与 assemble() 同一套规则）：
// system = seq 之前最后一份 system，工具 = seq 之前最后一个工具 epoch，消息 = seq 之前进入且没被移出的节点。
// 不需要 fold 额外提供逐步的项列表。
const OTHER_KEY = '__unlogged'

interface Strip {
  /** null：未记录的部分（校准后多出来的 other） */
  node: CtxNode | null
  cat: Cat
  /** token（累计口径按类校准到柱高；变化口径用估算） */
  h: number
  kind?: 'entered' | 'left' | 'grew' | 'shrank'
}

interface DnaCol {
  up: Strip[]
  down: Strip[]
}

function nodeBound(seq: number): number {
  const nodes = props.fold.nodes
  let lo = 0
  let hi = nodes.length
  while (lo < hi) {
    const mid = (lo + hi) >> 1
    if (nodes[mid]!.seq < seq) lo = mid + 1
    else hi = mid
  }
  return lo
}

/** 第 r 根柱子那次请求带的所有项，按模型读取的顺序 */
function itemsAt(r: number): CtxNode[] {
  const row = rows.value[r]
  if (!row) return []
  const f = props.fold
  const at = row.step.seq
  const out: CtxNode[] = []
  let sys: CtxNode | null = null
  for (const s of f.systems) if (s.seq < at) sys = s
  if (sys) out.push(sys)
  let tools: CtxNode[] = []
  for (const e of f.toolEpochs) if (e.seq < at) tools = e.tools
  out.push(...tools)
  const end = nodeBound(at)
  for (let i = 0; i < end; i++) {
    const n = f.nodes[i]!
    if (n.gone != null && n.gone <= at) continue
    out.push(n)
  }
  return out
}

function totalCol(r: number): DnaCol {
  const row = rows.value[r]!
  const items = itemsAt(r)
  const sum = zeroParts()
  for (const n of items) sum[n.cat] += n.est
  // 每类按校准值缩放，色带总高和柱状图一致
  const k = zeroParts()
  for (const c of ALL_CATS) k[c] = c !== 'other' && sum[c] > 0 && row.parts[c] > 0 ? row.parts[c] / sum[c] : 1
  const up: Strip[] = items.map((n) => ({ node: n, cat: n.cat, h: n.est * k[n.cat] }))
  const extra = row.parts.other - sum.other
  if (extra > 0) up.push({ node: null, cat: 'other', h: extra })
  return { up, down: [] }
}

function deltaCol(r: number): DnaCol {
  const cur = itemsAt(r)
  const prev = r > 0 ? itemsAt(r - 1) : []
  const pm = new Map(prev.map((n) => [n.key, n]))
  const cm = new Map(cur.map((n) => [n.key, n]))
  const up: Strip[] = []
  const down: Strip[] = []
  for (const n of cur) {
    const p = pm.get(n.key)
    if (!p) up.push({ node: n, cat: n.cat, h: n.est, kind: 'entered' })
    else if (n.est > p.est) up.push({ node: n, cat: n.cat, h: n.est - p.est, kind: 'grew' })
    else if (n.est < p.est) down.push({ node: n, cat: n.cat, h: p.est - n.est, kind: 'shrank' })
  }
  for (const p of prev) if (!cm.has(p.key)) down.push({ node: p, cat: p.cat, h: p.est, kind: 'left' })
  const d = (rows.value[r]?.parts.other ?? 0) - (rows.value[r - 1]?.parts.other ?? 0)
  if (d > 0) up.push({ node: null, cat: 'other', h: d, kind: 'grew' })
  else if (d < 0) down.push({ node: null, cat: 'other', h: -d, kind: 'shrank' })
  return { up, down }
}

/** 列缓存：悬停重画只读缓存；数据或口径变了整个清掉，太大也清 */
const colCache = new Map<number, DnaCol>()
watch([() => props.fold, rows, mode], () => colCache.clear())
function colAt(r: number): DnaCol {
  let c = colCache.get(r)
  if (!c) {
    if (colCache.size > 600) colCache.clear()
    c = mode.value === 'delta' ? deltaCol(r) : totalCol(r)
    colCache.set(r, c)
  }
  return c
}

/** DNA 的可见窗口：沿用共享窗口，柱子至少 3px 宽 */
const dnaWin = computed(() => {
  void winTick.value
  const n = rows.value.length
  const maxCols = Math.max(1, Math.floor(plotW.value / 3))
  const want = win.end > win.start ? win.end - win.start : MAX_BARS
  const size = Math.min(n, Math.max(1, Math.min(want, maxCols)))
  const start = Math.max(0, Math.min(win.start, n - size))
  return { start, size }
})

function onScrub(e: Event) {
  const v = Number((e.target as HTMLInputElement).value)
  setWin(v, v + dnaWin.value.size)
}

let raf = 0
function schedule() {
  if (raf) return
  raf = requestAnimationFrame(() => {
    raf = 0
    draw()
  })
}
onBeforeUnmount(() => cancelAnimationFrame(raf))

watch(
  [view, cw, cvs, rows, values, buckets, theme, hoverCat, pinnedCat, hoverKey, hoverRow, winTick, adaptive, mode, curve, selectedRow, markers],
  schedule,
  { flush: 'post' },
)

function draw() {
  const el = cvs.value
  const W = cw.value
  if (!el || W <= 0) return
  const dpr = window.devicePixelRatio || 1
  const pw = Math.round(W * dpr)
  const ph = Math.round(H * dpr)
  if (el.width !== pw || el.height !== ph) {
    el.width = pw
    el.height = ph
  }
  const g = el.getContext('2d')
  if (!g) return
  g.setTransform(dpr, 0, 0, dpr, 0, 0)
  g.clearRect(0, 0, W, H)
  g.font = `11px ${theme.value.fontFamily}`
  if (view.value === 'overview') drawOverview(g)
  else if (view.value === 'dna') drawDna(g)
}

function drawYAxis(g: CanvasRenderingContext2D) {
  const th = theme.value
  const { min, max } = lastScale
  const ticks = min < 0 ? [min, 0, max] : [0, max / 2, max]
  g.textAlign = 'right'
  g.textBaseline = 'middle'
  g.lineWidth = 1
  for (const v of ticks) {
    const y = Math.round(yOf(v)) + 0.5
    g.strokeStyle = th.grid
    g.beginPath()
    g.moveTo(PAD.l, y)
    g.lineTo(PAD.l + plotW.value, y)
    g.stroke()
    g.fillStyle = th.fg2
    g.fillText(fmtBig(v), PAD.l - 6, y)
  }
}

function vline(g: CanvasRenderingContext2D, x: number, color: string, opts: { dash?: boolean; width?: number; alpha?: number; label?: string } = {}) {
  g.save()
  g.strokeStyle = color
  g.globalAlpha = opts.alpha ?? 1
  g.lineWidth = opts.width ?? 1
  if (opts.dash) g.setLineDash([4, 3])
  g.beginPath()
  g.moveTo(x, PAD.t)
  g.lineTo(x, PAD.t + plotH)
  g.stroke()
  if (opts.label) {
    g.fillStyle = color
    g.textAlign = 'center'
    g.textBaseline = 'bottom'
    g.fillText(opts.label, x, PAD.t - 1)
  }
  g.restore()
}

/** 耗时曲线：canvas 里不画右轴，只看起伏（具体值在 tooltip） */
function drawCurve(g: CanvasRenderingContext2D, pts: { x: number; ms: number }[]) {
  let mx = 0
  for (const p of pts) if (p.ms > mx) mx = p.ms
  if (mx <= 0) return
  g.save()
  g.strokeStyle = theme.value.fg2
  g.globalAlpha = 0.7
  g.lineWidth = 1
  g.beginPath()
  pts.forEach((p, i) => {
    const y = PAD.t + plotH - (p.ms / mx) * plotH * 0.92
    if (i === 0) g.moveTo(p.x, y)
    else g.lineTo(p.x, y)
  })
  g.stroke()
  g.restore()
}

function drawOverview(g: CanvasRenderingContext2D) {
  const th = theme.value
  const B = buckets.value
  const n = rows.value.length
  if (!B.length || !n) return
  const cs = cats.value
  let hi = 0
  let lo = 0
  for (const b of B) {
    let u = 0
    let d = 0
    for (const x of b.v) {
      if (x > 0) u += x
      else d += x
    }
    if (u > hi) hi = u
    if (d < lo) lo = d
  }
  lastScale = scaleOf(hi, lo)
  drawYAxis(g)
  const pw = plotW.value
  const bw = pw / B.length
  const active = hoverCat.value ?? pinnedCat.value
  for (let i = 0; i < B.length; i++) {
    const b = B[i]!
    // 多画 0.6px 盖住相邻列之间的缝，看起来是连续的带
    const x0 = PAD.l + i * bw
    const w = bw + 0.6
    let up = 0
    let dn = 0
    for (let k = 0; k < cs.length; k++) {
      const v = b.v[k]!
      if (!v) continue
      const c = cs[k]!
      g.globalAlpha = active == null || active === c ? 1 : 0.18
      g.fillStyle = catColor(c, th)
      const a = v > 0 ? up : dn
      const y0 = yOf(a)
      const y1 = yOf(a + v)
      g.fillRect(x0, Math.min(y0, y1), w, Math.abs(y1 - y0))
      if (v > 0) up += v
      else dn += v
    }
  }
  g.globalAlpha = 1
  if (curve.value) drawCurve(g, B.map((b, i) => ({ x: PAD.l + (i + 0.5) * bw, ms: b.ms })))
  const xOf = (r: number) => PAD.l + (r / n) * pw
  for (const r of markers.value) vline(g, Math.round(xOf(r)) + 0.5, th.danger, { dash: true, label: '✂' })
  if (selectedRow.value >= 0) vline(g, xOf(selectedRow.value + 0.5), th.accent, { width: 2, alpha: 0.6 })
  if (hoverRow.value != null) vline(g, xOf(hoverRow.value + 0.5), th.fg, { alpha: 0.5 })
  // 四分位旗标：首、¼、½、¾、尾
  g.textBaseline = 'top'
  const qs = [0, 0.25, 0.5, 0.75, 1]
  for (const q of qs) {
    const r = Math.min(n - 1, Math.round(q * (n - 1)))
    const x = Math.round(xOf(r + 0.5)) + 0.5
    g.strokeStyle = th.fg2
    g.globalAlpha = 0.6
    g.beginPath()
    g.moveTo(x, PAD.t + plotH)
    g.lineTo(x, PAD.t + plotH + 5)
    g.stroke()
    g.globalAlpha = 1
    g.fillStyle = th.fg2
    g.textAlign = q === 0 ? 'left' : q === 1 ? 'right' : 'center'
    g.fillText(`#${rows.value[r]!.label}`, q === 0 ? x - 2 : q === 1 ? x + 2 : x, PAD.t + plotH + 7)
  }
}

interface HlRect {
  x: number
  w: number
  a: number
  b: number
  color: string
}

/** 从 y0 起沿 dir（-1 向上 / +1 向下）一条条画色带；不足 3px 的同类项并成一段，省 fillRect */
function drawStrips(g: CanvasRenderingContext2D, list: Strip[], x: number, w: number, y0: number, dir: 1 | -1, ppt: number, hk: string | null, hl: HlRect[]) {
  const th = theme.value
  let y = y0
  let runCat: Cat | null = null
  let runY = y0
  const flush = (yEnd: number) => {
    if (runCat == null) return
    g.fillStyle = catColor(runCat, th)
    g.fillRect(x, Math.min(runY, yEnd), w, Math.abs(yEnd - runY))
    runCat = null
  }
  for (const s of list) {
    const h = s.h * ppt
    if (h <= 0) continue
    const yn = y + dir * h
    const color = catColor(s.cat, th)
    if (h >= 3) {
      flush(y)
      g.fillStyle = color
      // 远端留一道缝，相邻项分得开
      const far = yn - dir * 0.75
      g.fillRect(x, Math.min(y, far), w, Math.abs(far - y))
    } else if (runCat !== s.cat) {
      flush(y)
      runCat = s.cat
      runY = y
    }
    if ((s.node?.key ?? OTHER_KEY) === hk) hl.push({ x, w, a: y, b: yn, color })
    y = yn
  }
  flush(y)
}

function dnaGeom() {
  const { start, size } = dnaWin.value
  const slot = plotW.value / Math.max(1, size)
  const bw = Math.max(1, Math.min(22, slot * 0.82))
  return { start, size, slot, bw }
}

function drawDna(g: CanvasRenderingContext2D) {
  const th = theme.value
  const n = rows.value.length
  const { start, size, slot, bw } = dnaGeom()
  if (!n || !size) return
  const cols: DnaCol[] = []
  for (let i = 0; i < size; i++) cols.push(colAt(start + i))
  if (mode.value === 'total') lastScale = adaptive.value ? yRange(start, start + size) : yRange(0, n)
  else {
    let hi = 0
    let lo = 0
    for (const c of cols) {
      let u = 0
      let d = 0
      for (const s of c.up) u += s.h
      for (const s of c.down) d += s.h
      if (u > hi) hi = u
      if (-d < lo) lo = -d
    }
    if (!adaptive.value) {
      const gr = yRange(0, n)
      hi = Math.max(hi, gr.max)
      lo = Math.min(lo, gr.min)
    }
    lastScale = scaleOf(hi, lo)
  }
  drawYAxis(g)
  const ppt = plotH / (lastScale.max - lastScale.min)
  const y0 = yOf(0)
  const hk = hoverKey.value
  const hl: HlRect[] = []
  g.globalAlpha = hk ? 0.35 : 1
  for (let i = 0; i < size; i++) {
    const x = PAD.l + i * slot + (slot - bw) / 2
    drawStrips(g, cols[i]!.up, x, bw, y0, -1, ppt, hk, hl)
    drawStrips(g, cols[i]!.down, x, bw, y0, 1, ppt, hk, hl)
  }
  g.globalAlpha = 1
  // 同一项在各根柱子里的存续：全亮 + 描边
  for (const r of hl) {
    const top = Math.min(r.a, r.b)
    const hgt = Math.max(1.5, Math.abs(r.b - r.a))
    g.fillStyle = r.color
    g.fillRect(r.x, top, r.w, hgt)
    g.strokeStyle = th.fg
    g.lineWidth = 1
    g.strokeRect(r.x - 0.5, top - 0.5, r.w + 1, hgt + 1)
  }
  if (mode.value === 'delta') {
    g.strokeStyle = th.axis
    g.beginPath()
    g.moveTo(PAD.l, Math.round(y0) + 0.5)
    g.lineTo(PAD.l + plotW.value, Math.round(y0) + 0.5)
    g.stroke()
  }
  const xOf = (r: number) => PAD.l + (r - start) * slot
  if (curve.value) {
    const pts: { x: number; ms: number }[] = []
    for (let i = 0; i < size; i++) pts.push({ x: xOf(start + i + 0.5), ms: rows.value[start + i]!.ms })
    drawCurve(g, pts)
  }
  for (const r of markers.value) if (r >= start && r < start + size) vline(g, Math.round(xOf(r)) + 0.5, th.danger, { dash: true, label: '✂' })
  const sr = selectedRow.value
  if (sr >= start && sr < start + size) vline(g, xOf(sr + 0.5), th.accent, { width: 2, alpha: 0.6 })
  // x 轴标签：最多十来个
  g.fillStyle = th.fg2
  g.textAlign = 'center'
  g.textBaseline = 'top'
  const every = Math.max(1, Math.ceil(size / Math.max(1, Math.floor(plotW.value / 48))))
  for (let i = 0; i < size; i += every) g.fillText(rows.value[start + i]!.label, xOf(start + i + 0.5), PAD.t + plotH + 6)
}

// ---- 命中测试 ----

function localXY(e: MouseEvent): [number, number] {
  const rect = cvs.value!.getBoundingClientRect()
  return [e.clientX - rect.left, e.clientY - rect.top]
}

function inPlot(mx: number, my: number): boolean {
  return mx >= PAD.l && mx < PAD.l + plotW.value && my >= PAD.t - 4 && my <= PAD.t + plotH + 4
}

function hitOverview(mx: number, my: number): { r: number; cat: Cat | null } | null {
  const n = rows.value.length
  const B = buckets.value
  if (!n || !B.length || !inPlot(mx, my)) return null
  const fx = (mx - PAD.l) / plotW.value
  const r = Math.min(n - 1, Math.max(0, Math.floor(fx * n)))
  const b = B[Math.min(B.length - 1, Math.max(0, Math.floor(fx * B.length)))]!
  const cs = cats.value
  let up = 0
  let dn = 0
  for (let k = 0; k < cs.length; k++) {
    const v = b.v[k]!
    if (!v) continue
    const a = v > 0 ? up : dn
    const y0 = yOf(a)
    const y1 = yOf(a + v)
    if (my >= Math.min(y0, y1) && my <= Math.max(y0, y1)) return { r, cat: cs[k]! }
    if (v > 0) up += v
    else dn += v
  }
  return { r, cat: null }
}

function hitDna(mx: number, my: number): { r: number; strip: Strip | null; sign: 1 | -1 } | null {
  if (!rows.value.length || !inPlot(mx, my)) return null
  const { start, size, slot } = dnaGeom()
  const i = Math.floor((mx - PAD.l) / slot)
  if (i < 0 || i >= size) return null
  const r = start + i
  const col = colAt(r)
  const ppt = plotH / (lastScale.max - lastScale.min)
  const y0 = yOf(0)
  const up = my <= y0
  let y = y0
  for (const s of up ? col.up : col.down) {
    const h = s.h * ppt
    if (h <= 0) continue
    const yn = up ? y - h : y + h
    if (my >= Math.min(y, yn) && my <= Math.max(y, yn)) return { r, strip: s, sign: up ? 1 : -1 }
    y = yn
  }
  return { r, strip: null, sign: 1 }
}

function nodeLine(n: CtxNode): string {
  return n.name && n.cat !== 'user' ? `${n.name} · ${n.label}` : n.label
}

function dnaTipHtml(r: number, strip: Strip | null, sign: 1 | -1): string {
  const th = theme.value
  const row = rows.value[r]
  if (!row) return ''
  let html = ttTitle(row.title)
  if (strip) {
    const label = strip.node ? nodeLine(strip.node) : t('ctxTrend.unlogged')
    const h = Math.round(strip.h)
    const value = mode.value === 'delta' ? fmtSigned(sign * h) : `${strip.node ? '≈' : ''}${fmtInt(h)}`
    html += ttRow(catColor(strip.cat, th), label.length > 80 ? `${label.slice(0, 80)}…` : label, value)
    html += ttRow(null, catLabel(strip.cat), strip.kind ? t(`ctxTrend.${strip.kind}`) : strip.node ? t('ctxTrend.approxEst') : '', true)
    if (strip.node) html += `<div style="margin-top:4px;color:${th.fg2}">${esc(t('ctxTrend.locateHint'))}</div>`
  } else html += ttRow(null, t('ctx.trend.estimate'), fmtInt(sumParts(row.parts)), true)
  if (row.step.approximate) html += `<div style="margin-top:4px;color:${th.fg2}">${esc(t('ctxTrend.approximate'))}</div>`
  return html
}

function onMove(e: MouseEvent) {
  if (!cvs.value) return
  const [mx, my] = localXY(e)
  let r: number | null = null
  let html = ''
  if (view.value === 'overview') {
    const h = hitOverview(mx, my)
    r = h?.r ?? null
    hoverCat.value = h?.cat ?? null
    if (h) html = tooltipHtml(h.r)
  } else if (view.value === 'dna') {
    const h = hitDna(mx, my)
    r = h?.r ?? null
    hoverKey.value = h?.strip ? (h.strip.node?.key ?? OTHER_KEY) : null
    if (h) html = dnaTipHtml(h.r, h.strip, h.sign)
  }
  hoverRow.value = r
  hovered.value = r != null ? (rows.value[r]?.step.idx ?? null) : null
  tip.value = html ? { x: mx, y: my, html } : null
}

function onLeave() {
  hoverRow.value = null
  hoverCat.value = null
  hoverKey.value = null
  tip.value = null
  hovered.value = null
}

function onCanvasClick(e: MouseEvent) {
  if (!cvs.value) return
  const [mx, my] = localXY(e)
  if (view.value === 'overview') {
    const h = hitOverview(mx, my)
    if (!h) return
    const row = rows.value[h.r]
    if (h.cat) {
      pinnedCat.value = pinnedCat.value === h.cat ? null : h.cat
      if (pinnedCat.value) emit('focusCat', pinnedCat.value)
    }
    if (row) emit('select', row.step.idx)
  } else if (view.value === 'dna') {
    const h = hitDna(mx, my)
    const row = h ? rows.value[h.r] : undefined
    if (!h || !row) return
    emit('select', row.step.idx)
    if (h.strip?.node) emit('locate', h.strip.node.key, row.step.idx)
  }
}

/** 换视图时清掉悬停（ECharts 那边的 globalout 不会再来） */
watch(view, onLeave)

const tipStyle = computed(() => {
  const p = tip.value
  if (!p) return {}
  const right = p.x > cw.value / 2
  return {
    top: `${Math.max(0, Math.min(p.y + 14, H - 40))}px`,
    ...(right ? { right: `${cw.value - p.x + 14}px` } : { left: `${p.x + 14}px` }),
  }
})

// ---- 简报 ----

const briefIdx = computed(() => hovered.value ?? (props.selected < props.fold.steps.length ? props.selected : props.fold.steps.length - 1))
const brief = computed(() => briefOf(props.fold, briefIdx.value))
const briefStep = computed(() => props.fold.steps[briefIdx.value])

// ---- 控件 ----

const GRAN_OPTIONS = computed(() => [
  { value: 'step' as const, label: t('ctx.trend.step') },
  { value: 'turn' as const, label: t('ctx.trend.turn') },
])
const MODE_OPTIONS = computed(() => [
  { value: 'total' as const, label: t('ctx.trend.total') },
  { value: 'delta' as const, label: t('ctx.trend.delta') },
])
const AUTO_OPTIONS = computed(() => [
  ...(['100', '200', '300'] as const).map((v) => ({ value: v, label: t('ctxTrend.autoSteps', { n: Number(v) }) })),
  { value: 'off' as const, label: t('ctxTrend.autoOff') },
])

/** 总览打开时 DNA / 自适应不生效；点它们就退出总览并打开 */
function toggleDna() {
  if (overview.value) {
    overview.value = false
    dna.value = true
  } else dna.value = !dna.value
}
function toggleAdaptive() {
  if (overview.value) {
    overview.value = false
    adaptive.value = true
  } else adaptive.value = !adaptive.value
}

const toggles = computed(() => [
  { key: 'curve', on: curve.value, icon: 'i-jannchie-chart-line', label: t('ctx.trend.duration'), tip: t('ctx.trend.duration'), click: () => (curve.value = !curve.value) },
  { key: 'overview', on: overview.value, icon: 'i-jannchie-arrows-horizontal', label: t('ctxTrend.overview'), tip: t('ctxTrend.overviewTip'), click: () => (overview.value = !overview.value) },
  { key: 'adaptive', on: adaptive.value && !overview.value, icon: 'i-jannchie-arrows-up-down', label: t('ctxTrend.adaptive'), tip: t('ctxTrend.adaptiveTip'), click: toggleAdaptive },
  { key: 'dna', on: dna.value && !overview.value, icon: 'i-jannchie-dna', label: t('ctxTrend.dna'), tip: t('ctxTrend.dnaTip'), click: toggleDna },
])

const chartLabel = computed(() => {
  const last = rows.value[rows.value.length - 1]
  return `${t('ctx.trend.title')}: ${rows.value.length}${last?.step.prompt != null ? `, ${fmtInt(last.step.prompt)}` : ''}`
})

const hint = computed(() => (view.value === 'overview' ? t('ctxTrend.overviewHint') : view.value === 'dna' ? t('ctxTrend.dnaHint') : t('ctx.trend.pickHint')))
</script>

<template>
  <DashCard icon="i-jannchie-chart-stacked" :title="t('ctx.trend.title')" :caption="t('ctx.trend.caption')">
    <template #controls>
      <div class="flex flex-wrap items-center justify-end gap-2">
        <Segmented v-model="gran" :label="t('ctx.trend.granularity')" :options="GRAN_OPTIONS" />
        <Segmented v-model="mode" :label="t('ctx.trend.mode')" :options="MODE_OPTIONS" />
        <div class="flex flex-wrap items-center gap-1" role="group" :aria-label="t('ctxTrend.view')">
          <button
            v-for="b in toggles"
            :key="b.key"
            type="button"
            class="segment border-0"
            :class="b.on ? 'segment-selected' : 'bg-transparent ink-muted surface'"
            :aria-pressed="b.on"
            :title="b.tip"
            @click="b.click"
          >
            <i :class="b.icon" class="h-3.5 w-3.5" aria-hidden="true" />{{ b.label }}
          </button>
        </div>
      </div>
    </template>

    <EChart v-if="view === 'bars'" :option="option" :height="H" :label="chartLabel" class="cursor-pointer" @ready="onReady" />
    <div v-else ref="wrap" class="relative w-full min-w-0" :style="{ height: `${H}px` }">
      <canvas
        ref="cvs"
        role="img"
        :aria-label="chartLabel"
        class="block h-full w-full cursor-pointer"
        @mousemove="onMove"
        @mouseleave="onLeave"
        @click="onCanvasClick"
      />
      <!-- tooltip 内容都经过 esc / ttRow 转义 -->
      <div v-if="tip" class="trend-tip" :style="tipStyle" v-html="tip.html" />
    </div>

    <!-- DNA 的可见范围拖条（总览不需要：整段都在） -->
    <div v-if="view === 'dna' && rows.length > dnaWin.size" class="mt-1 flex items-center gap-2 text-xs ink-soft">
      <input
        type="range"
        class="min-w-0 flex-1"
        :min="0"
        :max="rows.length - dnaWin.size"
        :value="dnaWin.start"
        :aria-label="t('ctxTrend.scrub')"
        @input="onScrub"
      />
      <span class="num shrink-0">{{
        t('ctxTrend.range', { a: rows[dnaWin.start]?.label ?? '', b: rows[dnaWin.start + dnaWin.size - 1]?.label ?? '', n: rows.length })
      }}</span>
    </div>

    <div class="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs ink-soft">
      <p class="m-0 min-w-0 flex-1">{{ hint }}</p>
      <button v-if="view === 'overview' && pinnedCat" type="button" class="pin-chip surface" :title="t('ctxTrend.unpin')" @click="pinnedCat = null">
        <span class="inline-block h-2 w-2 rounded-full" :style="{ background: catColor(pinnedCat, theme) }" />
        {{ t('ctxTrend.pinned', { cat: catLabel(pinnedCat) }) }}
        <i class="i-jannchie-x h-3 w-3" aria-hidden="true" />
      </button>
      <label class="flex items-center gap-1" :title="t('ctxTrend.autoOverviewTip')">
        {{ t('ctxTrend.autoOverview') }}
        <select v-model="autoOverview" class="field py-0.5 text-xs">
          <option v-for="o in AUTO_OPTIONS" :key="o.value" :value="o.value">{{ o.label }}</option>
        </select>
      </label>
    </div>

    <!-- 步骤简报：谁开的头、新进来了什么、回了什么 -->
    <div v-if="briefStep" class="mt-3 grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1 rounded-xl px-3 py-2 text-xs surface">
      <span class="ink-soft">#{{ briefStep.idx + 1 }}</span>
      <span class="truncate ink-muted">
        <template v-if="briefStep.turnN != null">{{ t('ctx.trend.turnN', { n: briefStep.turnN }) }} · </template>{{ briefStep.model }}
        <template v-if="briefStep.prompt != null"> · <span class="num">{{ fmtInt(briefStep.prompt) }}</span></template>
      </span>
      <span class="ink-soft">{{ t('ctx.trend.brief.user') }}</span>
      <button v-if="brief.opener" type="button" class="brief-row" @click="emit('focus', brief.opener)">{{ brief.opener.label }}</button>
      <span v-else class="ink-soft">{{ t('ctx.trend.brief.none') }}</span>
      <span class="ink-soft">{{ t('ctx.trend.brief.in') }}</span>
      <span v-if="brief.inputs.length" class="flex min-w-0 flex-col">
        <button v-for="n in brief.inputs.slice(0, 4)" :key="n.key" type="button" class="brief-row" @click="emit('focus', n)">
          <span class="mr-1 inline-block h-2 w-2 rounded-full" :style="{ background: catColor(n.cat, theme) }" />{{ nodeLine(n) }}
          <span class="ml-1 num ink-soft">≈{{ fmtBig(n.est) }}</span>
        </button>
        <span v-if="brief.inputs.length > 4" class="ink-soft">+{{ brief.inputs.length - 4 }}</span>
      </span>
      <span v-else class="ink-soft">{{ t('ctx.trend.brief.none') }}</span>
      <span class="ink-soft">{{ t('ctx.trend.brief.response') }}</span>
      <span v-if="brief.response.length" class="flex min-w-0 flex-col">
        <button v-for="n in brief.response.slice(0, 4)" :key="n.key" type="button" class="brief-row" @click="emit('focus', n)">
          <i v-if="n.kinds?.includes('tools')" class="i-jannchie-toolbox mr-1 h-3 w-3" aria-hidden="true" />{{ nodeLine(n) }}
        </button>
        <span v-if="brief.response.length > 4" class="ink-soft">+{{ brief.response.length - 4 }}</span>
      </span>
      <span v-else class="ink-soft">{{ t('ctx.trend.brief.none') }}</span>
    </div>
  </DashCard>
</template>

<style scoped>
.brief-row {
  display: block;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  border: 0;
  background: transparent;
  padding: 0;
  text-align: left;
  color: var(--fg-1);
  cursor: pointer;
}
.brief-row:hover {
  color: var(--fg);
  text-decoration: underline;
}
/* 与 ECharts tooltipBase 同一套外观 */
.trend-tip {
  position: absolute;
  z-index: 10;
  max-width: 360px;
  pointer-events: none;
  border: 1px solid var(--line);
  border-radius: 12px;
  background: var(--bg);
  padding: 8px 12px;
  color: var(--fg);
  font-size: 12px;
  box-shadow: var(--tooltip-shadow);
}
.pin-chip {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  border: 0;
  border-radius: 999px;
  padding: 1px 8px;
  color: var(--fg-1);
  cursor: pointer;
}
</style>
