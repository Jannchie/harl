// 两个旭日图共用：节点结构、内嵌标签的墨色选择、基础 series 配置
import type { SunburstSeriesOption } from 'echarts/charts'
import type { ChartTheme } from './chartTheme'

export interface SunNode {
  name: string
  value?: number
  children?: SunNode[]
  itemStyle?: { color?: string; opacity?: number }
  label?: { color?: string; show?: boolean }
  /** tooltip 里用的说明 */
  meta?: Record<string, string>
}

/** 填充色上放文字：按亮度选白或墨 */
export function inkOn(hex: string, dark = '#1c1f23', light = '#ffffff'): string {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim())
  if (!m) return light
  const n = parseInt(m[1]!, 16)
  const r = (n >> 16) & 255
  const g = (n >> 8) & 255
  const b = n & 255
  const lum = (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255
  return lum > 0.6 ? dark : light
}

export function colored(node: SunNode, color: string, opacity = 1): SunNode {
  node.itemStyle = { color, opacity }
  node.label = { color: inkOn(color) }
  return node
}

export function sunburstSeries(t: ChartTheme, data: SunNode[]): SunburstSeriesOption {
  return {
    type: 'sunburst',
    data: data as SunburstSeriesOption['data'],
    radius: ['18%', '92%'],
    center: ['50%', '52%'],
    sort: undefined,
    nodeClick: false,
    itemStyle: { borderColor: t.bg, borderWidth: 2 },
    emphasis: { focus: 'ancestor', itemStyle: { borderColor: t.fg, borderWidth: 1 } },
    label: {
      rotate: 'tangential',
      minAngle: 9,
      fontSize: 10,
      fontFamily: t.fontFamily,
      overflow: 'truncate',
      width: 70,
      ellipsis: '…',
    },
    levels: [
      {},
      { r0: '18%', r: '46%', label: { rotate: 'tangential', fontWeight: 600 } },
      { r0: '46%', r: '92%', label: { align: 'left' } },
    ],
  }
}
