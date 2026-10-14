// 会话 → 项目（工作目录）。后端从日志开头取 cwd；取不到就退回按 provider 的存放位置猜。
import { t } from '../i18n'

export interface Project {
  /** 分组键：小写、`/` 分隔、去掉结尾斜杠 */
  key: string
  /** 目录名 */
  label: string
  /** 完整路径，悬停时显示 */
  path: string
}

export function projectOf(s: { cwd?: string | null }): Project {
  const cwd = s.cwd?.replace(/\\/g, '/').replace(/\/+$/, '')
  // label 每次现取：切换语言后跟着变
  if (!cwd) return { key: '', label: t('common.project.unknown'), path: '' }
  const label = cwd.split('/').filter(Boolean).pop() ?? cwd
  return { key: cwd.toLowerCase(), label, path: cwd }
}
