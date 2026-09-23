// 会话 → 项目（工作目录）。后端从日志开头取 cwd；取不到就退回按 provider 的存放位置猜。
import type { SessionInfo } from '../api'

export interface Project {
  /** 分组键：小写、`/` 分隔、去掉结尾斜杠 */
  key: string
  /** 目录名 */
  label: string
  /** 完整路径，悬停时显示 */
  path: string
}

const UNKNOWN: Project = { key: '', label: '未知目录', path: '' }

export function projectOf(s: SessionInfo): Project {
  const cwd = s.cwd?.replace(/\\/g, '/').replace(/\/+$/, '')
  if (!cwd) return UNKNOWN
  const label = cwd.split('/').filter(Boolean).pop() ?? cwd
  return { key: cwd.toLowerCase(), label, path: cwd }
}
