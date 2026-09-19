// 把 Event[] 折成 turn → step → item 的树。只做分组和 tool_call/tool_result 配对，不丢事件：
// request_header / meta 之类默认不进 item（平铺模式直接看 Event[]）。

import type { Event, Usage } from '../api'

/** step 里的一条：一个事件，tool_call 附带配对到的 tool_result */
export interface Item {
  ev: Event
  /** ev 是 tool_call 时，配对到的结果 */
  result?: Event
}

export interface Step {
  key: string
  id: string
  n: number | null
  started?: Event
  completed?: Event
  items: Item[]
  /** 这一步的 usage：step_completed 的，否则同 step 最后一条带 usage 的事件（Claude Code 同 requestId 各行重复带同一份） */
  usage: Usage | null
  model: string | null
  firstSeq: number
  firstTs?: string
  lastTs?: string
}

export type TurnChild = { type: 'step'; step: Step } | { type: 'item'; item: Item }

export interface Turn {
  key: string
  id: string
  n: number | null
  started?: Event
  completed?: Event
  children: TurnChild[]
  /** user 输入：turn_started 的 text，否则 turn 内第一条 role=user 的 message */
  userText: string | null
  /** turn_completed 上显式给的 usage */
  usage: Usage | null
  /** 没有显式 usage 时按 step 求和 */
  usageSum: Usage | null
  model: string | null
  inferred: boolean
  firstSeq: number
  firstTs?: string
  lastTs?: string
  stepCount: number
  toolCount: number
}

export type Block = { type: 'turn'; turn: Turn } | { type: 'loose'; items: Item[]; firstSeq: number }

export interface Grouped {
  blocks: Block[]
  /** call_id → tool_call / tool_result，跨 step 找 */
  calls: Map<string, { call?: Event; result?: Event }>
  /** 被过滤掉没进树的事件数（request_header / meta……） */
  hidden: number
}

/** 分组视图里不当成 item 显示的 kind */
const STRUCTURAL = new Set(['turn_started', 'turn_completed', 'step_started', 'step_completed'])
const HIDDEN = new Set(['request_header', 'meta'])

export function isHiddenInTree(ev: Event): boolean {
  if (HIDDEN.has(ev.kind)) return true
  // pfh 的 tool 角色 message_committed（内容 tool_result 已带）
  if (ev.kind === 'message' && ev.role === 'tool') return true
  return false
}

function addUsage(a: Usage | null, b: Usage | null | undefined): Usage | null {
  if (!b) return a
  const out: Usage = { ...(a ?? {}) }
  for (const k of ['input', 'output', 'cache_read', 'cache_write'] as const) {
    const v = b[k]
    if (typeof v === 'number') out[k] = (typeof out[k] === 'number' ? (out[k] as number) : 0) + v
  }
  return out
}

export function groupEvents(events: Event[]): Grouped {
  const blocks: Block[] = []
  const turns = new Map<string, Turn>()
  const calls = new Map<string, { call?: Event; result?: Event }>()
  let hidden = 0

  // 先把配对做完，这样 tool_result 到来时能找到 call
  for (const ev of events) {
    if (!ev.tool) continue
    const slot = calls.get(ev.tool.call_id) ?? {}
    if (ev.kind === 'tool_call' && !slot.call) slot.call = ev
    else if (ev.kind === 'tool_result' && !slot.result) slot.result = ev
    calls.set(ev.tool.call_id, slot)
  }

  const touch = (t: { firstTs?: string; lastTs?: string }, ts?: string) => {
    if (!ts) return
    if (!t.firstTs) t.firstTs = ts
    t.lastTs = ts
  }

  for (const ev of events) {
    const turnId = ev.turn?.id
    if (turnId == null) {
      if (STRUCTURAL.has(ev.kind) || isHiddenInTree(ev)) {
        hidden++
        continue
      }
      const last = blocks[blocks.length - 1]
      const item = { ev }
      if (last && last.type === 'loose') last.items.push(item)
      else blocks.push({ type: 'loose', items: [item], firstSeq: ev.seq })
      continue
    }

    let turn = turns.get(turnId)
    if (!turn) {
      turn = {
        key: `t:${turnId}`,
        id: turnId,
        n: ev.turn?.n ?? null,
        children: [],
        userText: null,
        usage: null,
        usageSum: null,
        model: null,
        inferred: ev.inferred?.includes('turn') ?? false,
        firstSeq: ev.seq,
        stepCount: 0,
        toolCount: 0,
      }
      turns.set(turnId, turn)
      blocks.push({ type: 'turn', turn })
    }
    touch(turn, ev.ts)
    if (turn.n == null && ev.turn?.n != null) turn.n = ev.turn.n
    if (!turn.model && ev.model) turn.model = ev.model

    if (ev.kind === 'turn_started') {
      turn.started = ev
      if (ev.text && turn.userText == null) turn.userText = ev.text
      continue
    }
    if (ev.kind === 'turn_completed') {
      turn.completed = ev
      if (ev.usage) turn.usage = ev.usage
      continue
    }

    const stepId = ev.step?.id
    let step: Step | undefined
    if (stepId != null) {
      const last = turn.children[turn.children.length - 1]
      // 同一 step 的事件在 pfh / Claude Code 里都是连续的；不连续就再开一段，保持原顺序
      if (last && last.type === 'step' && last.step.id === stepId) step = last.step
      else {
        step = {
          key: `s:${turnId}:${stepId}:${ev.seq}`,
          id: stepId,
          n: ev.step?.n ?? null,
          items: [],
          usage: null,
          model: null,
          firstSeq: ev.seq,
        }
        turn.children.push({ type: 'step', step })
        turn.stepCount++
      }
      touch(step, ev.ts)
      if (step.n == null && ev.step?.n != null) step.n = ev.step.n
      if (!step.model && ev.model) step.model = ev.model
      if (ev.usage) {
        // step_completed 是权威；否则同 step 内最后一份
        if (ev.kind === 'step_completed' || step.completed == null) step.usage = ev.usage
      }
    }

    if (ev.kind === 'step_started') {
      if (step) step.started = ev
      continue
    }
    if (ev.kind === 'step_completed') {
      if (step) step.completed = ev
      continue
    }
    if (isHiddenInTree(ev)) {
      hidden++
      continue
    }

    if (ev.kind === 'tool_result' && ev.tool) {
      const slot = calls.get(ev.tool.call_id)
      // 有 call 的结果挂到 call 上；孤儿结果自己占一条
      if (slot?.call && slot.result === ev) continue
    }
    if (ev.kind === 'message' && ev.role === 'user' && turn.userText == null) turn.userText = ev.text ?? null

    const item: Item = { ev }
    if (ev.kind === 'tool_call' && ev.tool) {
      item.result = calls.get(ev.tool.call_id)?.result
      turn.toolCount++
    }
    if (step) step.items.push(item)
    else turn.children.push({ type: 'item', item })
  }

  for (const turn of turns.values()) {
    let sum: Usage | null = null
    for (const c of turn.children) if (c.type === 'step') sum = addUsage(sum, c.step.usage)
    turn.usageSum = sum
  }

  return { blocks, calls, hidden }
}

function clip(s: string, max: number): string {
  const t = s.replace(/\s+/g, ' ').trim()
  return t.length > max ? `${t.slice(0, max - 1)}…` : t
}

/** 事件的一行摘要（平铺视图 / 大纲用） */
export function summarize(ev: Event, max = 140): string {
  if (ev.tool) {
    const name = ev.tool.name || '(tool)'
    if (ev.kind === 'tool_call') return `${name} ${clip(safeJson(ev.tool.input), max)}`
    return `${name}${ev.tool.status ? ` [${ev.tool.status}]` : ''} ${clip(outputText(ev.tool.output), max)}`
  }
  if (ev.text) return clip(ev.text, max)
  if (ev.reasoning) return clip(`(reasoning) ${ev.reasoning}`, max)
  if (ev.kind === 'request_header') {
    const at = (ev.ext.attachment as { type?: string } | undefined)?.type
    return at ? `attachment: ${at}` : ''
  }
  if (ev.kind === 'meta') {
    const ty = ev.ext.type
    const sub = ev.ext.subtype
    return [ty, sub].filter((x) => typeof x === 'string').join('/')
  }
  return ''
}

function safeJson(v: unknown): string {
  if (v === undefined) return ''
  try {
    return JSON.stringify(v) ?? ''
  } catch {
    return String(v)
  }
}

/** tool output 的可读文本：字符串直接用；Claude Code 的 content block 数组拼 text；其他 JSON */
export function outputText(v: unknown): string {
  if (v == null) return ''
  if (typeof v === 'string') return v
  if (Array.isArray(v)) {
    const texts: string[] = []
    let allText = true
    for (const p of v) {
      const part = p as { type?: string; text?: unknown } | null
      if (part && typeof part === 'object' && part.type === 'text' && typeof part.text === 'string') {
        texts.push(part.text)
      } else {
        allText = false
        break
      }
    }
    if (allText && texts.length) return texts.join('\n')
  }
  return safeJson(v)
}

/** tool input 的一行摘要：常见工具挑最有信息量的字段，其他用紧凑 JSON */
export function inputSummary(input: unknown): string {
  if (input && typeof input === 'object' && !Array.isArray(input)) {
    const o = input as Record<string, unknown>
    const pick = (...keys: string[]) => {
      for (const k of keys) {
        const v = o[k]
        if (typeof v === 'string' && v.trim()) return v
      }
      return null
    }
    const s =
      pick('command') ??
      pick('pattern', 'query', 'url') ??
      pick('file_path', 'path', 'notebook_path') ??
      pick('prompt', 'description', 'content', 'text', 'message')
    if (s) return s
  }
  return safeJson(input)
}
