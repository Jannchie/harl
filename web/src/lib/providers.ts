// provider 与工具类别的展示：颜色、图标、中文名。图表另有一份从 CSS token 读的（summaryModel.providerColor）。
import type { Provider, ToolCategory } from '../api'

const PROVIDER_NAMES: Record<string, string> = {
  'claude-code': 'Claude Code',
  pfh: 'Preferred Harness',
  dsh: 'Deepseek Harness',
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
  return 'bg-neutral-400'
}

export const TOOL_ICON: Record<ToolCategory, string> = {
  shell: 'i-carbon-terminal',
  read: 'i-carbon-document',
  edit: 'i-carbon-edit',
  search: 'i-carbon-search',
  web: 'i-carbon-earth',
  agent: 'i-carbon-bot',
  user: 'i-carbon-help',
  mcp: 'i-carbon-plug',
  meta: 'i-carbon-settings-adjust',
  other: 'i-carbon-tools',
}
