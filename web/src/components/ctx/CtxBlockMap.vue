<script lang="ts">
import { toolSource, type Cat, type CtxNode } from '../../lib/context'

/** 块图里的一块：同类的项合在一起 */
export interface MapBlock {
  /** 分组键，见 {@link groupKey}；合并出来的「其余 N 组」是 null */
  key: string | null
  cat: Cat
  label: string
  /** 校准后的 token */
  tokens: number
  count: number
}

/** 一项在它的类别里归到哪一组：工具结果按工具名、工具定义按来源、注入和 skill 按注入方；其余整类一组（空串） */
export function groupSub(n: CtxNode): string {
  switch (n.cat) {
    case 'tool':
      return n.name ?? ''
    case 'tools':
      return n.plugin ?? toolSource(n) ?? ''
    case 'inject':
    case 'skill':
      return n.name ?? ''
    default:
      return ''
  }
}

/** 分组键：`<类别>:<组>`，比如 `tool:Bash`；整类一组时是 `user:` */
export function groupKey(n: CtxNode): string {
  return `${n.cat}:${groupSub(n)}`
}

interface Rect {
  x: number
  y: number
  w: number
  h: number
}

/** squarified treemap：values 已按从大到小排好，都 > 0 */
function squarify(values: number[], box: Rect): Rect[] {
  const out: Rect[] = []
  const total = values.reduce((a, b) => a + b, 0)
  if (total <= 0) return out
  const sizes = values.map((v) => (v / total) * box.w * box.h)
  let { x, y, w, h } = box
  let i = 0
  while (i < sizes.length) {
    const side = Math.min(w, h)
    // 往这一行里加，直到最差的长宽比变坏
    let j = i
    let sum = 0
    let mn = Number.POSITIVE_INFINITY
    let mx = 0
    let best = Number.POSITIVE_INFINITY
    while (j < sizes.length) {
      const s = sizes[j]!
      const ns = sum + s
      const nmn = Math.min(mn, s)
      const nmx = Math.max(mx, s)
      const worst = Math.max((side * side * nmx) / (ns * ns), (ns * ns) / (side * side * nmn))
      if (j > i && worst > best) break
      best = worst
      sum = ns
      mn = nmn
      mx = nmx
      j++
    }
    // 这一行沿短边排开
    const thick = side > 0 ? sum / side : 0
    let off = 0
    for (let k = i; k < j; k++) {
      const len = thick > 0 ? sizes[k]! / thick : 0
      out.push(w >= h ? { x, y: y + off, w: thick, h: len } : { x: x + off, y, w: len, h: thick })
      off += len
    }
    if (w >= h) {
      x += thick
      w -= thick
    } else {
      y += thick
      h -= thick
    }
    i = j
  }
  return out
}
</script>

<script setup lang="ts">
// 块图：选中那次请求的上下文按 token 画成 treemap，同类的项合成一块；点一块让下面的列表只看这一组
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useChartTheme } from '../../lib/chartTheme'
import { catColor, catLabel } from '../../lib/ctxView'
import { fmtBig, fmtInt, fmtPct } from '../../lib/format'

const props = defineProps<{
  blocks: MapBlock[]
  /** 当前在下面过滤的组 */
  active: string | null
}>()
const emit = defineEmits<{ pick: [key: string | null, cat: Cat] }>()
const { t } = useI18n()
const theme = useChartTheme()

/** 最多画这么多块；再小的按类别合成「其余 N 组」 */
const MAX_BLOCKS = 48
/** 占比低于这个的也合进「其余」 */
const MIN_SHARE = 0.004

const box = ref<HTMLElement | null>(null)
const size = ref({ w: 600, h: 160 })
// 容器可能后出现（一开始是空的），跟着元素挂上 / 卸下观察
watch(box, (el, _, onCleanup) => {
  if (!el) return
  const measure = () => {
    const r = el.getBoundingClientRect()
    if (r.width > 0 && r.height > 0) size.value = { w: r.width, h: r.height }
  }
  measure()
  const ro = new ResizeObserver(measure)
  ro.observe(el)
  onCleanup(() => ro.disconnect())
})

const total = computed(() => props.blocks.reduce((a, b) => a + Math.max(0, b.tokens), 0))

/** 排序、把小块按类别合并后的块 */
const shown = computed<MapBlock[]>(() => {
  const all = props.blocks.filter((b) => b.tokens > 0).sort((a, b) => b.tokens - a.tokens)
  const tot = total.value
  const keep: MapBlock[] = []
  const rest = new Map<Cat, MapBlock & { groups: number }>()
  for (const b of all) {
    if (keep.length < MAX_BLOCKS && b.tokens / tot >= MIN_SHARE) {
      keep.push(b)
      continue
    }
    const r = rest.get(b.cat) ?? { key: null, cat: b.cat, label: '', tokens: 0, count: 0, groups: 0 }
    r.tokens += b.tokens
    r.count += b.count
    r.groups++
    rest.set(b.cat, r)
  }
  for (const r of rest.values()) {
    keep.push({ ...r, label: t('ctxBrowser.map.others', { n: fmtInt(r.groups) }) })
  }
  return keep.sort((a, b) => b.tokens - a.tokens)
})

const rects = computed(() => {
  const { w, h } = size.value
  const rs = squarify(
    shown.value.map((b) => b.tokens),
    { x: 0, y: 0, w, h },
  )
  return shown.value.map((b, i) => {
    const r = rs[i]!
    return {
      b,
      i,
      style: {
        left: `${(r.x / w) * 100}%`,
        top: `${(r.y / h) * 100}%`,
        width: `${(r.w / w) * 100}%`,
        height: `${(r.h / h) * 100}%`,
      },
      // 太小的块不放字
      label: r.w >= 56 && r.h >= 22,
      sub: r.w >= 56 && r.h >= 38,
    }
  })
})

const hover = ref<number | null>(null)
const hovered = computed(() => (hover.value != null ? (shown.value[hover.value] ?? null) : null))

function describe(b: MapBlock): string {
  return [
    b.label,
    b.label === catLabel(b.cat) ? null : catLabel(b.cat),
    t('ctxBrowser.map.items', { n: fmtInt(b.count) }),
    `${fmtInt(Math.round(b.tokens))} tok`,
    t('ctxBrowser.map.share', { p: fmtPct(total.value > 0 ? b.tokens / total.value : 0) }),
  ]
    .filter(Boolean)
    .join(' · ')
}

function fill(b: MapBlock, i: number): string {
  const c = catColor(b.cat, theme.value)
  const on = (b.key != null && b.key === props.active) || hover.value === i
  return `color-mix(in srgb, ${c} ${on ? 55 : 30}%, transparent)`
}
</script>

<template>
  <div class="flex flex-col gap-1">
    <p v-if="!total" class="m-0 px-2 text-xs ink-soft">{{ t('ctxBrowser.map.empty') }}</p>
    <div v-else ref="box" class="relative h-40 overflow-hidden rounded-lg" @pointerleave="hover = null">
      <button
        v-for="r in rects"
        :key="`${r.b.cat}|${r.b.key ?? '*'}`"
        type="button"
        class="absolute box-border flex flex-col items-start justify-start overflow-hidden border-0 p-0 text-left kbd-ring"
        :style="r.style"
        :aria-label="describe(r.b)"
        :aria-pressed="r.b.key != null && r.b.key === active"
        :title="describe(r.b)"
        @pointerenter="hover = r.i"
        @focus="hover = r.i"
        @click="emit('pick', r.b.key, r.b.cat)"
      >
        <span
          class="absolute inset-px rounded-[3px]"
          :style="{
            background: fill(r.b, r.i),
            boxShadow: r.b.key != null && r.b.key === active ? `inset 0 0 0 1.5px ${catColor(r.b.cat, theme)}` : undefined,
          }"
        />
        <span v-if="r.label" class="relative max-w-full truncate px-1.5 pt-1 text-[11px] leading-tight ink-strong">{{ r.b.label }}</span>
        <span v-if="r.sub" class="relative px-1.5 text-[11px] leading-tight num ink-soft">{{ fmtBig(r.b.tokens) }}</span>
      </button>
    </div>
    <p v-if="total" class="m-0 min-h-4 truncate px-1 text-xs ink-soft num">{{ hovered ? describe(hovered) : t('ctxBrowser.map.tip') }}</p>
  </div>
</template>
