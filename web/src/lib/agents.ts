// 子 agent 会话的归拢：每个会话挂到它的根会话下面（父链拉平），在根下面按类别 / 编排分组。
// 侧栏用它把几百个子 agent 收进根会话一行；类别、label、run 都来自 /api/sessions（见 docs/architecture.md）。
// 不依赖 Vue / i18n：fleet.ts 在 worker 里也用这里的函数。
import type { SessionInfo } from '../api'

/** 两分钟内还在写的算进行中 */
export const RUNNING_MS = 120_000

export function isRunning(s: SessionInfo, now = Date.now()): boolean {
  return now - Date.parse(s.mtime) < RUNNING_MS
}

/** 会话的显示名：agent 的短名字优先（workflow 的 label、Codex 的 agent 路径），其次是后端清理过的第一句提示；
 * 都没有就 null，调用方再退回会话 id。界面上统一走这里 */
export function sessionTitle(s: { title?: string | null; label?: string | null }): string | null {
  return s.label || s.title || null
}

/** 父链一直往上找到列表里最顶上的那个；父会话不在列表里就停在那一层。成环（不该有）时停在回到起点之前 */
export function rootOf(s: SessionInfo, bySession: Map<string, SessionInfo>): SessionInfo {
  let cur = s
  const seen = new Set([s.session])
  for (let p = s.parent_session; p && !seen.has(p); p = cur.parent_session) {
    seen.add(p)
    const up = bySession.get(p)
    if (!up) break
    cur = up
  }
  return cur
}

/** 一次 workflow 运行的子 agent */
export interface AgentGroup {
  /** 在根会话下唯一：`run:<run>` 是一次 workflow 运行，`workflow` 是认不出运行的 workflow agent */
  key: string
  /** 组名（转述的用户请求）；没有就 null，由界面给个通用名字 */
  title: string | null
  /** mtime 从新到旧，不会是空的 */
  items: SessionInfo[]
}

/** mtime 从新到旧 */
export const newestFirst = (a: SessionInfo, b: SessionInfo) => (a.mtime < b.mtime ? 1 : a.mtime > b.mtime ? -1 : 0)

/** 一个根会话的后代分组：不属于任何编排的子 agent（plain），和每次 workflow 运行一组（runs，按最近活动排） */
export function groupAgents(kids: SessionInfo[]): { plain: SessionInfo[]; runs: AgentGroup[] } {
  const plain: SessionInfo[] = []
  const byKey = new Map<string, AgentGroup>()
  for (const s of kids) {
    if (s.kind !== 'workflow') {
      plain.push(s)
      continue
    }
    const key = s.run ? `run:${s.run}` : 'workflow'
    let g = byKey.get(key)
    if (!g) byKey.set(key, (g = { key, title: null, items: [] }))
    g.items.push(s)
    g.title ??= s.run_title ?? null
  }
  const runs = [...byKey.values()]
  for (const g of runs) g.items.sort(newestFirst)
  runs.sort((a, b) => newestFirst(a.items[0]!, b.items[0]!))
  return { plain: plain.sort(newestFirst), runs }
}
