// 最小 mock：按 API 契约返回合成数据，监听 127.0.0.1:7777（和 `harl serve` 默认一样，vite 代理直接指过来）。
// 事件形状照着 crates/harl-core/src/normalize/{claude,pfh}.rs 的输出写；只保证 UI 能跑，不追求字段全。
//   pnpm mock      # 起 mock
//   pnpm dev       # 另一个终端起前端

import http from 'node:http'

const PORT = Number(process.env.PORT ?? 7777)
const HOST = '127.0.0.1'

const t0 = Date.parse('2026-09-19T01:02:03.000Z')
const at = (sec) => new Date(t0 + sec * 1000).toISOString()
const hex = (seed) => {
  let h = 0x811c9dc5
  for (const c of String(seed)) h = Math.imul(h ^ c.charCodeAt(0), 0x01000193) >>> 0
  return (h.toString(16).padStart(8, '0') + h.toString(16).padStart(8, '0')).repeat(4)
}

// ---------- claude-code 主会话 ----------

function claudeMain() {
  const session = 'a1b2c3d4-0000-4000-8000-000000000001'
  const evs = []
  const raw = []
  let seq = 0
  let sec = 0
  const push = (ev, rawLine) => {
    ev.provider = 'claude-code'
    ev.session = session
    ev.seq = seq
    ev.prov = { chunk: hex(`${session}:${seq}`) }
    ev.ext ??= {}
    evs.push(ev)
    raw.push(JSON.stringify(rawLine ?? { type: ev.ext.type ?? ev.kind, uuid: `u-${seq}`, timestamp: ev.ts, _mock: true }))
    seq++
  }
  const usage = (i, o, cr = 0, cw = 0) => ({ input: i, output: o, cache_read: cr, cache_write: cw, service_tier: 'standard' })

  // meta: 斜杠命令回显
  push({ ts: at(sec++), kind: 'meta', text: '<command-name>/clear</command-name>', ext: { type: 'user', uuid: 'u-cmd', isMeta: false } })
  // request_header attachments
  push({ ts: at(sec++), kind: 'request_header', ext: { type: 'attachment', attachment: { type: 'environment', cwd: 'E:/code/harl', platform: 'win32' } } })

  // turn 1
  const T1 = 'prompt-0001'
  push({ ts: at(sec++), kind: 'turn_started', role: 'user', turn: { id: T1, n: 1 }, text: '看一下 src/event.rs，告诉我 Event 有哪些字段。', ext: { type: 'user', promptId: T1, uuid: 'u-t1' } })
  push({ ts: at(sec++), kind: 'request_header', turn: { id: T1, n: 1 }, inferred: ['step'], ext: { type: 'attachment', attachment: { type: 'model', model: 'claude-opus-5' } } })
  push({
    ts: at(sec++),
    kind: 'tool_call',
    role: 'assistant',
    turn: { id: T1, n: 1 },
    step: { id: 'req_001', n: 1 },
    inferred: ['step'],
    reasoning: '用户要看字段列表。先读文件，不要猜。',
    text: '我先读一下这个文件。',
    tool: { call_id: 'toolu_01', name: 'Read', input: { file_path: 'E:\\code\\harl\\src\\event.rs' } },
    model: 'claude-opus-5',
    usage: usage(1200, 85, 22000, 300),
    ext: { type: 'assistant', uuid: 'u-a1', message: { id: 'msg_01', stop_reason: 'tool_use' } },
  })
  push({
    ts: at(sec++),
    kind: 'tool_result',
    role: 'tool',
    turn: { id: T1, n: 1 },
    step: { id: 'req_001', n: 1 },
    inferred: ['step'],
    tool: { call_id: 'toolu_01', name: '', output: [{ type: 'text', text: '     1\t//! canonical 事件 v1\n     2\t\n     3\tpub struct Event {\n     4\t    pub provider: &\'static str,\n ...（省略 120 行）' }], status: 'completed' },
    ext: { type: 'user', promptId: T1, uuid: 'u-r1', toolUseResult: { type: 'text', file: { filePath: 'E:\\code\\harl\\src\\event.rs', numLines: 133 } } },
  })
  push({
    ts: at(sec++),
    kind: 'message',
    role: 'assistant',
    turn: { id: T1, n: 1 },
    step: { id: 'req_002', n: 2 },
    inferred: ['step'],
    text: '`Event` 的字段：\n\n| 字段 | 类型 | 说明 |\n|---|---|---|\n| `provider` | `&str` | harness 名 |\n| `session` | `String` | 会话 id |\n| `seq` | `u32` | 行号 |\n| `kind` | `Kind` | 事件种类 |\n| `turn` / `step` | `Option<Scope>` | 分组身份 |\n| `tool` | `Option<Tool>` | 工具调用 / 结果 |\n\n没映射的字段留在 `ext`，`prov.chunk` 指回原始行。',
    model: 'claude-opus-5',
    usage: usage(1500, 220, 23000, 0),
    ext: { type: 'assistant', uuid: 'u-a2', message: { id: 'msg_02', stop_reason: 'end_turn' } },
  })
  push({ ts: at(sec++), kind: 'turn_completed', turn: { id: T1, n: 1 }, ext: { type: 'system', subtype: 'turn_duration', durationMs: 5200 } })

  // turn 2: Agent 调用 + 后台通知 + 失败的 Bash
  const T2 = 'prompt-0002'
  push({ ts: at(sec++), kind: 'turn_started', role: 'user', turn: { id: T2, n: 2 }, text: '起一个子 agent 去搜一下 normalize 目录里有几个 provider，然后跑 cargo test。', ext: { type: 'user', promptId: T2, uuid: 'u-t2' } })
  push({
    ts: at(sec++),
    kind: 'tool_call',
    role: 'assistant',
    turn: { id: T2, n: 2 },
    step: { id: 'req_003', n: 1 },
    inferred: ['step'],
    tool: { call_id: 'toolu_02', name: 'Agent', input: { description: '数 provider', prompt: '在 src/normalize 下找 `pub static PROVIDER`，报告数量和名字。', subagent_type: 'Explore', run_in_background: true } },
    model: 'claude-opus-5',
    usage: usage(900, 120, 24000, 0),
    ext: { type: 'assistant', uuid: 'u-a3', message: { id: 'msg_03' } },
  })
  push({
    ts: at(sec++),
    kind: 'tool_result',
    role: 'tool',
    turn: { id: T2, n: 2 },
    step: { id: 'req_003', n: 1 },
    inferred: ['step'],
    tool: { call_id: 'toolu_02', name: '', output: 'Agent started in background. Task id: task-77.', status: 'completed' },
    links: [{ rel: 'spawned', session: 'agent-abc123', call_id: 'toolu_02' }],
    ext: { type: 'user', promptId: T2, uuid: 'u-r2', toolUseResult: { agentId: 'abc123', status: 'async_launched' } },
  })
  push({
    ts: at(sec++),
    kind: 'tool_call',
    role: 'assistant',
    turn: { id: T2, n: 2 },
    step: { id: 'req_004', n: 2 },
    inferred: ['step'],
    text: '子 agent 在跑，我先跑测试。',
    tool: { call_id: 'toolu_03', name: 'Bash', input: { command: 'cargo test -q 2>&1 | tail -20', description: 'Run the test suite', timeout: 120000 } },
    model: 'claude-opus-5',
    usage: usage(950, 60, 24500, 0),
    ext: { type: 'assistant', uuid: 'u-a4', message: { id: 'msg_04' } },
  })
  push({
    ts: at((sec += 12)),
    kind: 'tool_result',
    role: 'tool',
    turn: { id: T2, n: 2 },
    step: { id: 'req_004', n: 2 },
    inferred: ['step'],
    tool: { call_id: 'toolu_03', name: '', output: 'error[E0432]: unresolved import `crate::event::Prov`\n  --> src/normalize/pfh.rs:14:5\n\nerror: could not compile `harl` (lib test) due to 1 previous error', status: 'failed' },
    ext: { type: 'user', promptId: T2, uuid: 'u-r3', toolUseResult: 'Exit code 101' },
  })
  push({
    ts: at(sec++),
    kind: 'message',
    turn: { id: T2, n: 2 },
    step: { id: 'req_004', n: 2 },
    inferred: ['step'],
    text: '<task-notification>\n<task-id>task-77</task-id>\n<tool-use-id>toolu_02</tool-use-id>\n<status>completed</status>\n<summary>Agent completed: 2 providers (claude-code, pfh)</summary>\n</task-notification>',
    links: [{ rel: 'notifies', call_id: 'toolu_02', task: 'task-77' }],
    ext: { type: 'user', promptId: T2, uuid: 'u-n1', origin: { kind: 'task-notification' } },
  })
  push({
    ts: at(sec++),
    kind: 'message',
    role: 'user',
    turn: { id: T2, n: 2 },
    step: { id: 'req_004', n: 2 },
    inferred: ['step'],
    text: '测试先别管，只要 provider 数量。',
    ext: { type: 'user', promptId: T2, uuid: 'u-q1', promptSource: 'queued' },
  })
  push({
    ts: at(sec++),
    kind: 'message',
    role: 'assistant',
    turn: { id: T2, n: 2 },
    step: { id: 'req_005', n: 3 },
    inferred: ['step'],
    reasoning: 'cargo test 编译失败但用户说不管。子 agent 报告了 2 个 provider。',
    text: '两个 provider：`claude-code` 和 `pfh`。（`cargo test` 目前编译不过，`pfh.rs` 里 `Prov` 的 import 断了，按你说的先不管。）',
    model: 'claude-opus-5',
    usage: usage(1300, 90, 25000, 0),
    ext: { type: 'assistant', uuid: 'u-a5', message: { id: 'msg_05', stop_reason: 'end_turn' } },
  })
  push({ ts: at(sec++), kind: 'turn_completed', turn: { id: T2, n: 2 }, ext: { type: 'system', subtype: 'turn_duration', durationMs: 19000 } })

  // meta：system-reminder，不属于任何 turn
  push({ ts: at(sec++), kind: 'meta', text: '<system-reminder>The user opened the file src/store.rs in the IDE.</system-reminder>', ext: { type: 'user', uuid: 'u-m2', isMeta: true } })

  // turn 3：老日志风格，没有 promptId → turn 推断；还没结束
  const T3 = 'u-t3'
  push({ ts: at(sec++), kind: 'turn_started', role: 'user', turn: { id: T3, n: 3 }, inferred: ['turn'], text: '把 README 里"存储"那节翻译成英文。' + '\n'.repeat(2) + '要求：\n- 保留代码块\n- 术语 chunk / pack / tree 不翻', ext: { type: 'user', uuid: T3 } })
  push({
    ts: at(sec++),
    kind: 'message',
    role: 'assistant',
    turn: { id: T3, n: 3 },
    step: { id: 'req_006', n: 1 },
    inferred: ['turn', 'step'],
    text: '## Storage\n\nData directory is `$HARL_DATA_DIR` or `~/.harl`:\n\n```\nmachine-id\nraw/manifest.jsonl\nraw/packs/*.pack\nindex.db\n```\n\n- A **chunk** is one line (including `\\n`), addressed by blake3.\n- A **pack** is a series of zstd frames …',
    model: 'claude-opus-5',
    usage: usage(2100, 640, 26000, 0),
    ext: { type: 'assistant', uuid: 'u-a6', message: { id: 'msg_06', stop_reason: 'end_turn' } },
  })

  return { session, parent: null, evs, raw }
}

// ---------- claude-code 子会话 ----------

function claudeSub() {
  const session = 'agent-abc123'
  const parent = 'a1b2c3d4-0000-4000-8000-000000000001'
  const evs = []
  const raw = []
  let seq = 0
  let sec = 8
  const push = (ev) => {
    ev.provider = 'claude-code'
    ev.session = session
    ev.parent_session = parent
    ev.seq = seq
    ev.prov = { chunk: hex(`${session}:${seq}`) }
    ev.ext ??= {}
    evs.push(ev)
    raw.push(JSON.stringify({ type: ev.ext.type ?? ev.kind, timestamp: ev.ts, _mock: true }))
    seq++
  }
  const T = 'prompt-sub-1'
  push({ ts: at(sec++), kind: 'turn_started', role: 'user', turn: { id: T, n: 1 }, text: '在 src/normalize 下找 `pub static PROVIDER`，报告数量和名字。', ext: { type: 'user', promptId: T } })
  push({
    ts: at(sec++),
    kind: 'tool_call',
    role: 'assistant',
    turn: { id: T, n: 1 },
    step: { id: 'req_s1', n: 1 },
    inferred: ['step'],
    tool: { call_id: 'toolu_s1', name: 'Grep', input: { pattern: 'pub static PROVIDER', path: 'src/normalize', output_mode: 'content' } },
    model: 'claude-sonnet-5',
    usage: { input: 400, output: 40, cache_read: 8000 },
    ext: { type: 'assistant' },
  })
  push({
    ts: at(sec++),
    kind: 'tool_result',
    role: 'tool',
    turn: { id: T, n: 1 },
    step: { id: 'req_s1', n: 1 },
    inferred: ['step'],
    tool: { call_id: 'toolu_s1', name: '', output: [{ type: 'text', text: 'src/normalize/claude.rs:29:pub static PROVIDER: Provider = Provider {\nsrc/normalize/pfh.rs:19:pub static PROVIDER: Provider = Provider {' }], status: 'completed' },
    ext: { type: 'user', promptId: T },
  })
  push({
    ts: at(sec++),
    kind: 'message',
    role: 'assistant',
    turn: { id: T, n: 1 },
    step: { id: 'req_s2', n: 2 },
    inferred: ['step'],
    text: '2 个 provider：`claude-code`（claude.rs）和 `pfh`（pfh.rs）。',
    model: 'claude-sonnet-5',
    usage: { input: 500, output: 30, cache_read: 8200 },
    ext: { type: 'assistant' },
  })
  push({ ts: at(sec++), kind: 'turn_completed', turn: { id: T, n: 1 }, ext: { type: 'system', subtype: 'turn_duration', durationMs: 3000 } })
  return { session, parent, evs, raw }
}

// ---------- pfh 会话：显式 turn / step ----------

function pfhMain() {
  const session = 'pfh-9f8e7d6c'
  const evs = []
  const raw = []
  let seq = 0
  let sec = 100
  const push = (ev) => {
    ev.provider = 'pfh'
    ev.session = session
    ev.seq = seq
    ev.prov = { chunk: hex(`${session}:${seq}`) }
    ev.ext ??= {}
    evs.push(ev)
    raw.push(JSON.stringify({ version: 1, sessionId: session, runId: 'run-1', type: ev.ext.type ?? ev.kind, timestamp: ev.ts, _mock: true }))
    seq++
  }
  const T = 'turn-1'
  push({ ts: at(sec++), kind: 'meta', ext: { type: 'session_started', workspace: '/home/me/proj' } })
  push({ ts: at(sec++), kind: 'turn_started', role: 'user', turn: { id: T, n: 1 }, text: 'Add a --json flag to the status command.', ext: { type: 'turn_started', message: { role: 'user' } } })
  push({ ts: at(sec++), kind: 'step_started', turn: { id: T, n: 1 }, step: { id: 'step-1', n: 1 }, ext: { type: 'step_started' } })
  push({ ts: at(sec++), kind: 'request_header', turn: { id: T, n: 1 }, step: { id: 'step-1', n: 1 }, ext: { type: 'request_header', modelId: 'gpt-5', tools: ['read_file', 'edit_file', 'shell'], systemPromptHash: 'abc' } })
  push({
    ts: at(sec++),
    kind: 'message',
    role: 'assistant',
    turn: { id: T, n: 1 },
    step: { id: 'step-1', n: 1 },
    reasoning: 'Need to look at the CLI definition first.',
    text: "I'll look at how `status` is wired up.",
    ext: { type: 'message_committed', message: { role: 'assistant' } },
  })
  push({
    ts: at(sec++),
    kind: 'tool_call',
    role: 'assistant',
    turn: { id: T, n: 1 },
    step: { id: 'step-1', n: 1 },
    tool: { call_id: 'call_x1', name: 'read_file', input: { path: 'src/main.rs', start: 1, end: 80 } },
    ext: { type: 'tool_call_started' },
  })
  push({
    ts: at(sec++),
    kind: 'tool_result',
    role: 'tool',
    turn: { id: T, n: 1 },
    step: { id: 'step-1', n: 1 },
    tool: { call_id: 'call_x1', name: 'read_file', output: { content: 'use clap::Parser;\n\n#[derive(Parser)]\nenum Cmd { Status, Ls, Cat { what: String } }\n', truncated: false }, status: 'success' },
    ext: { type: 'tool_call_finished', durationMs: 12 },
  })
  push({ ts: at(sec++), kind: 'meta', role: 'tool', ext: { type: 'message_committed', message: { role: 'tool', toolCallId: 'call_x1' } } })
  push({ ts: at(sec++), kind: 'step_completed', turn: { id: T, n: 1 }, step: { id: 'step-1', n: 1 }, model: 'gpt-5', usage: { input: 3000, output: 150 }, ext: { type: 'step_completed', finishReason: 'tool-calls' } })
  push({ ts: at(sec++), kind: 'step_started', turn: { id: T, n: 1 }, step: { id: 'step-2', n: 2 }, ext: { type: 'step_started' } })
  push({
    ts: at(sec++),
    kind: 'tool_call',
    role: 'assistant',
    turn: { id: T, n: 1 },
    step: { id: 'step-2', n: 2 },
    tool: { call_id: 'call_x2', name: 'edit_file', input: { path: 'src/main.rs', old: 'Status,', new: 'Status { #[arg(long)] json: bool },' } },
    ext: { type: 'tool_call_started' },
  })
  push({
    ts: at(sec++),
    kind: 'tool_result',
    role: 'tool',
    turn: { id: T, n: 1 },
    step: { id: 'step-2', n: 2 },
    tool: { call_id: 'call_x2', name: 'edit_file', output: { error: 'old string not found (2 matches)' }, status: 'error' },
    ext: { type: 'tool_call_finished', durationMs: 3 },
  })
  push({ ts: at(sec++), kind: 'step_completed', turn: { id: T, n: 1 }, step: { id: 'step-2', n: 2 }, model: 'gpt-5', usage: { input: 3400, output: 90 }, ext: { type: 'step_completed' } })
  push({ ts: at(sec++), kind: 'step_started', turn: { id: T, n: 1 }, step: { id: 'step-3', n: 3 }, ext: { type: 'step_started' } })
  push({
    ts: at(sec++),
    kind: 'message',
    role: 'assistant',
    turn: { id: T, n: 1 },
    step: { id: 'step-3', n: 3 },
    text: 'The edit target was ambiguous; I stopped here. `Status` appears twice in `src/main.rs` — which one is the subcommand enum?',
    ext: { type: 'message_committed', message: { role: 'assistant' } },
  })
  push({ ts: at(sec++), kind: 'step_completed', turn: { id: T, n: 1 }, step: { id: 'step-3', n: 3 }, model: 'gpt-5', usage: { input: 3600, output: 60 }, ext: { type: 'step_completed', finishReason: 'stop' } })
  push({ ts: at(sec++), kind: 'turn_completed', turn: { id: T, n: 1 }, model: 'gpt-5', usage: { input: 10000, output: 300, cache_read: 6000 }, ext: { type: 'turn_completed', steps: 3 } })
  return { session, parent: null, evs, raw }
}

// ---------- 会话表 ----------

const built = [claudeMain(), claudeSub(), pfhMain()]
const sources = {
  'a1b2c3d4-0000-4000-8000-000000000001': 'C:/Users/me/.claude/projects/E--code-harl/a1b2c3d4-0000-4000-8000-000000000001.jsonl',
  'agent-abc123': 'C:/Users/me/.claude/projects/E--code-harl/a1b2c3d4-0000-4000-8000-000000000001/subagents/agent-abc123.jsonl',
  'pfh-9f8e7d6c': '/home/me/.pfh/agent/protocol-events/pfh-9f8e7d6c.jsonl',
}

const sessions = built.map((b, i) => {
  const rawText = b.raw.join('\n') + '\n'
  return {
    info: {
      id: hex(`tree:${b.session}`),
      provider: b.evs[0].provider,
      machine: 'mock-machine',
      source: sources[b.session],
      session: b.session,
      parent_session: b.parent,
      size: Buffer.byteLength(rawText),
      lines: b.raw.length,
      mtime: at(200 + i * 60),
      captured_at: at(300 + i * 60),
    },
    events: b.evs,
    raw: rawText,
  }
})

// 再塞一堆只有列表项没有事件的会话，看看列表密度
for (let i = 0; i < 40; i++) {
  const s = `ffff0000-0000-4000-8000-${String(i).padStart(12, '0')}`
  sessions.push({
    info: {
      id: hex(`tree:${s}`),
      provider: i % 3 === 0 ? 'pfh' : 'claude-code',
      machine: i % 5 === 0 ? 'laptop' : 'mock-machine',
      source: i % 3 === 0 ? `/home/me/work/proj-${i}/.pfh/agent/protocol-events/${s}.jsonl` : `C:/Users/me/.claude/projects/E--code-proj-${i % 7}/${s}.jsonl`,
      session: s,
      parent_session: null,
      size: 1000 + i * 73371,
      lines: 10 + i * 37,
      mtime: at(-i * 3600 * 5),
      captured_at: at(-i * 3600 * 4),
    },
    events: [],
    raw: '',
  })
}

const status = {
  data_dir: 'C:/Users/me/.harl',
  machine: 'mock-machine',
  sources: sessions.length,
  records: sessions.length + 12,
  logical_bytes: sessions.reduce((a, s) => a + s.info.size, 0),
  packs: 3,
  pack_bytes: Math.round(sessions.reduce((a, s) => a + s.info.size, 0) * 0.29),
  chunks: sessions.reduce((a, s) => a + s.info.lines, 0),
  index_bytes: 1_234_567,
}

const byId = new Map(sessions.map((s) => [s.info.id, s]))

function send(res, code, body, type = 'application/json; charset=utf-8') {
  res.writeHead(code, { 'content-type': type, 'access-control-allow-origin': '*' })
  res.end(typeof body === 'string' ? body : JSON.stringify(body))
}

http
  .createServer((req, res) => {
    const url = new URL(req.url, `http://${HOST}:${PORT}`)
    const p = url.pathname
    console.log(req.method, p)
    if (p === '/api/status') return send(res, 200, status)
    if (p === '/api/sessions') return send(res, 200, sessions.map((s) => s.info))
    const m = p.match(/^\/api\/sessions\/([^/]+)\/(events|raw)$/)
    if (m) {
      const s = byId.get(decodeURIComponent(m[1]))
      if (!s) return send(res, 404, { error: 'no such session' })
      if (m[2] === 'events') return send(res, 200, s.events)
      return send(res, 200, s.raw, 'text/plain; charset=utf-8')
    }
    send(res, 404, { error: 'not found' })
  })
  .listen(PORT, HOST, () => console.log(`mock api on http://${HOST}:${PORT}`))
