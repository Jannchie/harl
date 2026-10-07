// 根目录 `pnpm dev`：同时起 `harl serve` 和 vite，并保证退出时整棵进程树都被收掉。
// 不用 concurrently 是因为 pnpm 的 script-shell 在 Windows 上走 msys bash，Ctrl+C / 关终端时
// 中间那层 sh.exe 先死，下面的 cargo / harl.exe / vite / esbuild 就成了没人管的孤儿进程。
// 这里直接 spawn 真正的程序（不经过 shell），任何一个退出、收到信号、或父进程消失，都整树杀掉。
import { execFile, spawn, spawnSync } from 'node:child_process'
import { promisify } from 'node:util'
import { createInterface } from 'node:readline'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const isWin = process.platform === 'win32'
const env = { ...process.env, FORCE_COLOR: '1', CARGO_TERM_COLOR: 'always' }

// 和根目录 package.json 的 dev:api、web/package.json 的 dev 是同一条命令，改一边记得改另一边
const tasks = [
  { name: 'api', color: 34, cmd: 'cargo', args: ['run', '-p', 'harl', '--', 'serve'], cwd: root },
  { name: 'web', color: 32, cmd: process.execPath, args: [path.join(root, 'web/node_modules/vite/bin/vite.js')], cwd: path.join(root, 'web') },
]

const children = []
let stopping = false

function killTree(child) {
  if (child.exitCode !== null || child.signalCode !== null) return
  if (isWin) spawnSync('taskkill', ['/T', '/F', '/PID', String(child.pid)], { stdio: 'ignore' })
  else {
    try { process.kill(-child.pid, 'SIGTERM') } catch {}
  }
}

// 真正的收尾在 'exit' 里做，这样 process.exit 从哪儿触发都会杀子进程
function shutdown(code) {
  if (stopping) return
  stopping = true
  process.exit(code)
}

for (const task of tasks) {
  const child = spawn(task.cmd, task.args, {
    cwd: task.cwd,
    env,
    stdio: ['ignore', 'pipe', 'pipe'],
    // POSIX 下自成进程组，才能用 -pid 一次杀整组
    detached: !isWin,
  })
  const prefix = `\x1b[${task.color}m[${task.name}]\x1b[0m `
  for (const stream of [child.stdout, child.stderr]) {
    createInterface({ input: stream }).on('line', line => process.stdout.write(prefix + line + '\n'))
  }
  child.on('error', err => {
    process.stdout.write(`${prefix}启动失败：${err.message}\n`)
    shutdown(1)
  })
  child.on('exit', (code, signal) => {
    if (stopping) return
    process.stdout.write(`${prefix}已退出（${signal ?? code}），关闭其余进程\n`)
    shutdown(code ?? 1)
  })
  children.push(child)
}

for (const sig of ['SIGINT', 'SIGTERM', 'SIGHUP', 'SIGBREAK']) process.on(sig, () => shutdown(0))
process.on('exit', () => { for (const child of children) killTree(child) })

// 终端被关掉 / 外层进程被硬杀时没有信号可收，而且 pnpm 自己在父进程死后也不会退出，
// 所以盯住整条祖先链（pnpm、shell 包装层，以及最外层启动 pnpm 的那个进程），任何一个没了就收尾。
const WRAPPERS = new Set(['node', 'bash', 'sh', 'cmd', 'pnpm', 'npm'])

// 起 PowerShell 查进程表要一两秒，异步跑，别卡住子进程输出的转发
async function ancestors() {
  if (!isWin) return [process.ppid]
  const { stdout: out } = await promisify(execFile)('powershell.exe', [
    '-NoProfile', '-NonInteractive', '-Command',
    'Get-CimInstance Win32_Process | ForEach-Object { "$($_.ProcessId) $($_.ParentProcessId) $($_.Name)" }',
  ]).catch(() => ({ stdout: '' }))
  const table = new Map()
  for (const line of out.split(/\r?\n/)) {
    const [pid, ppid, ...name] = line.trim().split(' ')
    if (pid) table.set(Number(pid), { ppid: Number(ppid), name: name.join(' ').replace(/\.exe$/i, '').toLowerCase() })
  }
  const chain = []
  let pid = process.ppid
  while (table.has(pid) && chain.length < 10) {
    chain.push(pid)
    const { ppid, name } = table.get(pid)
    if (!WRAPPERS.has(name)) break
    pid = ppid
  }
  return chain.length ? chain : [process.ppid]
}

const watched = await ancestors()
setInterval(() => {
  for (const pid of watched) {
    try { process.kill(pid, 0) } catch { return shutdown(0) }
  }
}, 1000).unref()
