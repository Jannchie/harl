// provider 与工具类别的展示：颜色、图标、中文名。图表另有一份从 CSS token 读的（summaryModel.providerColor）。
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

/** provider 的标识点。只有这一个地方用 provider 的颜色 */
export function providerDot(p: Provider): string {
  if (p === 'claude-code') return 'bg-amber-500'
  if (p === 'pfh') return 'bg-blue-500'
  if (p === 'dsh') return 'bg-violet-500'
  if (p === 'codex') return 'bg-green-600'
  return 'bg-neutral-400'
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
