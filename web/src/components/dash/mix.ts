// 上下文组成（Mix）→ 图上的段：类别顺序、配色、名字都跟上下文视图（lib/ctxView.ts）走，两边颜色对得上
import type { Mix } from '../../api'
import type { ChartTheme } from '../../lib/chartTheme'
import { ALL_CATS, type Cat } from '../../lib/context'
import { catColor, catLabel } from '../../lib/ctxView'
import { fmtBig, fmtInt, fmtPct } from '../../lib/format'

export interface MixSeg {
  key: Cat
  label: string
  value: number
  color: string
  share: number
  tip: string
}

export function mixTotal(m: Mix | null | undefined): number {
  if (!m) return 0
  let s = 0
  for (const c of ALL_CATS) s += m[c] ?? 0
  return s
}

/** 按固定类别顺序出段；值为 0 的也留着（图例要稳定），画图时自己过滤 */
export function mixSegs(m: Mix | null | undefined, theme: ChartTheme): MixSeg[] {
  const total = mixTotal(m)
  return ALL_CATS.map((c) => {
    const value = m?.[c] ?? 0
    const share = total > 0 ? value / total : 0
    const label = catLabel(c)
    return { key: c, label, value, color: catColor(c, theme), share, tip: `${label}\n${fmtInt(value)} tokens · ${fmtPct(share)}` }
  })
}

/** 给读屏的一行：「用户消息 1.2M（30%）、工具结果 …」，只列非零的 */
export function mixAria(segs: MixSeg[]): string {
  return segs
    .filter((s) => s.value > 0)
    .map((s) => `${s.label} ${fmtBig(s.value)} (${fmtPct(s.share)})`)
    .join(', ')
}
