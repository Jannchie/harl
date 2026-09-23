// provider 与工具类别的展示：颜色、图标、中文名。图表另有一份从 CSS token 读的（summaryModel.providerColor）。
import type { Provider, ToolCategory } from '../api'

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
