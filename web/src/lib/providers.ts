// provider 与工具类别的展示：颜色、图标、中文名。
// provider 的颜色只在这里定一次（PROVIDER_TONE）：界面上的点用对应 token 的类，图表用同名的 ChartTheme 字段（summaryModel.providerColor）。
import type { Provider, ToolCategory } from '../api'

const PROVIDER_NAMES: Record<string, string> = {
  'claude-code': 'Claude Code',
  pfh: 'Preferred Harness',
  dsh: 'Deepseek Harness',
  codex: 'Codex',
}

/** 界面上显示的全名；URL、筛选参数里仍用 id */
export function providerName(p: Provider): string {
  return PROVIDER_NAMES[p] ?? p
}

/** provider 的标识色，取 style.css 的 token：claude-code = --tool 琥珀，pfh = --accent 蓝，dsh = --user 紫，codex = --ok 绿，其余 --chart-other 灰 */
export type ProviderTone = 'tool' | 'accent' | 'user' | 'ok' | 'other'

const PROVIDER_TONE: Record<string, ProviderTone> = {
  'claude-code': 'tool',
  pfh: 'accent',
  dsh: 'user',
  codex: 'ok',
}

export function providerTone(p: Provider | string): ProviderTone {
  return PROVIDER_TONE[p] ?? 'other'
}

// 类名写全，UnoCSS 才扫得到
const TONE_BG: Record<ProviderTone, string> = {
  tool: 'bg-[var(--tool)]',
  accent: 'bg-accent',
  user: 'bg-[var(--user)]',
  ok: 'bg-ok',
  other: 'bg-[var(--chart-other)]',
}

/** provider 的标识点（背景类） */
export function providerDot(p: Provider): string {
  return TONE_BG[providerTone(p)]
}

export const TOOL_ICON: Record<ToolCategory, string> = {
  shell: 'i-jannchie-terminal',
  read: 'i-jannchie-file',
  edit: 'i-jannchie-edit',
  search: 'i-jannchie-search',
  web: 'i-jannchie-globe',
  agent: 'i-jannchie-robot',
  user: 'i-jannchie-help',
  mcp: 'i-jannchie-plug',
  meta: 'i-jannchie-sliders',
  other: 'i-jannchie-tools',
}
