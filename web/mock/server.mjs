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


// ---------- /api/summary：合成一个月的汇总 ----------
// 形状对着 src/api.ts 的 Summary；数字量级模仿真实归档（几百会话、几千美元、几十小时）但全是伪随机合成的。

function makeSummary(tzOffsetMin, from = null, to = null) {
  let seed = 0x9e3779b9
  const rnd = () => {
    seed = (Math.imul(seed ^ (seed >>> 15), 0x2c1b3c6d) ^ (seed >>> 13)) >>> 0
    return seed / 0x100000000
  }
  const zeroTk = () => ({ input: 0, output: 0, cache_read: 0, cache_write: 0 })
  const addTk = (a, b) => {
    a.input += b.input
    a.output += b.output
    a.cache_read += b.cache_read
    a.cache_write += b.cache_write
  }
  // 名字是编的；price 是每百万 token 的 (input, output, cache_read, cache_write)
  const MODELS = [
    { name: 'claude-opus-5', provider: 'claude-code', weight: 0.6, price: [15, 75, 1.5, 18.75] },
    { name: 'claude-sonnet-5', provider: 'claude-code', weight: 0.2, price: [3, 15, 0.3, 3.75] },
    { name: 'claude-fable-5-1', provider: 'claude-code', weight: 0.1, price: [5, 25, 0.5, 6.25] },
    { name: 'claude-haiku-4-5', provider: 'claude-code', weight: 0.02, price: [1, 5, 0.1, 1.25] },
    { name: 'deepseek-v4-flash', provider: 'pfh', weight: 0.06, price: null },
    { name: 'gpt-5.6-luna', provider: 'pfh', weight: 0.02, price: null },
  ]
  const TOOLS = ['Bash', 'Read', 'Edit', 'Write', 'AskUserQuestion', 'Glob', 'Agent', 'WebFetch', 'WebSearch', 'PowerShell', 'ToolSearch', 'Skill', 'Monitor', 'TaskStop', 'Grep', 'bash', 'edit_file', 'read_file', 'mcp__browser__computer', 'mcp__browser__batch']
  // 与 crates/harl-core 的 ToolCategory 归类一致
  const categoryOf = (name) => {
    if (/^mcp__/.test(name)) return 'mcp'
    if (/^(bash|powershell)$/i.test(name)) return 'shell'
    if (/^(read|read_file)$/i.test(name)) return 'read'
    if (/^(edit|write|edit_file|write_file)$/i.test(name)) return 'edit'
    if (/^(grep|glob)$/i.test(name)) return 'search'
    if (/^(webfetch|websearch|web_fetch)$/i.test(name)) return 'web'
    if (/^(agent|run_subagent|sendmessage|task\w*|monitor)$/i.test(name)) return 'agent'
    if (/^(askuserquestion|request_user_input|artifact)$/i.test(name)) return 'user'
    if (/^(skill|toolsearch|todo\w*|enterplanmode|exitplanmode)$/i.test(name)) return 'meta'
    return 'other'
  }

  const totals = { sessions: 0, turns: 0, steps: 0, tool_calls: 0, tool_failures: 0, tokens: zeroTk(), cost_usd: 0, unpriced_steps: 0, model_ms: 0, tool_ms: 0, wall_ms: 0, first_ts: null, last_ts: null }
  // 全部数据的跨度，不受 from/to 影响
  let availFirst = null
  let availLast = null
  const by_provider = {}
  const by_model = {}
  const by_tool = {}
  const by_category = {}
  const dailyMap = new Map()
  const hourly_turns = Array.from({ length: 7 }, () => Array(24).fill(0))
  const hourly_cost = Array.from({ length: 7 }, () => Array(24).fill(0))
  const sessionsOut = []

  const end = new Date(t0)
  const start = new Date(t0 - 31 * 86400e3)
  const pad = (n) => String(n).padStart(2, '0')
  const localParts = (d) => {
    const l = new Date(d.getTime() + tzOffsetMin * 60e3)
    return { date: `${l.getUTCFullYear()}-${pad(l.getUTCMonth() + 1)}-${pad(l.getUTCDate())}`, dow: (l.getUTCDay() + 6) % 7, hour: l.getUTCHours() }
  }

  const N = 180
  for (let i = 0; i < N; i++) {
    const isSub = rnd() < 0.6
    const m = MODELS[Math.min(MODELS.length - 1, Math.floor(rnd() ** 2 * MODELS.length))]
    const provider = m.provider
    // 工作日、白天更密
    let startTs
    for (;;) {
      startTs = new Date(start.getTime() + rnd() * (end - start))
      const { dow, hour } = localParts(startTs)
      const w = (dow < 5 ? 1 : 0.4) * (hour >= 9 && hour <= 23 ? 1 : 0.15)
      if (rnd() < w) break
    }
    const turns = isSub ? 1 : 1 + Math.floor(rnd() ** 2 * 12)
    const steps = turns * (3 + Math.floor(rnd() * 40))
    const tool_calls = Math.floor(steps * (0.8 + rnd() * 0.5))
    const failures = Math.floor(tool_calls * rnd() * 0.08)
    const tk = {
      input: Math.floor(steps * (m.price ? 5 : 20000) * (0.5 + rnd())),
      output: Math.floor(steps * 800 * (0.3 + rnd())),
      cache_read: Math.floor(steps * (m.price ? 220000 : 200000) * (0.5 + rnd())),
      cache_write: Math.floor(steps * (m.price ? 4000 : 0) * (0.5 + rnd())),
    }
    const cost = m.price ? (tk.input * m.price[0] + tk.output * m.price[1] + tk.cache_read * m.price[2] + tk.cache_write * m.price[3]) / 1e6 : 0
    const model_ms = steps * (5000 + rnd() * 15000)
    const tool_ms = tool_calls * (200 + rnd() ** 3 * 60000)
    const wall_ms = (model_ms + tool_ms) * (1.02 + rnd() * 0.3)
    const endTs = new Date(startTs.getTime() + wall_ms)
    const sid = `${hex(`s${i}`).slice(0, 8)}-0000-4000-8000-${String(i).padStart(12, '0')}`
    if (!availFirst || startTs.toISOString() < availFirst) availFirst = startTs.toISOString()
    if (!availLast || endTs.toISOString() > availLast) availLast = endTs.toISOString()
    // 范围过滤：真后端按 turn 过滤，这里一个会话当一个 turn 桶，按开始日期算
    const { date, dow, hour } = localParts(startTs)
    // 不能 continue：rnd() 的调用序列要和不过滤时一致，available 才稳定
    const inRange = !((from && date < from) || (to && date > to))

    if (inRange) sessionsOut.push({
      id: hex(`tree:mock-summary-${i}`),
      provider,
      session: sid,
      source: provider === 'pfh' ? `/home/me/work/proj-${i % 5}/.pfh/agent/protocol-events/${sid}.jsonl` : `C:/Users/me/.claude/projects/E--code-proj-${i % 7}/${sid}.jsonl`,
      parent_session: isSub ? `${hex(`p${i % 40}`).slice(0, 8)}-0000-4000-8000-000000000000` : null,
      turns,
      steps,
      tool_calls,
      tokens: tk,
      cost_usd: cost,
      model_ms,
      tool_ms,
      wall_ms,
      first_ts: startTs.toISOString(),
      last_ts: endTs.toISOString(),
    })

    if (inRange) {
      totals.sessions++
      totals.turns += turns
      totals.steps += steps
      totals.tool_calls += tool_calls
      totals.tool_failures += failures
      addTk(totals.tokens, tk)
      totals.cost_usd += cost
      if (!m.price) totals.unpriced_steps += steps
      totals.model_ms += model_ms
      totals.tool_ms += tool_ms
      totals.wall_ms += wall_ms
      if (!totals.first_ts || startTs.toISOString() < totals.first_ts) totals.first_ts = startTs.toISOString()
      if (!totals.last_ts || endTs.toISOString() > totals.last_ts) totals.last_ts = endTs.toISOString()

      const bp = (by_provider[provider] ??= { sessions: 0, turns: 0, tokens: zeroTk(), cost_usd: 0, model_ms: 0, tool_ms: 0, wall_ms: 0, by_model: {} })
      bp.sessions++
      bp.turns += turns
      addTk(bp.tokens, tk)
      bp.cost_usd += cost
      bp.model_ms += model_ms
      bp.tool_ms += tool_ms
      bp.wall_ms += wall_ms

      for (const table of [by_model, bp.by_model]) {
        const bm = (table[m.name] ??= { steps: 0, tokens: zeroTk(), cost_usd: 0, unpriced: !m.price, model_ms: 0 })
        bm.steps += steps
        addTk(bm.tokens, tk)
        bm.cost_usd += cost
        bm.model_ms += model_ms
      }
    }

    // 工具：按 Zipf 分给几种工具
    let leftCalls = tool_calls
    let leftMs = tool_ms
    let leftFail = failures
    const pool = provider === 'pfh' ? TOOLS.slice(15) : TOOLS.slice(0, 15)
    for (let k = 0; k < pool.length && leftCalls > 0; k++) {
      const share = k === pool.length - 1 ? 1 : 0.25 / (k + 1)
      const c = Math.min(leftCalls, Math.max(1, Math.round(tool_calls * share)))
      const ms = Math.min(leftMs, tool_ms * share * (0.5 + rnd()))
      const f = Math.min(leftFail, Math.round(failures * share * rnd() * 2))
      if (inRange) {
        const cat = categoryOf(pool[k])
        const bt = (by_tool[pool[k]] ??= { category: cat, calls: 0, failures: 0, ms: 0 })
        bt.calls += c
        bt.failures += f
        bt.ms += ms
        const bc = (by_category[cat] ??= { category: cat, calls: 0, failures: 0, ms: 0 })
        bc.calls += c
        bc.failures += f
        bc.ms += ms
      }
      leftCalls -= c
      leftMs -= ms
      leftFail -= f
    }

    if (!inRange) continue
    const d = dailyMap.get(date) ?? { date, turns: 0, steps: 0, tool_calls: 0, tokens: zeroTk(), cost_usd: 0, model_ms: 0, tool_ms: 0, wall_ms: 0, by_model: {} }
    d.turns += turns
    d.steps += steps
    d.tool_calls += tool_calls
    addTk(d.tokens, tk)
    d.cost_usd += cost
    d.model_ms += model_ms
    d.tool_ms += tool_ms
    d.wall_ms += wall_ms
    const dm = (d.by_model[m.name] ??= { steps: 0, tokens: zeroTk(), cost_usd: 0, unpriced: !m.price, model_ms: 0 })
    dm.steps += steps
    addTk(dm.tokens, tk)
    dm.cost_usd += cost
    dm.model_ms += model_ms
    dailyMap.set(date, d)
    hourly_turns[dow][hour] += turns
    hourly_cost[dow][hour] += cost
  }

  return {
    tz_offset_min: tzOffsetMin,
    range: { from, to },
    available: [availFirst, availLast],
    totals,
    by_provider,
    by_model,
    by_tool,
    by_category,
    daily: [...dailyMap.values()].sort((a, b) => (a.date < b.date ? -1 : 1)),
    hourly_turns,
    hourly_cost,
    sessions: sessionsOut,
  }
}

const summaryCache = new Map()
function summaryFor(tz, from, to) {
  const key = `${tz}|${from ?? ''}|${to ?? ''}`
  if (!summaryCache.has(key)) summaryCache.set(key, makeSummary(tz, from, to))
  return summaryCache.get(key)
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
    if (p === '/api/summary') {
      const tz = Number(url.searchParams.get('tz') ?? 0)
      if (!Number.isFinite(tz)) return send(res, 400, { error: 'bad tz' })
      const dateRe = /^\d{4}-\d{2}-\d{2}$/
      const from = url.searchParams.get('from')
      const to = url.searchParams.get('to')
      if ((from && !dateRe.test(from)) || (to && !dateRe.test(to))) return send(res, 400, { error: 'bad from/to' })
      // 真后端首次要算几秒，这里也拖一下，方便看加载态
      const delay = summaryCache.size ? 0 : 1500
      return setTimeout(() => send(res, 200, summaryFor(Math.round(tz), from || null, to || null)), delay)
    }
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
