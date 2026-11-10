// 仪表盘上「点一项就筛会话」的那几种筛选：工具、skill、命令（程序）。都在前端按 SessionRow 的同名表过滤，几个同时选是 AND。
// 状态放在 URL query 里（?tool=&skill=&cmd=），刷新不丢；日期（?day=）走的是范围，不在这里
import type { SessionRow } from '../../api'

export type PickKind = 'tool' | 'skill' | 'program'

/** URL query 的键 */
export const PICK_QUERY: Record<PickKind, string> = { tool: 'tool', skill: 'skill', program: 'cmd' }

export type Picks = Record<PickKind, string | null>

/** 会话行里对应的表：工具名 / skill / 程序 → 次数 */
const FIELD: Record<PickKind, 'tools' | 'skills' | 'programs'> = { tool: 'tools', skill: 'skills', program: 'programs' }

export function matchPicks(s: Pick<SessionRow, 'tools' | 'skills' | 'programs'>, p: Picks): boolean {
  for (const k of Object.keys(FIELD) as PickKind[]) {
    const v = p[k]
    if (v != null && !((s[FIELD[k]] ?? {})[v] ?? 0)) return false
  }
  return true
}

export function anyPick(p: Picks): boolean {
  return p.tool != null || p.skill != null || p.program != null
}

/** 截断后的排行里补上选中的那项（排在末尾），筛选状态总能看见、能点掉 */
export function withSelected<R>(shown: readonly R[], all: readonly R[], sel: string | null | undefined, key: (r: R) => string): R[] {
  const out = [...shown]
  if (sel && !out.some((r) => key(r) === sel)) {
    const r = all.find((x) => key(x) === sel)
    if (r) out.push(r)
  }
  return out
}
