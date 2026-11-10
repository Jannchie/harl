// 子 agent 会话的归拢：每个会话挂到它的根会话下面（父链拉平），在根下面按类别 / 编排分组。
// 侧栏用它把几百个子 agent 收进根会话一行；类别、label、run 都来自 /api/sessions（见 docs/architecture.md）。
// 不依赖 Vue / i18n：fleet.ts 在 worker 里也用这里的函数。
import type { AgentKind, SessionInfo } from '../api'

/** 两分钟内还在写的算进行中 */
export function isRunning(s: SessionInfo, now = Date.now()): boolean {
  return now - Date.parse(s.mtime) < 120_000
}

/** 会话的显示名：agent 的短名字优先（workflow 的 label、Codex 的 agent 路径），其次是后端清理过的第一句提示；
 * 都没有就 null，调用方再退回会话 id。界面上统一走这里（lib/title.ts 也导出它） */
export function sessionTitle(s: { title?: string | null; label?: string | null }): string | null {
  return s.label || s.title || null
}

/** 老后端没有 kind：有父会话就当普通子 agent */
export function kindOf(s: SessionInfo): AgentKind {
  return s.kind ?? (s.parent_session ? 'subagent' : 'main')
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

export interface AgentGroup {
  /** 在根会话下唯一：`agents` 是不属于任何编排的子 agent，`run:<run>` 是一次 workflow 运行，`workflow` 是认不出运行的 workflow agent */
  key: string
  /** run 组的组名（转述的用户请求）；没有就 null，由界面给个通用名字 */
  title: string | null
  /** mtime 从新到旧 */
  items: SessionInfo[]
  /** 组里最新的 mtime，组之间按它排 */
  mtime: string
}

const newestFirst = (a: SessionInfo, b: SessionInfo) => (a.mtime < b.mtime ? 1 : a.mtime > b.mtime ? -1 : 0)

/** 一个根会话的后代分组：普通子 agent 一组排最前，之后每次 workflow 运行一组，按最近活动排 */
export function groupAgents(kids: SessionInfo[]): AgentGroup[] {
  const agents: SessionInfo[] = []
  const runs = new Map<string, AgentGroup>()
  for (const s of kids) {
    if (kindOf(s) !== 'workflow') {
      agents.push(s)
      continue
    }
    const key = s.run ? `run:${s.run}` : 'workflow'
    let g = runs.get(key)
    if (!g) runs.set(key, (g = { key, title: null, items: [], mtime: s.mtime }))
    g.items.push(s)
    g.title ??= s.run_title ?? null
    if (s.mtime > g.mtime) g.mtime = s.mtime
  }
  const out = [...runs.values()].sort((a, b) => (a.mtime < b.mtime ? 1 : a.mtime > b.mtime ? -1 : 0))
  for (const g of out) g.items.sort(newestFirst)
  if (agents.length) out.unshift({ key: 'agents', title: null, items: agents.sort(newestFirst), mtime: agents[0]!.mtime })
  return out
}
