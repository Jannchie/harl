// 文件活动：从工具调用里认出读 / 写 / 搜索了哪些文件，按文件聚合。
// 规则取自 dsh-context 的 fileOps，扩到各家的工具名：Claude Code 的 Read / Edit / MultiEdit / Grep，
// Codex 的 apply_patch，dsh 的 str_replace_editor 和 run_code 里嵌套的调用。shell 命令不解析（猜不准）。

import type { Event } from '../api'

export type OpKind = 'read' | 'write' | 'search'

export interface FileOp {
  kind: OpKind
  path: string
  /** 工具调用（或 run_code 里嵌套调用）的 seq */
  seq: number
  /** 嵌套调用所属的外层调用的 seq；切到某一步时按它算 */
  parent?: number
  ts?: string
  tool: string
  added: number
  removed: number
  /** 读的窗口：从第几行、读了几行（est = 只是按 limit 估的） */
  window?: { start: number; count: number; est: boolean }
  /** 搜索命中数 */
  hits?: number
  /** path 不是文件，是搜索的 pattern */
  pattern?: boolean
  /** 搜索条件 */
  detail?: string
  err: boolean
  /** 结果事件，跳到上下文浏览器用 */
  result?: Event
  call: Event
  /** 嵌套在 run_code 里时，外层程序的描述 */
  program?: string
}

const IMAGE_EXT = /\.(png|jpe?g|gif|webp|bmp|svg|avif|ico|tiff?)$/i

/** 工具名 → 读 / 写 / 搜索；null 不是文件操作 */
function kindOf(name: string, args: Record<string, unknown>): OpKind | null {
  const n = name.toLowerCase()
  if (n === 'str_replace_editor' || n === 'str_replace_based_edit_tool') return args.command === 'view' ? 'read' : 'write'
  if (['read', 'read_file', 'read_image', 'view', 'notebookread', 'view_image'].includes(n)) return 'read'
  if (['write', 'write_file', 'edit', 'multiedit', 'notebookedit', 'create_file', 'apply_patch', 'edit_file'].includes(n)) return 'write'
  if (['grep', 'glob', 'search', 'find', 'list_dir', 'ls', 'file_search', 'grep_search'].includes(n)) return 'search'
  return null
}

function linesOf(s: unknown): number {
  if (typeof s !== 'string' || !s) return 0
  let n = 0
  for (let i = 0; i < s.length; i++) if (s.charCodeAt(i) === 10) n++
  return s.endsWith('\n') ? n : n + 1
}

function pathOf(name: string, a: Record<string, unknown>): { path: string; pattern: boolean } | null {
  const s = (k: string) => (typeof a[k] === 'string' && a[k] ? (a[k] as string) : null)
  const n = name.toLowerCase()
  if (n === 'grep' || n === 'glob' || n === 'search' || n === 'grep_search' || n === 'file_search') {
    const p = s('path') ?? s('cwd')
    if (p) return { path: p, pattern: false }
    const pat = s('pattern') ?? s('query')
    return pat ? { path: pat, pattern: true } : null
  }
  const p = s('file_path') ?? s('filePath') ?? s('path') ?? s('notebook_path') ?? s('target_file')
  return p ? { path: p, pattern: false } : null
}

function deltaOf(name: string, a: Record<string, unknown>): { added: number; removed: number } {
  const n = name.toLowerCase()
  if (n === 'edit' || n === 'edit_file') return { added: linesOf(a.new_string), removed: linesOf(a.old_string) }
  if (n === 'multiedit' && Array.isArray(a.edits)) {
    let added = 0
    let removed = 0
    for (const e of a.edits as Record<string, unknown>[]) {
      added += linesOf(e?.new_string)
      removed += linesOf(e?.old_string)
    }
    return { added, removed }
  }
  if (n === 'write' || n === 'write_file' || n === 'create_file') return { added: linesOf(a.content), removed: 0 }
  if (n === 'notebookedit') return { added: linesOf(a.new_source), removed: 0 }
  if (n === 'str_replace_editor' || n === 'str_replace_based_edit_tool') {
    if (a.command === 'str_replace') return { added: linesOf(a.new_str), removed: linesOf(a.old_str) }
    if (a.command === 'insert') return { added: linesOf(a.new_str), removed: 0 }
    if (a.command === 'create') return { added: linesOf(a.file_text), removed: 0 }
  }
  return { added: 0, removed: 0 }
}

/** apply_patch 的补丁：每个 `*** Update/Add/Delete File:` 一个写操作，+/- 行数 */
function patchOps(patch: string): { path: string; added: number; removed: number }[] {
  const out: { path: string; added: number; removed: number }[] = []
  let cur: { path: string; added: number; removed: number } | null = null
  for (const line of patch.split('\n')) {
    const m = /^\*\*\* (?:Update|Add|Delete) File: (.+)$/.exec(line)
    if (m) {
      cur = { path: m[1]!.trim(), added: 0, removed: 0 }
      out.push(cur)
      continue
    }
    if (!cur || line.startsWith('***') || line.startsWith('@@')) continue
    if (line.startsWith('+')) cur.added++
    else if (line.startsWith('-')) cur.removed++
  }
  return out
}

function argsOf(input: unknown): Record<string, unknown> {
  if (input && typeof input === 'object' && !Array.isArray(input)) return input as Record<string, unknown>
  if (typeof input === 'string') {
    try {
      const v = JSON.parse(input)
      if (v && typeof v === 'object') return v as Record<string, unknown>
    } catch {
      return { input }
    }
  }
  return {}
}

function opsOfCall(
  name: string,
  args: Record<string, unknown>,
  base: Omit<FileOp, 'kind' | 'path' | 'added' | 'removed' | 'tool'>,
  meta: Record<string, unknown> | null,
): FileOp[] {
  const n = name.toLowerCase()
  if (n === 'apply_patch') {
    const patch = typeof args.input === 'string' ? args.input : typeof args.patch === 'string' ? args.patch : ''
    return patchOps(patch).map((p) => ({ ...base, kind: 'write' as const, tool: name, ...p }))
  }
  const kind = kindOf(name, args)
  if (!kind) return []
  const target = pathOf(name, args)
  if (!target) return []
  const op: FileOp = { ...base, kind, tool: name, path: target.path, pattern: target.pattern || undefined, ...deltaOf(name, args) }
  if (kind === 'read') {
    const lines = Array.isArray(meta?.lines) ? (meta.lines as unknown[]).length : null
    const offset = typeof meta?.offset === 'number' ? meta.offset : typeof args.offset === 'number' ? args.offset : 1
    if (lines != null) op.window = { start: offset, count: lines, est: false }
    else if (typeof args.limit === 'number') op.window = { start: offset, count: args.limit, est: true }
  }
  if (kind !== 'search') return [op]
  op.detail = [args.pattern ?? args.query, args.include ?? args.glob].filter((x) => typeof x === 'string').join(' · ')
  // dsh 的结果 meta 列出了命中的文件：每个文件一个操作，带命中数（只信没截断的）
  const out: FileOp[] = [op]
  if (meta && meta.truncated === false) {
    const files = meta.shape === 'matches' && Array.isArray(meta.files) ? (meta.files as Record<string, unknown>[]) : []
    const paths = meta.shape === 'paths' && Array.isArray(meta.paths) ? (meta.paths as unknown[]) : []
    for (const f of files)
      if (typeof f.path === 'string') out.push({ ...op, path: f.path, pattern: undefined, hits: Array.isArray(f.matches) ? f.matches.length : undefined })
    for (const p of paths) if (typeof p === 'string') out.push({ ...op, path: p, pattern: undefined })
    if (out.length > 1 && out.slice(1).some((o) => o.path === op.path)) out.shift()
  }
  return out
}

/** 一个会话的所有文件操作，按 seq 排 */
export function collectFileOps(events: Event[], calls: Map<string, { call?: Event; result?: Event }>): FileOp[] {
  const ops: FileOp[] = []
  /** run_code 的嵌套调用先攒着，外层结果到了再算进来（它们属于那一步） */
  const nested = new Map<string, FileOp[]>()
  for (const ev of events) {
    const ext = ev.ext as Record<string, unknown>
    const ty = typeof ext.type === 'string' ? ext.type : ''
    if (ty === 'tool/ptc-dispatch' || ty === 'tool/code-dispatch') {
      const d = (ext.data ?? {}) as Record<string, unknown>
      const name = typeof d.name === 'string' ? d.name : ''
      const root = typeof d.rootCallId === 'string' ? d.rootCallId : ''
      const list = opsOfCall(name, argsOf(d.arguments), { seq: ev.seq, ts: ev.ts, err: d.isError === true, call: ev }, null)
      if (list.length) nested.set(root, [...(nested.get(root) ?? []), ...list])
      continue
    }
    if (ev.kind !== 'tool_call' || !ev.tool) continue
    const result = calls.get(ev.tool.call_id)?.result
    const err = result?.tool?.status === 'error' || result?.tool?.status === 'failed'
    const meta = ((result?.ext as Record<string, unknown> | undefined)?.data as Record<string, unknown> | undefined)?.meta as
      | Record<string, unknown>
      | undefined
    ops.push(...opsOfCall(ev.tool.name, argsOf(ev.tool.input), { seq: ev.seq, ts: ev.ts, err, result, call: ev }, meta ?? null))
  }
  // 嵌套调用挂到外层 run_code 上
  for (const [root, list] of nested) {
    const slot = calls.get(root)
    const parent = slot?.call
    const desc = (parent?.tool?.input as Record<string, unknown> | undefined)?.description
    for (const o of list) {
      o.parent = parent?.seq
      o.result ??= slot?.result
      if (typeof desc === 'string') o.program = desc
      ops.push(o)
    }
  }
  return ops.sort((a, b) => (a.parent ?? a.seq) - (b.parent ?? b.seq) || a.seq - b.seq)
}

export interface FileRow {
  path: string
  /** 文件名（路径最后一段） */
  name: string
  form: 'file' | 'image' | 'dir' | 'pattern'
  reads: number
  writes: number
  searches: number
  added: number
  removed: number
  hits: number
  errors: number
  lastSeq: number
  lastTs?: string
  ops: FileOp[]
}

/** 路径统一成 `/` 分隔、去掉 ./，大小写保留（展示用原样） */
function normPath(p: string): string {
  return p.replace(/\\/g, '/').replace(/^\.\//, '')
}

/** 按文件聚合；upTo 给了就只算外层 seq 在它之前的 */
export function aggregateOps(ops: FileOp[], upTo?: number): FileRow[] {
  const rows = new Map<string, FileRow>()
  for (const o of ops) {
    if (upTo != null && (o.parent ?? o.seq) >= upTo) continue
    const path = normPath(o.path)
    const key = path.toLowerCase()
    let r = rows.get(key)
    if (!r) {
      const name = path.replace(/\/$/, '').split('/').pop() || path
      r = {
        path,
        name,
        form: o.pattern ? 'pattern' : path.endsWith('/') ? 'dir' : IMAGE_EXT.test(path) || o.tool === 'read_image' ? 'image' : 'file',
        reads: 0,
        writes: 0,
        searches: 0,
        added: 0,
        removed: 0,
        hits: 0,
        errors: 0,
        lastSeq: 0,
        ops: [],
      }
      rows.set(key, r)
    }
    if (o.kind === 'read') r.reads++
    else if (o.kind === 'write') r.writes++
    else r.searches++
    r.added += o.added
    r.removed += o.removed
    r.hits += o.hits ?? 0
    if (o.err) r.errors++
    r.lastSeq = Math.max(r.lastSeq, o.seq)
    if (o.ts && (!r.lastTs || o.ts > r.lastTs)) r.lastTs = o.ts
    r.ops.push(o)
  }
  return [...rows.values()]
}
