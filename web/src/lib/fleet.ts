// Portions derived from dsh-context (Apache-2.0, © 2025 bowenliang123), modified for harl. See THIRD_PARTY_NOTICES.md.
// Agent 网络（Fleet）的数据层：家族卡片、各会话日志里抽出的协作记录、跨会话的合并与解析。
//
// 各 provider 能拿到什么（都来自归档的日志，没有运行时接口）：
// - Claude Code：Agent / Task 工具调用 = 委派（tool_result 上的 links: spawned 指向子会话 agent-<id>，
//   toolUseResult 带 status / content）；后台 agent 的完成通知（links: notifies）= 回报；
//   父会话 SendMessage 送来的消息（links: delivered_from）和 SendMessage 调用 = 消息；
//   TaskCreate / TaskUpdate = 团队任务（状态、owner、blockedBy；没有写入范围）。
// - Codex：spawn_agent 调用 + event_msg 里的 SubAgentActivity（started / completed，带线程 id 和 agent 路径）= 委派；
//   response_item 的 agent_message（author / recipient 是 agent 路径，FINAL_ANSWER 当回报）= 消息。
//   没有团队任务；正文可能是加密的，那就只有元数据。
// - dsh：subagent/catalog、team/member = 委派（模式 one-shot / continuable）；子会话的 subagent/descriptor = 自己的模式；
//   user/message 的 source.kind（agent-message / team-message / coordinator / subagent-settled / subagent-report）= 消息和回报；
//   team/task = 团队任务（含 blockedBy、writeScopes、ownerId）；request/context 报上下文窗口。
// - pfh 等：只有父子关系（卡片和数字），没有协作记录。
import { markRaw, reactive } from 'vue'
import { api, toolFailed, type Event, type Provider, type SessionInfo } from '../api'
import { isRunning, sessionTitle } from './agents'
import { foldContext } from './context'
import { fleetCache, fleetStats, summarize } from './ctxView'
import type { FleetJob, FleetJobResult } from './fleetJob'
import { ensurePricing } from './pricing'

// ---- 家族卡片 ----

export interface FamilyCard {
  info: SessionInfo
  depth: number
  role: 'parent' | 'current' | 'child'
  /** 图上的上级（session id）；最顶上的父会话没有 */
  parent: string | null
}

/** 父会话链 + 当前 + 子孙（深度优先，同级按 mtime） */
export function familyCards(info: SessionInfo, bySession: Map<string, SessionInfo>): FamilyCard[] {
  const out: FamilyCard[] = []
  const parents: SessionInfo[] = []
  let p = info.parent_session
  const seen = new Set<string>([info.session])
  while (p && !seen.has(p)) {
    seen.add(p)
    const s = bySession.get(p)
    if (!s) break
    parents.unshift(s)
    p = s.parent_session
  }
  parents.forEach((s, i) => out.push({ info: s, depth: i, role: 'parent', parent: i ? parents[i - 1]!.session : null }))
  out.push({ info, depth: parents.length, role: 'current', parent: parents.at(-1)?.session ?? null })
  const kids = new Map<string, SessionInfo[]>()
  for (const s of bySession.values()) {
    if (!s.parent_session) continue
    const list = kids.get(s.parent_session)
    if (list) list.push(s)
    else kids.set(s.parent_session, [s])
  }
  const walk = (id: string, depth: number) => {
    for (const c of (kids.get(id) ?? []).sort((a, b) => a.mtime.localeCompare(b.mtime))) {
      if (seen.has(c.session)) continue
      seen.add(c.session)
      out.push({ info: c, depth, role: 'child', parent: id })
      walk(c.session, depth + 1)
    }
  }
  walk(info.session, parents.length + 1)
  return out
}

export const cardKey = (s: SessionInfo) => `${s.id}@${s.mtime}`

export { isRunning }

// ---- 单个会话里抽出的协作记录 ----

export type RecordKind = 'message' | 'report' | 'delegation'
/** dsh 的结算通知按 summary 的结尾认出来（与 dsh-context 相同的规则）；Claude Code 的通知按 <status> 映射 */
export type Settle = 'finished' | 'finishedFinal' | 'stopped' | 'outOfRoom' | 'declined' | 'failed'
export type DelegationMode = 'one-shot' | 'continuable' | 'external' | 'unknown'

/** 端点：日志里怎么写的就怎么记（会话 id、名字、Codex 的 agent 路径），合并时再解析成家族里的会话 */
export interface CollabRecord {
  key: string
  kind: RecordKind
  /** 细分：relay / mailbox / coordinator / send / settled / report / notify / final */
  sub: string
  ts: number
  from: string | null
  fromName?: string
  to: string | null
  /** 记在哪个会话的日志里 */
  source: string
  seq: number
  text: string
  /** 结算通知的原句 */
  summary?: string
  settle?: Settle | null
  /** dsh 结算里附带的结构化结果 */
  structured?: string
  /** 正文是加密的（Codex），只剩元数据 */
  encrypted?: boolean
  /** 发送方的记录（SendMessage 调用）；接收方也有记录时去掉 */
  sent?: boolean
  /** 只凭 task-id 猜的发送方（Claude Code 的完成通知）；合并时解析不到家族里的会话就丢掉 */
  weak?: boolean
}

export interface Delegation {
  key: string
  source: string
  seq: number
  ts: number
  /** 子会话（日志里写的 id / 路径），还不知道就 null */
  to: string | null
  /** Claude Code 的 teammate 名、Codex 的 task_name */
  name: string | null
  label: string | null
  prompt: string | null
  tool: string
  callId: string | null
  mode: DelegationMode | null
  background: boolean
  /** 日志里看到的结果：done / failed / stopped；没看到就 null */
  outcome: 'done' | 'failed' | 'stopped' | null
  report: string | null
  encrypted?: boolean
}

/** 团队任务的一次修订；字段缺省表示这次没改 */
export interface TaskRev {
  /** 同一个任务的合并键 */
  group: string
  id: string
  source: string
  seq: number
  ts: number
  subject?: string
  description?: string
  status?: string
  owner?: string | null
  blockedBy?: string[]
  addBlockedBy?: string[]
  addBlocks?: string[]
  writeScopes?: string[]
}

export interface TeamMember {
  id: string | null
  name: string
  description?: string
  phase?: string
  team?: string
}

export interface CollabDigest {
  session: string
  provider: Provider
  window: number | null
  /** dsh 子会话自己的模式 */
  mode: 'one-shot' | 'continuable' | null
  label: string | null
  firstTs: number | null
  lastTs: number | null
  records: CollabRecord[]
  delegations: Delegation[]
  tasks: TaskRev[]
  members: TeamMember[]
  /** Codex：agent 路径 → 线程 id */
  paths: [string, string][]
  /** 最后一条带正文的回复 */
  lastReply: string | null
}

const MAX_TEXT = 24_000

const obj = (v: unknown): Record<string, unknown> | null =>
  v != null && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, unknown>) : null
const str = (v: unknown): string | null => (typeof v === 'string' && v !== '' ? v : null)
const strs = (v: unknown): string[] => (Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string' && x !== '') : [])
const clip = (s: string) => (s.length > MAX_TEXT ? `${s.slice(0, MAX_TEXT)}…` : s)

/** tool 输出 / 消息体：字符串、part 数组、{text} / {content} 都拍成文本 */
export function textOf(v: unknown): string {
  if (v == null) return ''
  if (typeof v === 'string') return v
  if (Array.isArray(v)) return v.map(textOf).filter(Boolean).join('\n')
  const o = obj(v)
  if (o) {
    if (typeof o.text === 'string') return o.text
    if (o.content != null) return textOf(o.content)
    try {
      return JSON.stringify(o)
    } catch {
      return ''
    }
  }
  return String(v)
}

function xmlTag(s: string, tag: string): string | null {
  const m = new RegExp(`<${tag}>([\\s\\S]*?)</${tag}>`).exec(s)
  return m ? m[1]!.trim() : null
}

export function settleOf(summary: string): Settle | null {
  const t = summary.trim()
  if (t.endsWith('finished and will do no further work unless you send it more.')) return 'finished'
  if (t.endsWith('finished. It cannot receive follow-up messages.')) return 'finishedFinal'
  if (t.endsWith('was stopped before it finished.')) return 'stopped'
  if (t.endsWith('ran out of room before it finished.')) return 'outOfRoom'
  if (t.endsWith('declined the task.')) return 'declined'
  if (t.endsWith('failed before it finished.')) return 'failed'
  return null
}

/** dsh 结算通知：summary 之后是收尾消息，`Structured result: ` 之后是结构化结果 */
function splitSettled(summary: string, text: string): { report: string; structured?: string } {
  let rest = text.startsWith(summary) ? text.slice(summary.length).trim() : text.trim()
  let structured: string | undefined
  const i = rest.indexOf('Structured result: ')
  if (i >= 0) {
    structured = rest.slice(i + 19).trim() || undefined
    rest = rest.slice(0, i).trim()
  }
  if (rest === 'It left no closing message.' || rest.startsWith('It sent you a message just before finishing')) rest = ''
  else if (rest.startsWith('Its closing message:')) rest = rest.slice(20).trim()
  return { report: rest, structured }
}

const DSH_TEAM_RE = /^Team message \S+ from ([^:\n]{1,64}):\s*/
const DSH_AGENT_RE = /^(?:Agent \S+ sent a message: |Team message from ([^:\n]{1,64}):\s*)/
const DSH_REPORT_RE = /^Background subagent (\S+) reported:\s*/
const ENCRYPTED_RE = /^gAAAA[\w-]{20,}={0,2}$/
const CLAUDE_DELEGATE = new Set(['Agent', 'Task', 'Workflow'])
const COORDINATOR_RE = /^The coordinator sent a message[^\n]*:\n/
/** SendMessage 里指"上级"的写法：解析成发送方的父会话 */
const PARENT_ALIASES = new Set(['main', 'parent', 'lead', 'coordinator', 'leader'])
const CODEX_DELEGATE = new Set(['spawn_agent'])
const DSH_DELEGATE = new Set(['subagent', 'spawn_teammate', 'run_subagent', 'spawn_agent'])
const SEND_TOOLS = new Set(['SendMessage', 'send_message', 'send_input'])

function msOf(ts: string | undefined): number | null {
  if (!ts) return null
  const n = Date.parse(ts)
  return Number.isFinite(n) ? n : null
}

/** 扫一遍会话的事件，抽出协作相关的东西。只读字段，不改事件 */
export function scanCollab(session: string, provider: Provider, events: Event[]): CollabDigest {
  const d: CollabDigest = {
    session,
    provider,
    window: null,
    mode: null,
    label: null,
    firstTs: null,
    lastTs: null,
    records: [],
    delegations: [],
    tasks: [],
    members: [],
    paths: [],
    lastReply: null,
  }
  const byCall = new Map<string, Delegation>()
  /** 还没对上 catalog 的 dsh 委派调用 */
  const dshCalls: Delegation[] = []
  /** Claude TaskCreate：等结果里的任务号 */
  const pendingCreate = new Map<string, TaskRev>()
  let n = 0
  let lastTs = 0
  const push = (r: Omit<CollabRecord, 'key' | 'source'>) => d.records.push({ ...r, key: `${session}:${r.seq}:${n++}`, source: session })

  for (const ev of events) {
    const ts = msOf(ev.ts)
    if (ts != null) {
      lastTs = ts
      if (d.firstTs == null || ts < d.firstTs) d.firstTs = ts
      if (d.lastTs == null || ts > d.lastTs) d.lastTs = ts
    }
    const at = ts ?? lastTs
    const tool = ev.tool
    const input = obj(tool?.input)

    if (ev.kind === 'message' && ev.role === 'assistant' && ev.text?.trim()) d.lastReply = clip(ev.text)

    // ---- 通用：发消息的工具调用（发送方的记录）----
    if (ev.kind === 'tool_call' && tool && SEND_TOOLS.has(tool.name) && input) {
      const to = str(input.to) ?? str(input.recipient) ?? str(input.target) ?? str(input.id)
      const body = textOf(input.message ?? input.content ?? input.text ?? input.items)
      const enc = ENCRYPTED_RE.test(body)
      push({
        kind: 'message',
        sub: 'send',
        ts: at,
        from: session,
        to,
        seq: ev.seq,
        text: enc ? '' : clip(body),
        summary: str(input.summary) ?? undefined,
        encrypted: enc || undefined,
        sent: true,
      })
      continue
    }

    if (provider === 'claude-code') {
      if (ev.kind === 'tool_call' && tool) {
        if (CLAUDE_DELEGATE.has(tool.name) && input && (input.prompt != null || input.description != null || tool.name === 'Workflow')) {
          const dl: Delegation = {
            key: `${session}:${ev.seq}:d`,
            source: session,
            seq: ev.seq,
            ts: at,
            to: null,
            name: str(input.name),
            label: str(input.description) ?? str(input.subagent_type) ?? str(input.name),
            prompt: str(input.prompt),
            tool: tool.name,
            callId: tool.call_id || null,
            mode: null,
            background: input.run_in_background === true || str(input.team_name) != null,
            outcome: null,
            report: null,
          }
          d.delegations.push(dl)
          if (dl.callId) byCall.set(dl.callId, dl)
          if (dl.name) d.members.push({ id: null, name: dl.name, description: dl.label ?? undefined, team: str(input.team_name) ?? undefined })
        } else if (tool.name === 'TaskCreate' && input) {
          const rev: TaskRev = {
            group: '',
            id: '',
            source: session,
            seq: ev.seq,
            ts: at,
            subject: str(input.subject) ?? undefined,
            description: str(input.description) ?? undefined,
            status: 'pending',
          }
          if (tool.call_id) pendingCreate.set(tool.call_id, rev)
        } else if (tool.name === 'TaskUpdate' && input) {
          const id = str(input.taskId) ?? str(input.task_id) ?? (typeof input.taskId === 'number' ? String(input.taskId) : null)
          if (id) {
            d.tasks.push({
              group: `cc:${id}`,
              id,
              source: session,
              seq: ev.seq,
              ts: at,
              subject: str(input.subject) ?? undefined,
              description: str(input.description) ?? undefined,
              status: str(input.status) ?? undefined,
              owner: str(input.owner) ?? undefined,
              addBlockedBy: strs(input.addBlockedBy),
              addBlocks: strs(input.addBlocks),
            })
          }
        }
      }
      if (ev.kind === 'tool_result' && tool) {
        const dl = byCall.get(tool.call_id)
        const tur = obj(ev.ext?.toolUseResult)
        if (dl) {
          const spawned = ev.links?.find((l) => l.rel === 'spawned')
          if (spawned?.session) dl.to = spawned.session
          else if (str(tur?.agentId)) dl.to = `agent-${str(tur!.agentId)!.replace(/^agent-/, '')}`
          const status = str(tur?.status)
          // Workflow 的说明在结果里；它起的 agent 不逐个连边
          if (dl.tool === 'Workflow' && str(tur?.summary)) dl.label = str(tur!.summary)
          if (toolFailed(tool.status)) dl.outcome = 'failed'
          else if (status === 'async_launched' || status === 'teammate_spawned') dl.background = true
          else {
            dl.outcome = 'done'
            const body = textOf(tur?.content ?? tool.output).trim()
            if (body) dl.report = clip(body)
          }
          const m = dl.name ? d.members.find((x) => x.name === dl.name && x.id == null) : undefined
          if (m && dl.to) m.id = dl.to
        }
        const rev = pendingCreate.get(tool.call_id)
        if (rev) {
          pendingCreate.delete(tool.call_id)
          const task = obj(tur?.task)
          const id =
            str(task?.id) ?? (typeof task?.id === 'number' ? String(task.id) : null) ?? /#(\w+)/.exec(textOf(tool.output))?.[1] ?? null
          if (id && !toolFailed(tool.status)) d.tasks.push({ ...rev, id, group: `cc:${id}` })
        }
      }
      for (const l of ev.links ?? []) {
        if (l.rel === 'delivered_from') {
          const text = (ev.text ?? '').replace(COORDINATOR_RE, '')
          push({ kind: 'message', sub: 'coordinator', ts: at, from: l.session ?? null, to: session, seq: ev.seq, text: clip(text) })
        } else if (l.rel === 'notifies') {
          // 指回 Agent / Workflow 调用的通知算协作；否则 task-id 对得上家族里的 agent-<id> 才算（合并时判断），
          // 后台 shell 任务的通知两样都对不上，不算
          const dl = l.call_id ? byCall.get(l.call_id) : undefined
          const t = ev.text ?? ''
          const taskId = l.task ?? xmlTag(t, 'task-id')
          if (!dl && !taskId) continue
          const status = xmlTag(t, 'status')
          const settle: Settle | null =
            status === 'completed' ? 'finished' : status === 'failed' ? 'failed' : status === 'killed' || status === 'stopped' ? 'stopped' : null
          const result = xmlTag(t, 'result') ?? ''
          if (dl) {
            dl.outcome = settle === 'failed' ? 'failed' : settle === 'stopped' ? 'stopped' : 'done'
            if (result) dl.report = clip(result)
          }
          push({
            kind: 'report',
            sub: 'notify',
            ts: at,
            from: dl?.to ?? (dl ? null : `agent-${taskId}`),
            // Workflow 没有单个子会话，拿它的说明当发送方的名字
            fromName: dl && !dl.to ? (dl.label ?? undefined) : undefined,
            to: session,
            seq: ev.seq,
            text: clip(result),
            summary: xmlTag(t, 'summary') ?? undefined,
            settle,
            weak: !dl || undefined,
          })
        }
      }
      continue
    }

    if (provider === 'codex') {
      const p = obj(ev.ext?.payload)
      if (ev.kind === 'tool_call' && tool && CODEX_DELEGATE.has(tool.name)) {
        const msg = str(input?.message) ?? str(input?.prompt) ?? textOf(input?.items)
        const enc = msg != null && ENCRYPTED_RE.test(msg)
        const dl: Delegation = {
          key: `${session}:${ev.seq}:d`,
          source: session,
          seq: ev.seq,
          ts: at,
          to: null,
          name: str(input?.task_name),
          label: str(input?.task_name) ?? str(input?.agent_type),
          prompt: enc ? null : msg || null,
          tool: tool.name,
          callId: tool.call_id || null,
          mode: null,
          background: true,
          outcome: null,
          report: null,
          encrypted: enc || undefined,
        }
        d.delegations.push(dl)
        if (dl.callId) byCall.set(dl.callId, dl)
      } else if (ev.kind === 'tool_result' && tool && byCall.has(tool.call_id)) {
        const dl = byCall.get(tool.call_id)!
        if (toolFailed(tool.status)) dl.outcome = 'failed'
        try {
          const o = obj(JSON.parse(textOf(tool.output)))
          if (str(o?.task_name)) dl.name = str(o!.task_name)
        } catch {
          // 不是 JSON
        }
      }
      const item = obj(p?.item)
      if (p?.type === 'item_completed' && item?.type === 'SubAgentActivity') {
        const thread = str(item.agent_thread_id)
        const path = str(item.agent_path)
        if (thread && path) d.paths.push([path, thread])
        const dl = byCall.get(str(item.id) ?? '') ?? (thread ? d.delegations.find((x) => x.to === thread) : undefined)
        if (item.kind === 'started' && dl) {
          dl.to = thread
          if (path) dl.name = path
        } else if (item.kind === 'completed') {
          const target = dl ?? d.delegations.find((x) => x.name === path)
          if (target) target.outcome = target.outcome ?? 'done'
        }
      }
      if (p?.type === 'agent_message') {
        const raw = ev.text ?? ''
        const type = /^Message Type: (\S+)/m.exec(raw)?.[1] ?? null
        const at2 = raw.indexOf('Payload:\n')
        const body = (at2 >= 0 ? raw.slice(at2 + 9) : raw).trim()
        const enc = !body && Array.isArray(p.content)
        push({
          kind: type === 'FINAL_ANSWER' ? 'report' : 'message',
          sub: type === 'FINAL_ANSWER' ? 'final' : 'relay',
          ts: at,
          from: str(p.author),
          to: str(p.recipient) ?? session,
          seq: ev.seq,
          text: clip(body),
          encrypted: enc || undefined,
        })
      }
      continue
    }

    if (provider === 'dsh') {
      const ty = str(ev.ext?.type)
      const data = obj(ev.ext?.data)
      if (ev.kind === 'tool_call' && tool && DSH_DELEGATE.has(tool.name)) {
        const dl: Delegation = {
          key: `${session}:${ev.seq}:d`,
          source: session,
          seq: ev.seq,
          ts: at,
          to: null,
          name: str(input?.name),
          label: str(input?.description) ?? str(input?.name),
          prompt: str(input?.prompt),
          tool: tool.name,
          callId: tool.call_id || null,
          mode: null,
          background: true,
          outcome: null,
          report: null,
        }
        dshCalls.push(dl)
        continue
      }
      // 委派调用失败了（子会话没建起来）
      if (ev.kind === 'tool_result' && tool && toolFailed(tool.status)) {
        const c = dshCalls.find((x) => x.callId === tool.call_id)
        if (c) c.outcome = 'failed'
      }
      if (!ty || !data) continue
      if (ty === 'request/context' && typeof data.contextWindow === 'number') d.window = data.contextWindow
      else if (ty === 'subagent/descriptor') {
        if (data.mode === 'one-shot' || data.mode === 'continuable') d.mode = data.mode
        d.label = str(data.label)
      } else if (ty === 'subagent/catalog' || ty === 'team/member') {
        const member = obj(data.member)
        const childId = ty === 'subagent/catalog' ? str(data.childId) : str(member?.id)
        if (!childId || childId === data.teamId) continue
        if (member && str(member.name)) {
          const old = d.members.find((m) => m.id === childId)
          const m: TeamMember = {
            id: childId,
            name: str(member.name)!,
            description: str(member.description) ?? undefined,
            phase: str(member.phase) ?? undefined,
            team: str(data.teamId) ?? undefined,
          }
          if (old) Object.assign(old, m)
          else d.members.push(m)
        }
        let dl = d.delegations.find((x) => x.to === childId)
        if (!dl) {
          const label = str(data.label) ?? str(member?.description)
          const name = str(member?.name)
          // 对上发起这次委派的工具调用，好拿到 prompt
          const i = dshCalls.findIndex((c) => (name && c.name === name) || (label && c.label === label))
          const call = i >= 0 ? dshCalls.splice(i, 1)[0] : undefined
          const created = typeof data.childCreatedAt === 'number' ? data.childCreatedAt : at
          dl = {
            key: `${session}:${ev.seq}:d`,
            source: session,
            seq: call?.seq ?? ev.seq,
            ts: call?.ts ?? created,
            to: childId,
            name: name ?? call?.name ?? null,
            label: label ?? call?.label ?? null,
            prompt: call?.prompt ?? null,
            tool: call?.tool ?? ty,
            callId: call?.callId ?? null,
            mode: null,
            background: true,
            outcome: null,
            report: null,
          }
          d.delegations.push(dl)
        }
        const mode = ty === 'subagent/catalog' ? data.mode : 'continuable'
        if (mode === 'one-shot' || mode === 'continuable' || mode === 'external' || mode === 'unknown') {
          if (ty === 'subagent/catalog' || !dl.mode) dl.mode = mode
        }
      } else if (ty === 'team/task') {
        const task = obj(data.task)
        const id = str(task?.id)
        if (task && id) {
          d.tasks.push({
            group: `dsh:${str(data.teamId) ?? session}:${id}`,
            id,
            source: session,
            seq: ev.seq,
            ts: at,
            subject: str(task.subject) ?? undefined,
            description: typeof task.description === 'string' ? task.description : undefined,
            status: str(task.status) ?? undefined,
            owner: str(task.ownerId),
            blockedBy: strs(task.blockedBy),
            writeScopes: strs(task.writeScopes),
          })
        }
      } else if (ty === 'user/message') {
        const src = obj(data.source)
        const kind = str(src?.kind)
        const text = ev.text ?? ''
        if (kind === 'agent-message' || kind === 'team-message' || kind === 'coordinator') {
          const re = kind === 'team-message' ? DSH_TEAM_RE : DSH_AGENT_RE
          const m = re.exec(text)
          const body = text.replace(re, '')
          push({
            kind: 'message',
            sub: kind === 'team-message' ? 'mailbox' : kind === 'coordinator' ? 'coordinator' : 'relay',
            ts: at,
            from: str(src?.senderSessionId) ?? str(src?.senderId),
            fromName: str(src?.senderName) ?? m?.[1] ?? undefined,
            to: session,
            seq: ev.seq,
            text: clip(body),
          })
        } else if (kind === 'subagent-settled') {
          const summary = str(src?.summary)?.trim() ?? ''
          const { report, structured } = splitSettled(summary, text)
          push({
            kind: 'report',
            sub: 'settled',
            ts: at,
            from: str(src?.senderSessionId),
            to: session,
            seq: ev.seq,
            text: clip(report),
            summary: summary || undefined,
            settle: summary ? settleOf(summary) : null,
            structured: structured ? clip(structured) : undefined,
          })
        } else if (kind === 'subagent-report') {
          const m = DSH_REPORT_RE.exec(text)
          push({
            kind: 'report',
            sub: 'report',
            ts: at,
            from: str(src?.senderSessionId) ?? m?.[1] ?? null,
            to: session,
            seq: ev.seq,
            text: clip(text.replace(DSH_REPORT_RE, '')),
          })
        }
      }
    }
  }
  // 没对上 catalog 的 dsh 委派调用（子会话还没建、或日志被截断）也留着
  for (const c of dshCalls) d.delegations.push(c)
  return d
}

// ---- 缓存与加载 ----

export type CollabSlot = CollabDigest | 'loading' | 'error'
/** 各会话抽出的协作记录，跨组件实例留着；键同 fleetCache（id@mtime） */
export const collabCache = reactive(new Map<string, CollabSlot>())

const tick = () => new Promise<void>((r) => setTimeout(r, 0))

/**
 * worker 算好的卡片攒一会儿再一起写进缓存：一个家族上百个子会话，每回来一个就写一次，
 * Agent 网络卡就跟着重画上百次（每次一两百毫秒），主线程一直被占着
 */
const FLUSH_MS = 500
let pendingWrites: (() => void)[] = []
let flushTimer: ReturnType<typeof setTimeout> | null = null
function writeSoon(write: () => void) {
  pendingWrites.push(write)
  flushTimer ??= setTimeout(() => {
    flushTimer = null
    const list = pendingWrites
    pendingWrites = []
    for (const w of list) w()
  }, FLUSH_MS)
}

/**
 * 拉家族里各会话的事件：没有卡片数字的折一遍写进 fleetCache，没有协作记录的扫一遍写进 collabCache。
 * 每次最多同时拉 4 个；error 的每次挂载重试一回（retried 由调用方按挂载持有）。
 * 当前会话的数字由 ContextView 折好，这里只补协作记录；给了 currentEvents 就不再拉。
 */
export async function loadFamily(
  cards: FamilyCard[],
  opts: {
    signal: AbortSignal
    retried: Set<string>
    currentEvents?: Event[] | null
    /** 别的会话交给它算（拉事件、折叠、扫协作记录都不在主线程）；不给就在这里算 */
    offload?: (job: FleetJob, signal: AbortSignal) => Promise<FleetJobResult>
  },
): Promise<void> {
  const { signal, retried } = opts
  // 还是 loading 的是上一轮被取消的，重新排上
  const missing = (s: unknown) => s === undefined || s === 'loading'
  const queue: { card: FamilyCard; stats: boolean; collab: boolean }[] = []
  for (const c of cards) {
    const k = cardKey(c.info)
    const fs = c.role === 'current' ? null : fleetCache.get(k)
    const cs = collabCache.get(k)
    let stats = c.role !== 'current' && missing(fs)
    let collab = missing(cs)
    // 同一张卡的两个缓存共用一次重试机会
    if ((fs === 'error' || cs === 'error') && !retried.has(k)) {
      retried.add(k)
      stats ||= fs === 'error'
      collab ||= cs === 'error'
    }
    if (!stats && !collab) continue
    if (stats) fleetCache.set(k, 'loading')
    if (collab) collabCache.set(k, 'loading')
    queue.push({ card: c, stats, collab })
  }
  // 当前会话优先，事件在手就直接扫
  queue.sort((a, b) => (a.card.role === 'current' ? -1 : b.card.role === 'current' ? 1 : 0))
  const pricing = ensurePricing()
  const worker = async () => {
    while (queue.length && !signal.aborted) {
      const job = queue.shift()!
      const { info } = job.card
      const k = cardKey(info)
      try {
        const inHand = job.card.role === 'current' && opts.currentEvents
        if (!inHand && opts.offload) {
          const r = await opts.offload({ id: info.id, session: info.session, provider: info.provider, stats: job.stats, collab: job.collab }, signal)
          if (signal.aborted) return
          writeSoon(() => {
            if (job.stats && r.stats) fleetCache.set(k, r.stats)
            if (job.collab && r.collab) collabCache.set(k, markRaw(r.collab))
          })
          continue
        }
        const evs = inHand ? opts.currentEvents! : await api.events(info.id, signal)
        if (signal.aborted) return
        if (job.stats) {
          await pricing
          const f = foldContext(evs)
          fleetCache.set(k, fleetStats(f, summarize(f)))
        }
        if (job.collab) {
          // 大会话扫起来要一会儿，先让出主线程
          await tick()
          collabCache.set(k, markRaw(scanCollab(info.session, info.provider, evs)))
        }
      } catch {
        if (signal.aborted) return
        if (job.stats) fleetCache.set(k, 'error')
        if (job.collab) collabCache.set(k, 'error')
      }
      await tick()
    }
  }
  await Promise.all([worker(), worker(), worker(), worker()])
}

// ---- 合并：整个家族的视图 ----

export interface FleetAgent {
  id: string
  info: SessionInfo
  role: FamilyCard['role']
  depth: number
  parent: string | null
  /** 分支色槽：当前会话的每个直接子 agent 一个槽，子孙沿用；父会话和当前是 -1 */
  lineage: number
  name: string
  running: boolean
  mode: 'one-shot' | 'continuable' | null
  window: number | null
  firstTs: number | null
  lastTs: number | null
  /** 这张卡的协作记录读到没有 */
  slot: 'ok' | 'loading' | 'error'
}

export interface FleetRecord extends CollabRecord {
  fromId: string | null
  toId: string | null
  /** 时间轴上画在哪条泳道 */
  lane: string
  /** 关联的任务（合并键） */
  task?: string
  delegation?: FleetDelegation
}

export type DelegationStatus = 'running' | 'done' | 'failed' | 'stopped' | 'idle'

export interface FleetDelegation extends Delegation {
  fromId: string
  toId: string | null
  status: DelegationStatus
  settle: Settle | null
}

export type TaskStatus = 'pending' | 'in_progress' | 'completed' | 'deleted' | (string & {})

export interface FleetTask {
  key: string
  kind: 'team' | 'delegation'
  id: string
  subject: string
  description: string
  status: TaskStatus
  /** 归一后的筛选状态 */
  bucket: 'pending' | 'in_progress' | 'completed'
  owner: string | null
  ownerId: string | null
  /** 直接依赖 */
  blockedBy: string[]
  /** 依赖链（传递闭包，近的在前） */
  chain: string[]
  blocked: boolean
  writeScopes: string[]
  source: string
  seq: number
  ts: number
  updated: number
  revisions: { ts: number; seq: number; source: string; status?: string }[]
  report: string | null
  delegation?: FleetDelegation
}

export interface TaskMark {
  key: string
  task: string
  label: string
  ts: number
  lane: string
  status?: string
}

export interface FleetModel {
  agents: FleetAgent[]
  byId: Map<string, FleetAgent>
  records: FleetRecord[]
  delegations: FleetDelegation[]
  tasks: FleetTask[]
  marks: TaskMark[]
  range: [number, number] | null
  /** 还在读的卡片数 */
  pending: number
}

/** 解析端点：会话 id 的各种写法、成员名、Codex 路径 → 家族里的会话 id */
function resolver(agents: FleetAgent[], digests: CollabDigest[]) {
  const ids = new Set(agents.map((a) => a.id))
  const alias = new Map<string, string>()
  const put = (k: string | null | undefined, id: string) => {
    if (k && ids.has(id) && !alias.has(k)) alias.set(k, id)
  }
  for (const a of agents) {
    put(a.id, a.id)
    put(a.id.replace(/^(session-|agent-)/, ''), a.id)
  }
  const exact = (k: string): string | null => {
    if (ids.has(k)) return k
    for (const v of [`session-${k}`, `agent-${k}`, k.replace(/^(session-|agent-)/, '')]) if (ids.has(v)) return v
    return null
  }
  for (const d of digests) {
    for (const m of d.members) {
      const id = m.id ? exact(m.id) : null
      if (id) put(m.name, id)
      if (m.team) {
        const lead = exact(m.team)
        if (lead) put(`lead@${m.team}`, lead)
        if (lead) put('lead', lead)
      }
    }
    for (const [path, thread] of d.paths) {
      const id = exact(thread)
      if (id) put(path, id)
    }
    for (const dl of d.delegations) if (dl.name && dl.to) {
      const id = exact(dl.to)
      if (id) put(dl.name, id)
    }
  }
  // Codex 的根路径：家族里最顶上的 Codex 会话
  const rootCodex = agents.find((a) => a.info.provider === 'codex' && (!a.parent || !ids.has(a.parent)))
  if (rootCodex) put('/root', rootCodex.id)
  return (k: string | null | undefined): string | null => (k ? (exact(k) ?? alias.get(k) ?? null) : null)
}

const norm = (s: string) => s.replace(/\s+/g, ' ').trim().slice(0, 80)

function lineageOf(cards: FamilyCard[]): Map<string, number> {
  const m = new Map<string, number>()
  const current = cards.find((c) => c.role === 'current')?.info.session
  let slot = 0
  for (const c of cards) {
    if (c.role !== 'child') m.set(c.info.session, -1)
    else if (c.parent === current) m.set(c.info.session, slot++)
    else m.set(c.info.session, m.get(c.parent ?? '') ?? -1)
  }
  return m
}

const bucketOf = (s: string): FleetTask['bucket'] => (s === 'in_progress' ? 'in_progress' : s === 'completed' || s === 'deleted' ? 'completed' : 'pending')

export function buildFleet(cards: FamilyCard[], slot: (c: FamilyCard) => CollabSlot | undefined, now = Date.now()): FleetModel {
  const lineage = lineageOf(cards)
  const digests = new Map<string, CollabDigest>()
  let pending = 0
  const agents: FleetAgent[] = cards.map((c) => {
    const s = slot(c)
    const d = typeof s === 'object' ? s : null
    if (d) digests.set(c.info.session, d)
    if (s === undefined || s === 'loading') pending++
    return {
      id: c.info.session,
      info: c.info,
      role: c.role,
      depth: c.depth,
      parent: c.parent,
      lineage: lineage.get(c.info.session) ?? -1,
      name: (c.role === 'child' ? (d?.label ?? sessionTitle(c.info)) : (sessionTitle(c.info) ?? d?.label)) ?? c.info.session,
      running: isRunning(c.info, now),
      mode: d?.mode ?? null,
      window: d?.window ?? null,
      firstTs: d?.firstTs ?? null,
      lastTs: d?.lastTs ?? null,
      slot: d ? 'ok' : s === 'error' ? 'error' : 'loading',
    }
  })
  const byId = new Map(agents.map((a) => [a.id, a]))
  const list = [...digests.values()]
  const resolve = resolver(agents, list)

  // 委派
  const delegations: FleetDelegation[] = []
  for (const d of list) {
    for (const dl of d.delegations) {
      const toId = resolve(dl.to) ?? resolve(dl.name)
      const child = toId ? digests.get(toId) : undefined
      const kid = toId ? byId.get(toId) : undefined
      delegations.push({
        ...dl,
        mode: dl.mode ?? child?.mode ?? null,
        fromId: d.session,
        toId,
        status: dl.outcome === 'failed' ? 'failed' : dl.outcome === 'stopped' ? 'stopped' : dl.outcome === 'done' ? 'done' : kid?.running ? 'running' : 'idle',
        settle: null,
        report: dl.report,
      })
    }
  }

  // 子 agent 的名字：委派时起的名字（dsh 成员名、Claude teammate 名、Codex 路径的最后一段），其次是委派说明
  const named = new Set<string>()
  for (const dl of delegations) {
    const a = dl.toId ? byId.get(dl.toId) : undefined
    if (!a || a.role !== 'child' || named.has(a.id)) continue
    named.add(a.id)
    a.name = dl.name?.split('/').pop() || dl.label || a.name
  }

  // 记录：解析端点；发送方的记录在接收方也有时去掉
  const raw: FleetRecord[] = []
  for (const d of list) {
    for (const r of d.records) {
      const fromId = resolve(r.from) ?? (r.fromName ? resolve(r.fromName) : null)
      if (r.weak && !fromId) continue
      let toId = resolve(r.to)
      if (!toId && r.sent && r.to && PARENT_ALIASES.has(r.to.toLowerCase())) {
        const p = byId.get(d.session)?.parent
        toId = p && byId.has(p) ? p : null
      }
      raw.push({ ...r, fromId, toId, lane: fromId ?? r.source })
    }
  }
  const received = new Set(raw.filter((r) => !r.sent).map((r) => `${r.fromId}>${r.toId}>${norm(r.text)}`))
  const records = raw.filter((r) => !r.sent || !r.toId || !received.has(`${r.fromId}>${r.toId}>${norm(r.text)}`))

  // 结算 / 回报回填到委派上
  for (const r of records) {
    if (r.kind !== 'report' || !r.fromId) continue
    const dl = [...delegations].reverse().find((x) => x.toId === r.fromId && x.ts <= r.ts + 1000)
    if (!dl) continue
    if (r.settle) dl.settle = r.settle
    if (r.settle === 'failed') dl.status = 'failed'
    else if (r.settle === 'stopped' || r.settle === 'outOfRoom' || r.settle === 'declined') dl.status = 'stopped'
    // 结算 / 最终回答算收尾；dsh 的中途汇报（subagent-report）不改状态
    else if (r.settle || r.sub === 'settled' || r.sub === 'final' || r.sub === 'notify') dl.status = 'done'
    if (r.text && (!dl.report || r.sub === 'settled' || r.sub === 'final')) dl.report = r.text
  }
  for (const dl of delegations) {
    if (!dl.report && dl.toId && dl.status !== 'running') dl.report = digests.get(dl.toId)?.lastReply ?? null
    records.push({
      key: `${dl.key}:r`,
      kind: 'delegation',
      sub: dl.tool,
      ts: dl.ts,
      from: dl.source,
      to: dl.to,
      source: dl.source,
      seq: dl.seq,
      text: dl.prompt ?? '',
      summary: dl.label ?? undefined,
      encrypted: dl.encrypted,
      fromId: dl.fromId,
      toId: dl.toId,
      lane: dl.fromId,
      delegation: dl,
    })
  }

  // 团队任务：按合并键收修订
  const groups = new Map<string, TaskRev[]>()
  for (const d of list) for (const t of d.tasks) {
    const g = groups.get(t.group)
    if (g) g.push(t)
    else groups.set(t.group, [t])
  }
  const tasks: FleetTask[] = []
  const blocksAdd = new Map<string, Set<string>>()
  for (const revs of groups.values()) for (const r of revs) for (const b of r.addBlocks ?? []) {
    const k = r.group.replace(/:[^:]*$/, `:${b}`)
    const s = blocksAdd.get(k) ?? new Set()
    s.add(r.id)
    blocksAdd.set(k, s)
  }
  for (const [key, revs] of groups) {
    revs.sort((a, b) => a.ts - b.ts || a.seq - b.seq)
    let subject = ''
    let description = ''
    let status: string = 'pending'
    let owner: string | null = null
    let blockedBy: string[] | null = null
    const extra = new Set<string>(blocksAdd.get(key) ?? [])
    let writeScopes: string[] = []
    for (const r of revs) {
      if (r.subject) subject = r.subject
      if (r.description != null) description = r.description
      if (r.status) status = r.status
      if (r.owner !== undefined) owner = r.owner
      if (r.blockedBy) blockedBy = r.blockedBy
      for (const b of r.addBlockedBy ?? []) extra.add(b)
      if (r.writeScopes) writeScopes = r.writeScopes
    }
    const first = revs[0]!
    const last = revs.at(-1)!
    const ownerId = resolve(owner)
    tasks.push({
      key,
      kind: 'team',
      id: first.id,
      subject: subject || first.id,
      description,
      status,
      bucket: bucketOf(status),
      owner,
      ownerId,
      blockedBy: [...new Set([...(blockedBy ?? []), ...extra])],
      chain: [],
      blocked: false,
      writeScopes,
      source: first.source,
      seq: first.seq,
      ts: first.ts,
      updated: last.ts,
      revisions: revs.map((r) => ({ ts: r.ts, seq: r.seq, source: r.source, status: r.status })),
      report: status === 'completed' && ownerId ? (digests.get(ownerId)?.lastReply ?? null) : null,
    })
  }
  // 依赖链与是否被挡
  const byTaskId = new Map<string, FleetTask>()
  for (const t of tasks) byTaskId.set(`${t.key.replace(/:[^:]*$/, '')}:${t.id}`, t)
  for (const t of tasks) {
    const scope = t.key.replace(/:[^:]*$/, '')
    const seen = new Set<string>()
    const q = [...t.blockedBy]
    for (let i = 0; i < q.length; i++) {
      const id = q[i]!
      if (seen.has(id) || id === t.id) continue
      seen.add(id)
      q.push(...(byTaskId.get(`${scope}:${id}`)?.blockedBy ?? []))
    }
    t.chain = [...seen]
    t.blocked = t.bucket !== 'completed' && t.blockedBy.some((id) => byTaskId.get(`${scope}:${id}`)?.bucket !== 'completed')
  }
  // 委派也作为工作项
  for (const dl of delegations) {
    const st = dl.status === 'running' ? 'in_progress' : dl.status === 'idle' ? 'pending' : 'completed'
    tasks.push({
      key: `del:${dl.key}`,
      kind: 'delegation',
      id: dl.name ?? dl.label ?? dl.tool,
      subject: dl.label ?? dl.name ?? dl.tool,
      description: dl.prompt ?? '',
      status: dl.status,
      bucket: st,
      owner: dl.to,
      ownerId: dl.toId,
      blockedBy: [],
      chain: [],
      blocked: false,
      writeScopes: [],
      source: dl.source,
      seq: dl.seq,
      ts: dl.ts,
      updated: dl.ts,
      revisions: [],
      report: dl.report,
      delegation: dl,
    })
  }
  tasks.sort((a, b) => a.ts - b.ts)

  // 任务标记：每次修订一个，画在 owner（没有就记录者）的泳道上
  const marks: TaskMark[] = []
  for (const t of tasks) {
    if (t.kind !== 'team') continue
    const short = t.id.replace(/^task-/, '')
    for (const r of t.revisions) marks.push({ key: `${t.key}@${r.seq}:${r.source}`, task: t.key, label: short, ts: r.ts, lane: t.ownerId ?? r.source, status: r.status })
  }
  // 记录挂到任务上：正文里提到任务号（dsh 的 task-3、Claude 的 #3）
  const team = tasks.filter((t) => t.kind === 'team')
  if (team.length) {
    for (const r of records) {
      if (!r.text) continue
      const hit = team.find((t) => (/^task-/.test(t.id) ? r.text.includes(t.id) : new RegExp(`#${t.id}\\b`).test(r.text)))
      if (hit) r.task = hit.key
    }
  }
  for (const t of tasks) if (t.delegation) {
    for (const r of records) if (r.delegation === t.delegation) r.task = t.key
  }

  records.sort((a, b) => a.ts - b.ts || a.seq - b.seq)
  let lo = Infinity
  let hi = -Infinity
  for (const r of records) {
    if (r.ts < lo) lo = r.ts
    if (r.ts > hi) hi = r.ts
  }
  for (const a of agents) {
    if (a.firstTs != null && a.firstTs < lo) lo = a.firstTs
    if (a.lastTs != null && a.lastTs > hi) hi = a.lastTs
  }
  return {
    agents,
    byId,
    records,
    delegations,
    tasks,
    marks,
    range: Number.isFinite(lo) && Number.isFinite(hi) ? [lo, Math.max(hi, lo + 1)] : null,
    pending,
  }
}

/** 选中某个 agent 时，这条记录算不算它的 */
export function touches(r: FleetRecord, agent: string): boolean {
  return r.fromId === agent || r.toId === agent || (r.fromId == null && r.source === agent)
}

export function taskTouches(t: FleetTask, agent: string): boolean {
  return t.ownerId === agent || t.source === agent || t.delegation?.fromId === agent
}

/** 分支色：0..7 用分类色槽，超出和 -1（父 / 当前）用中性色 */
export function lineageColor(i: number, theme: { cat: string[]; other: string; accent: string }): string {
  if (i < 0) return theme.accent
  return theme.cat[i] ?? theme.other
}
