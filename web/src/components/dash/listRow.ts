// 会话列表（卡片 / 表格）共用的一行：会话本身 + 前端算出来的项目、计费 tokens、进行中、会话家族合计
import type { SessionInfo } from '../../api'
import { isRunning, sessionTitle } from '../../lib/agents'
import type { PricedSessionRow } from '../../lib/pricing'
import { projectOf, type Project } from '../../lib/project'
import { totalTokens } from '../../lib/summaryModel'

export interface Family {
  /** 家族在当前范围里的会话数（含自己） */
  n: number
  cost: number
  tokens: number
}

export interface ListRow {
  s: PricedSessionRow
  /** 显示名：会话列表里有这个快照就按它的（agent 的 label 优先，见 sessionTitle），否则用汇总里的标题 */
  title: string | null
  project: Project
  tokens: number
  running: boolean
  /** 只有家族里不止自己一个时才有 */
  family: Family | null
  /** 算「几分钟前」用的此刻，和 running 同一个时刻 */
  now: number
}

/**
 * 会话家族的键：沿 parent_session 一路往上找到根会话。
 * SessionRow 不带自己的会话 id，要靠会话列表（/api/sessions）把快照 id 对到会话 id；
 * 列表还没到或对不上时退回到直接父会话（同一个父的子 agent 仍能归在一起）
 */
function familyKey(s: PricedSessionRow, byId: Map<string, SessionInfo>, bySession: Map<string, SessionInfo>): string {
  const info = byId.get(s.id)
  if (!info) return s.parent_session ? `s:${s.parent_session}` : `id:${s.id}`
  let cur = info.session
  let parent = info.parent_session
  // 防环：最多爬 32 层
  for (let i = 0; parent && i < 32; i++) {
    cur = parent
    parent = bySession.get(parent)?.parent_session ?? null
  }
  return `s:${cur}`
}

export function buildRows(
  sessions: readonly PricedSessionRow[],
  list: readonly SessionInfo[] | null,
  bySession: Map<string, SessionInfo>,
  now: number,
): ListRow[] {
  const byId = new Map((list ?? []).map((x) => [x.id, x]))
  const keys = sessions.map((s) => familyKey(s, byId, bySession))
  const fam = new Map<string, Family>()
  sessions.forEach((s, i) => {
    const f = fam.get(keys[i]!) ?? { n: 0, cost: 0, tokens: 0 }
    f.n++
    f.cost += s.cost_usd
    f.tokens += totalTokens(s.tokens)
    fam.set(keys[i]!, f)
  })
  return sessions.map((s, i) => {
    const info = byId.get(s.id)
    // 没有列表项时按最后活动时间判断，和 isRunning 同一个两分钟窗口
    const running = info ? isRunning(info, now) : s.last_ts ? now - Date.parse(s.last_ts) < 120_000 : false
    const f = fam.get(keys[i]!)
    const title = (info && sessionTitle(info)) ?? s.title
    return { s, title, project: projectOf(s), tokens: totalTokens(s.tokens), running, family: f && f.n > 1 ? f : null, now }
  })
}

/** 搜索：标题、工作目录、最后一条消息，忽略大小写；空格分开的几个词都要命中 */
export function matchQuery(r: ListRow, q: string): boolean {
  const words = q.toLowerCase().split(/\s+/).filter(Boolean)
  if (!words.length) return true
  const hay = `${r.title ?? ''}\n${r.s.title ?? ''}\n${r.s.cwd ?? ''}\n${r.s.last_user ?? ''}`.toLowerCase()
  return words.every((w) => hay.includes(w))
}
