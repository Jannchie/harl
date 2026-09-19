// ECharts 按需引入：只注册仪表盘用到的图表和组件，其余不进 bundle。
import * as echarts from 'echarts/core'
import { BarChart, HeatmapChart, LineChart, SunburstChart, TreemapChart } from 'echarts/charts'
import {
  CalendarComponent,
  GridComponent,
  LegendComponent,
  TooltipComponent,
  VisualMapComponent,
} from 'echarts/components'
import { CanvasRenderer } from 'echarts/renderers'
import type {
  BarSeriesOption,
  HeatmapSeriesOption,
  LineSeriesOption,
  SunburstSeriesOption,
  TreemapSeriesOption,
} from 'echarts/charts'
import type {
  CalendarComponentOption,
  GridComponentOption,
  LegendComponentOption,
  TooltipComponentOption,
  VisualMapComponentOption,
} from 'echarts/components'
import type { ComposeOption } from 'echarts/core'

echarts.use([
  BarChart,
  LineChart,
  HeatmapChart,
  SunburstChart,
  TreemapChart,
  GridComponent,
  TooltipComponent,
  LegendComponent,
  CalendarComponent,
  VisualMapComponent,
  CanvasRenderer,
])

export type EChartsOption = ComposeOption<
  | BarSeriesOption
  | LineSeriesOption
  | HeatmapSeriesOption
  | SunburstSeriesOption
  | TreemapSeriesOption
  | GridComponentOption
  | TooltipComponentOption
  | LegendComponentOption
  | CalendarComponentOption
  | VisualMapComponentOption
>

export type ECharts = echarts.ECharts
export { echarts }
