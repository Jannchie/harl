// Portions derived from dsh-context (Apache-2.0, © 2025 bowenliang123), modified for harl. See THIRD_PARTY_NOTICES.md.
// 命令执行：把各家的 shell / 代码执行工具调用整理成一条条「跑了什么、跑了多久、结果如何」。
// 程序名抽取、结果尾部标记、摘要 / 值得注意 / 程序分布的口径参考 dsh-context（Apache-2.0）的 commands，
// 这里按 harl 的 Event 重新实现，并扩到 Claude Code（Bash / PowerShell）、Codex（shell / exec_command /
// shell_command / local_shell）、pfh 和 dsh（bash / pwsh / run_code，以及 run_code 里嵌套的命令）。

import { toolFailed, type Event } from '../api'

// ---------------------------------------------------------------------------
// 程序名抽取
// ---------------------------------------------------------------------------

export type ShellKind = 'sh' | 'pwsh'

/** 包一层再跑真正程序的前缀命令 */
const WRAPPERS = new Set(['env', 'sudo', 'doas', 'command', 'exec', 'time', 'nohup', 'nice', 'stdbuf', 'timeout', 'caffeinate', 'noglob', 'nocorrect'])
/** 包装命令里带参数的长短选项（参数要一起跳过） */
const WRAPPER_ARG_OPTS: Record<string, Set<string>> = {
  env: new Set(['-u', '--unset', '-C', '--chdir', '-S', '--split-string']),
  sudo: new Set(['-u', '--user', '-g', '--group', '-p', '--prompt', '-C', '--close-from', '-D', '--chdir', '-h', '--host', '-r', '--role', '-t', '--type', '-U', '--other-user', '-R', '--chroot']),
  doas: new Set(['-u', '-C']),
  nice: new Set(['-n', '--adjustment']),
  time: new Set(['-f', '--format', '-o', '--output']),
  timeout: new Set(['-s', '--signal', '-k', '--kill-after']),
  stdbuf: new Set(['-i', '--input', '-o', '--output', '-e', '--error']),
  caffeinate: new Set(['-t', '-w']),
  exec: new Set(['-a']),
}
/** 合在一起写的短选项（`sudo -iu root`）里哪些字母要参数 */
const WRAPPER_ARG_LETTERS: Record<string, string> = {
  sudo: 'ugpCDhrtUR',
  doas: 'uC',
  env: 'uCS',
  nice: 'n',
  timeout: 'sk',
  stdbuf: 'ioe',
  caffeinate: 'tw',
  exec: 'a',
}
/** sh 的关键字：落到它们上面说明是控制结构，记成 shell */
const SH_KEYWORDS = new Set(['[', '[[', 'for', 'if', 'while', 'until', 'case', 'do', 'then', 'else', 'elif', 'function', 'select', 'coproc', 'done', 'fi', 'esac'])
/** 引出一条命令的关键字：`if grep …`、`then echo …`，跳过它看后面的命令 */
const SH_LEADERS = new Set(['if', 'then', 'else', 'elif', 'do', 'while', 'until', '!', '{'])
const SH_DECL = new Set(['export', 'local', 'declare', 'readonly', 'typeset'])
const PWSH_KEYWORDS = new Set([
  'if', 'elseif', 'else', 'for', 'foreach', 'while', 'do', 'switch', 'function', 'filter', 'param', 'begin', 'process', 'end',
  'try', 'catch', 'finally', 'trap', 'return', 'throw', 'break', 'continue', 'using', 'class', 'enum',
])

type Vars = Map<string, string | null>

/** 双引号里 `\` 能转义的字符 */
const DQ_ESC = /[\\$`"\n]/

/** 读一个 shell 词：拼接引号段、处理转义；遇到空白或 ; & | ) } 停 */
function readWord(s: string): { word: string; end: number } | null {
  let i = 0
  let w = ''
  while (i < s.length) {
    const c = s[i]!
    if (c === '"' || c === "'") {
      i++
      while (i < s.length && s[i] !== c) {
        if (c === '"' && s[i] === '\\' && i + 1 < s.length && DQ_ESC.test(s[i + 1]!)) i++
        w += s[i]
        i++
      }
      if (i < s.length) i++
    } else if (c === '\\' && i + 1 < s.length && /\s/.test(s[i + 1]!)) {
      w += s[i + 1]
      i += 2
    } else {
      if (/[\s;&|)}]/.test(c)) break
      w += c
      i++
    }
  }
  return w ? { word: w, end: i } : null
}

/** 从 s[from]（一个 `(`）起找到配对的 `)` 之后的位置，跳过引号 */
function skipParens(s: string, from: number): number {
  let depth = 0
  for (let i = from; i < s.length; i++) {
    const c = s[i]
    if (c === '"' || c === "'") {
      const j = s.indexOf(c, i + 1)
      if (j < 0) return s.length
      i = j
    } else if (c === '(') depth++
    else if (c === ')' && --depth === 0) return i + 1
  }
  return s.length
}

/** sh 的 `NAME=value` / `NAME+=value`；value 能静态确定才记下，否则 null */
function shAssign(s: string): { name: string; value: string | null; end: number } | null {
  const m = /^([A-Za-z_][A-Za-z0-9_]*)\+?=/.exec(s)
  if (!m) return null
  const append = m[0].includes('+')
  let i = m[0].length
  const q = s[i]
  let value: string | null = null
  if (q === '"' || q === "'") {
    let j = i + 1
    let v = ''
    while (j < s.length && s[j] !== q) {
      if (q === '"' && s[j] === '\\' && j + 1 < s.length && DQ_ESC.test(s[j + 1]!)) j++
      v += s[j]
      j++
    }
    i = j < s.length ? j + 1 : s.length
    value = q === "'" || !/[$`]/.test(v) ? v : null
  } else if (s.startsWith('$(', i)) i = skipParens(s, i + 1)
  else if (q === '`') {
    const j = s.indexOf('`', i + 1)
    i = j < 0 ? s.length : j + 1
  } else if (q === '(') i = skipParens(s, i)
  else {
    const start = i
    while (i < s.length && !/[\s;&|]/.test(s[i]!)) i++
    const v = s.slice(start, i)
    value = /[$`]/.test(v) ? null : v
  }
  // 值后面紧跟着的拼接段（`A="x"y`）：值就不确定了
  let k = i
  while (k < s.length && !/[\s;&|]/.test(s[k]!)) {
    const c = s[k]
    if (c === '"' || c === "'") {
      const j = s.indexOf(c, k + 1)
      if (j < 0) {
        k = s.length
        break
      }
      k = j + 1
    } else k++
  }
  if (k > i) {
    i = k
    value = null
  }
  return { name: m[1]!, value: append ? null : value, end: i }
}

/** `export A=1 B=2 …`：记下变量，返回后面剩下的 */
function stripDecl(s: string, vars: Vars): string | null {
  const head = readWord(s)
  if (!head || !SH_DECL.has(head.word)) return null
  let i = head.end
  for (let n = 0; n < 16; n++) {
    const ws = /^[ \t]+/.exec(s.slice(i))
    if (!ws) break
    i += ws[0].length
    const rest = s.slice(i)
    if (!rest || /^[;|&#]/.test(rest)) break
    const a = shAssign(rest)
    if (a) {
      vars.set(a.name, a.value)
      i += a.end
      continue
    }
    const w = readWord(rest)
    if (!w) break
    i += w.end
  }
  return s.slice(i)
}

/** 去掉开头的重定向（`2>&1`、`> out`、`&>log`） */
function stripRedirs(s: string): string {
  let t = s
  for (let n = 0; n < 8; n++) {
    const m = /^\s*(?:&>|[0-9]*[<>]+&?)\s*/.exec(t)
    if (!m) break
    t = t.slice(m[0].length)
    const w = readWord(t)
    if (!w) break
    t = t.slice(w.end)
  }
  return t
}

/** `cd X && …` / `cd X; …`：返回后面的命令；只有 cd 没有后续就 null */
function stripCd(s: string, pwsh: boolean): string | null {
  const head = (pwsh ? /^(?:cd|Set-Location|sl)\b[ \t]*/i : /^cd[ \t]+/).exec(s)
  if (!head) return null
  const rest = s.slice(head[0].length)
  let i = 0
  for (let n = 0; n < 8; n++) {
    const o = (pwsh ? /^-(?:Path|LiteralPath)[ \t]+|^-[A-Z]+[ \t]*/i : /^(?:-[PLe@]+|--)[ \t]+/).exec(rest.slice(i))
    if (!o) break
    i += o[0].length
  }
  const dir = readWord(rest.slice(i))
  if (!dir) return null
  const after = stripRedirs(rest.slice(i + dir.end))
  const sep = /^[ \t]*(?:&&|;|\r?\n)[ \t]*/.exec(after)
  if (!sep) return null
  const next = after.slice(sep[0].length)
  return next.trim() ? next : null
}

/** 去掉路径和 .exe / .cmd / .bat */
function baseName(w: string): string {
  let t = w.trim()
  const i = Math.max(t.lastIndexOf('/'), t.lastIndexOf('\\'))
  if (i >= 0) t = t.slice(i + 1)
  const d = t.lastIndexOf('.')
  if (d > 0 && /\.(?:exe|cmd|bat)$/i.test(t)) t = t.slice(0, d)
  return t.trim()
}

/** 包装命令（sudo / env / timeout …）连同它们的选项去掉，返回被包的命令；认不出就 null */
function stripWrapper(s: string): string | null {
  const head = readWord(s)
  if (!head) return null
  const name = baseName(head.word)
  if (!WRAPPERS.has(name)) return null
  // `command -v x` 是查询，不是执行 x
  if (name === 'command' && /^\s+-[A-Za-z]*[vV][A-Za-z]*\b/.test(s.slice(head.end))) return null
  const letters = WRAPPER_ARG_LETTERS[name]
  let i = head.end
  let durationTaken = false
  for (let n = 0; n < 24; n++) {
    const ws = /^\s+/.exec(s.slice(i))
    if (!ws) break
    i += ws[0].length
    const rest = s.slice(i)
    if (!rest || /^[;|&#]/.test(rest)) return null
    if (/^--(?:\s|$)/.test(rest)) {
      i += 2
      continue
    }
    if (name === 'env' || name === 'sudo' || name === 'doas') {
      const a = shAssign(rest)
      if (a) {
        i += a.end
        continue
      }
    }
    const opt = /^(-{1,2}[A-Za-z0-9][A-Za-z0-9_-]*)(?:=\S+)?/.exec(rest)
    if (opt) {
      i += opt[0].length
      const flag = opt[1]!
      const inline = opt[0].includes('=')
      let takesArg = !inline && (WRAPPER_ARG_OPTS[name]?.has(flag) ?? false)
      // 合写的短选项：要参数的字母在最后才吃下一个词
      if (!inline && !flag.startsWith('--') && flag.length > 2 && letters) {
        const ls = flag.slice(1)
        for (let k = 0; k < ls.length; k++)
          if (letters.includes(ls[k]!)) {
            takesArg = k === ls.length - 1
            break
          }
      }
      if (takesArg) {
        let sp = 0
        while (i + sp < s.length && (s[i + sp] === ' ' || s[i + sp] === '\t')) sp++
        const w = readWord(s.slice(i + sp))
        if (w) i += sp + w.end
      }
      continue
    }
    // timeout 的第一个位置参数是时长
    if (name === 'timeout' && !durationTaken) {
      const w = readWord(rest)
      if (w) {
        i += w.end
        durationTaken = true
        continue
      }
    }
    break
  }
  const out = stripRedirs(s.slice(i)).trimStart()
  return !out || /^[;|&#]/.test(out) ? null : out
}

/** sh：命令头是 `$VAR` / `${VAR}/bin/x` 时按记下的变量解析；解析不了 null */
function shResolve(w: string, vars: Vars): string | null {
  if (!w || w.startsWith('$(') || w.includes('`')) return null
  const whole = /^\$([A-Za-z_][A-Za-z0-9_]*)$/.exec(w) ?? /^\$\{([A-Za-z_][A-Za-z0-9_]*)\}$/.exec(w)
  if (whole) return vars.get(whole[1]!) ?? null
  const pre = /^\$([A-Za-z_][A-Za-z0-9_]*)/.exec(w) ?? /^\$\{([A-Za-z_][A-Za-z0-9_]*)\}/.exec(w)
  if (pre) {
    const tail = w.slice(pre[0].length)
    if (tail.includes('$')) return null
    const v = vars.get(pre[1]!)
    if (v == null) {
      // 变量不知道，但后面是路径：取最后一段（`$HOME/.cargo/bin/cargo` → cargo）
      const i = Math.max(tail.lastIndexOf('/'), tail.lastIndexOf('\\'))
      return i >= 0 && tail.slice(i + 1) ? tail.slice(i + 1) : null
    }
    return v + tail
  }
  return w.includes('$') ? null : w
}

/** 一段 sh 命令的程序名；剥前缀时记下的变量写进 vars */
function shProgram(seg: string, vars: Vars): string {
  if (!seg.trim()) return ''
  let s = seg.trimStart()
  for (let n = 0; n < 32; n++) {
    s = s.replace(/^[\s;&|]+/, '')
    if (!s) break
    if (s.startsWith('#')) {
      const nl = s.indexOf('\n')
      if (nl < 0) break
      s = s.slice(nl + 1)
      continue
    }
    if (/^\\\r?\n/.test(s)) {
      s = s.replace(/^\\\r?\n[ \t]*/, '')
      continue
    }
    const lead = /^(\S+)[ \t]+/.exec(s)
    if (lead && SH_LEADERS.has(lead[1]!)) {
      s = s.slice(lead[0].length)
      continue
    }
    const r = stripRedirs(s)
    if (r !== s) {
      s = r.trimStart()
      if (!s) break
      continue
    }
    const cd = stripCd(s, false)
    if (cd != null) {
      s = cd
      continue
    }
    const decl = stripDecl(s, vars)
    if (decl != null) {
      s = decl
      continue
    }
    const a = shAssign(s)
    if (a) {
      vars.set(a.name, a.value)
      s = s.slice(a.end).trimStart()
      continue
    }
    const w = stripWrapper(s)
    if (w == null) break
    s = w
  }
  if (!s || s.startsWith('#') || s.startsWith('(') || s.startsWith('{')) return 'shell'
  const head = readWord(s)
  if (!head) return 'shell'
  // 函数定义 `f() { … }`
  if ((/^[A-Za-z_][A-Za-z0-9_-]*\($/.test(head.word) && s.slice(head.end).startsWith(')')) || /^\s*\(\s*\)/.test(s.slice(head.end)))
    return 'shell'
  const word = shResolve(head.word, vars)
  if (!word) return 'shell'
  if (SH_KEYWORDS.has(word) || SH_KEYWORDS.has(word.split('(')[0]!)) return 'shell'
  return baseName(word)
}

/** pwsh 语句的结尾（深度 0 的 ; 或换行），跳过引号和注释 */
function pwshStmtEnd(s: string, from: number): number {
  let depth = 0
  for (let i = from; i < s.length; i++) {
    const c = s[i]
    if (c === '"' || c === "'") {
      const j = s.indexOf(c, i + 1)
      if (j < 0) return s.length
      i = j
    } else if (c === '#') {
      const nl = s.indexOf('\n', i)
      if (nl < 0) return s.length
      i = nl - 1
    } else if (c === '(' || c === '[' || c === '{') depth++
    else if (c === ')' || c === ']' || c === '}') depth = Math.max(0, depth - 1)
    else if (depth === 0 && (c === ';' || c === '\n')) return i
  }
  return s.length
}

const PWSH_ASSIGN = /^\s*(?:\[[^\]]+\]\s*)?\$([A-Za-z_][A-Za-z0-9_]*(?::[A-Za-z_][A-Za-z0-9_-]*|\[[^\]]*\]|\.[A-Za-z_][A-Za-z0-9_]*)*)\s*(?:\+|-|\*|\/|%|\?\?)?=\s*/

/** pwsh 的 `$x = …`；值是字面量字符串才记下 */
function pwshAssign(s: string): { name: string; value: string | null; end: number } | null {
  const m = PWSH_ASSIGN.exec(s)
  if (!m) return null
  const compound = /[-+*/%]=|\?\?=/.test(m[0])
  const i = m[0].length
  const q = s[i]
  let value: string | null = null
  let end: number
  if (q === '"' || q === "'") {
    let j = i + 1
    let v = ''
    while (j < s.length && s[j] !== q) {
      if (q === '"' && s[j] === '`' && j + 1 < s.length) j++
      v += s[j]
      j++
    }
    end = j < s.length ? j + 1 : s.length
    value = q === "'" || !/[$`]/.test(v) ? v : null
    // 字符串后面还有别的（`"a" + $b`）：整条语句跳过，值不确定
    if (!/^[ \t]*(?:;|#|$|\r?\n)/.test(s.slice(end))) {
      end = pwshStmtEnd(s, end)
      value = null
    }
  } else end = pwshStmtEnd(s, i)
  return { name: m[1]!, value: compound ? null : value, end }
}

function pwshResolve(w: string, vars: Vars): string | null {
  if (!w || w.startsWith('$(')) return null
  const whole = /^\$([A-Za-z_][A-Za-z0-9_]*(?::[A-Za-z_][A-Za-z0-9_-]*)*)$/.exec(w) ?? /^\$\{([^}]+)\}$/.exec(w)
  if (whole) return vars.get(whole[1]!.replace(/^\$/, '')) ?? null
  const pre = /^\$([A-Za-z_][A-Za-z0-9_]*(?::[A-Za-z_][A-Za-z0-9_-]*)?)/.exec(w) ?? /^\$\{([^}]+)\}/.exec(w)
  if (pre) {
    const tail = w.slice(pre[0].length)
    if (tail.includes('$') || tail.includes('`')) return null
    const v = vars.get(pre[1]!.replace(/^\$/, ''))
    if (v != null) return v + tail
    const i = Math.max(tail.lastIndexOf('/'), tail.lastIndexOf('\\'))
    return i >= 0 && tail.slice(i + 1) ? tail.slice(i + 1) : null
  }
  return w.includes('$') || w.includes('`') ? null : w
}

/** 一段 pwsh 命令的程序名 */
function pwshProgram(seg: string, vars: Vars): string {
  if (!seg.trim()) return ''
  let s = seg.trimStart()
  for (let n = 0; n < 32; n++) {
    s = s.replace(/^[\s;]+/, '')
    if (!s) break
    if (s.startsWith('<#')) {
      const e = s.indexOf('#>', 2)
      s = e < 0 ? '' : s.slice(e + 2).trimStart()
      continue
    }
    if (s.startsWith('#')) {
      const nl = s.indexOf('\n')
      if (nl < 0) break
      s = s.slice(nl + 1)
      continue
    }
    const r = stripRedirs(s)
    if (r !== s) {
      s = r.trimStart()
      if (!s) break
      continue
    }
    const cd = stripCd(s, true)
    if (cd != null) {
      s = cd
      continue
    }
    // [Console]::OutputEncoding = …
    if (/^\s*\[[^\]]+\]::[A-Za-z_][A-Za-z0-9_.]*\s*(?:\+|-|\*|\/|%|\?\?)?=/.test(s)) {
      s = s.slice(pwshStmtEnd(s, 0)).trimStart()
      continue
    }
    const a = pwshAssign(s)
    if (!a) break
    vars.set(a.name, a.value)
    s = s.slice(a.end).trimStart()
  }
  if (!s || s.startsWith('#')) return 'pwsh'
  // 调用运算符 `& "C:\x\tool.exe"` / 点源 `. ./x.ps1`
  let invoked = false
  const call = /^&\s*/.exec(s) ?? /^\.\s+/.exec(s)
  if (call) {
    invoked = true
    s = s.slice(call[0].length)
  }
  if (!s) return 'pwsh'
  const quoted = s.startsWith('"') || s.startsWith("'")
  const head = readWord(s)
  if (!head || /^[([{@]/.test(head.word)) return 'pwsh'
  const numeric = /^\d+(?:\.\d+)?(?:\.\.\d+(?:\.\d+)?)?$/.test(head.word)
  // 不带调用运算符时，字符串 / 变量 / 数字开头是表达式，不是命令
  if (!invoked && (quoted || head.word.startsWith('$') || head.word.startsWith('!') || numeric || /^-(?:not|join|split)\b/i.test(head.word)))
    return 'pwsh'
  const word = pwshResolve(head.word, vars)
  if (!word) return 'pwsh'
  const lower = word.toLowerCase()
  if (PWSH_KEYWORDS.has(lower) || PWSH_KEYWORDS.has(lower.split('(')[0]!)) return 'pwsh'
  return baseName(word)
}

function programOf(seg: string, shell: ShellKind, vars: Vars): string {
  return shell === 'pwsh' ? pwshProgram(seg, vars) : shProgram(seg, vars)
}

/** heredoc 的正文跳过：从 from 起逐行找各个结束符，返回正文之后的位置 */
function skipHeredocs(s: string, from: number, docs: { delim: string; tabs: boolean }[]): number {
  let i = from
  for (const d of docs) {
    let found = false
    while (i < s.length) {
      const nl = s.indexOf('\n', i)
      let line = s.slice(i, nl < 0 ? s.length : nl)
      if (line.endsWith('\r')) line = line.slice(0, -1)
      if (d.tabs) line = line.replace(/^\t+/, '')
      i = nl < 0 ? s.length : nl + 1
      if (line === d.delim) {
        found = true
        break
      }
    }
    if (!found) return s.length
  }
  return i
}

export interface Segment {
  text: string
  /** 这段后面的分隔符：; && || | & 换行；最后一段是空串 */
  sep: string
}

/** 按 shell 语法切成一条条简单命令：引号、heredoc、$()、括号、注释里的分隔符不算 */
export function splitSegments(cmd: string, shell: ShellKind = 'sh'): Segment[] {
  const out: Segment[] = []
  const docs: { delim: string; tabs: boolean }[] = []
  const pwsh = shell === 'pwsh'
  let start = 0
  let i = 0
  let paren = 0
  let brace = 0
  const cut = (at: number, sep: string, next: number) => {
    const text = cmd.slice(start, at)
    if (text.trim()) out.push({ text, sep })
    start = next
  }
  while (i < cmd.length) {
    const c = cmd[i]!
    if (!pwsh && c === '\\') {
      i += 2
      continue
    }
    if (pwsh && c === '`') {
      i += 2
      continue
    }
    if (pwsh && c === '<' && cmd[i + 1] === '#') {
      const e = cmd.indexOf('#>', i + 2)
      i = e < 0 ? cmd.length : e + 2
      continue
    }
    // here-string：@" 或 @' 后紧跟换行，到行首的 "@ / '@ 结束
    if (pwsh && c === '@' && (cmd[i + 1] === '"' || cmd[i + 1] === "'") && /^\r?\n/.test(cmd.slice(i + 2))) {
      const close = `\n${cmd[i + 1]}@`
      const e = cmd.indexOf(close, i + 2)
      i = e < 0 ? cmd.length : e + close.length
      continue
    }
    if (c === '"' || c === "'" || (!pwsh && c === '`')) {
      let j = i + 1
      while (j < cmd.length) {
        const d = cmd[j]!
        if (c === '"' && j + 1 < cmd.length && ((!pwsh && d === '\\' && DQ_ESC.test(cmd[j + 1]!)) || (pwsh && d === '`'))) {
          j += 2
          continue
        }
        if (c !== "'" && !pwsh && c === '`' && d === '\\') {
          j += 2
          continue
        }
        if (d === c) {
          // pwsh 里 '' / "" 是转义的引号
          if (pwsh && cmd[j + 1] === c) {
            j += 2
            continue
          }
          break
        }
        j++
      }
      i = j + 1
      continue
    }
    if (c === '#' && (i === 0 || /[\s;&|(]/.test(cmd[i - 1]!))) {
      const nl = cmd.indexOf('\n', i)
      i = nl < 0 ? cmd.length : nl
      continue
    }
    if (!pwsh && c === '<' && cmd[i + 1] === '<' && cmd[i + 2] !== '<') {
      let j = i + 2
      const tabs = cmd[j] === '-'
      if (tabs) j++
      while (cmd[j] === ' ' || cmd[j] === '\t') j++
      const w = readWord(cmd.slice(j))
      // `$((1<<2))` 里的位移不是 heredoc
      if (w && !/^\d+$/.test(w.word)) {
        docs.push({ delim: w.word, tabs })
        i = j + w.end
        continue
      }
    }
    if (c === '(') paren++
    else if (c === ')' && paren > 0) paren--
    else if (c === '{') brace++
    else if (c === '}' && brace > 0) brace--
    if (c === '\n') {
      if (docs.length) {
        const next = skipHeredocs(cmd, i + 1, docs)
        if (paren === 0 && brace === 0) cut(i, '\n', next)
        docs.length = 0
        i = next
        continue
      }
      if (paren === 0 && brace === 0) {
        cut(i, '\n', i + 1)
        i++
        continue
      }
    } else if (paren === 0 && brace === 0) {
      let sep = ''
      let len = 1
      if (c === ';') sep = ';'
      else if (c === '|') {
        sep = cmd[i + 1] === '|' ? '||' : '|'
        len = cmd[i + 1] === '|' || cmd[i + 1] === '&' ? 2 : 1
      } else if (c === '&' && cmd[i + 1] === '&') {
        sep = '&&'
        len = 2
      } else if (!pwsh && c === '&' && cmd[i - 1] !== '>' && cmd[i + 1] !== '>') sep = '&'
      if (sep) {
        cut(i, sep, i + len)
        i += len
        continue
      }
    }
    i++
  }
  cut(cmd.length, '', cmd.length)
  return out
}

/**
 * 一条命令里调用的程序：第一个是主程序（剥掉 cd / 变量赋值 / sudo、env 这类包装之后的那个），
 * 其后是其余各段的程序（可重复，×N 按调用次数算）。`cd X &&` 不算；认不出的段落记成 shell / pwsh，
 * 只在整条都认不出时出现。
 */
export function extractPrograms(cmd: string, shell: ShellKind = 'sh'): string[] {
  if (!cmd.trim()) return []
  const generic = shell === 'pwsh' ? 'pwsh' : 'shell'
  const main = programOf(cmd, shell, new Map()) || generic
  const vars: Vars = new Map()
  const rest: string[] = []
  for (const seg of splitSegments(cmd, shell)) {
    const p = programOf(seg.text, shell, vars)
    const isCd = shell === 'pwsh' ? /^(?:cd|Set-Location|sl)$/i.test(p) : p === 'cd'
    if (isCd && (seg.sep === '&&' || seg.sep === ';' || seg.sep === '\n')) continue
    if (p && p !== generic) rest.push(p)
  }
  if (main === generic) return rest.length ? rest : [generic]
  const k = rest.indexOf(main)
  if (k >= 0) rest.splice(k, 1)
  return [main, ...rest]
}

// ---------------------------------------------------------------------------
// 结果状态
// ---------------------------------------------------------------------------

export type CmdStatus = 'running' | 'completed' | 'failed' | 'killed' | 'stopped'

/** 结果文本里认出的标记 */
export interface ExecFlags {
  /** 输出被截断（`[output truncated; …`） */
  truncated?: boolean
  /** 有 stderr 段 */
  stderr?: boolean
  timedOut?: boolean
  /** 沙箱拒绝了文件访问 */
  denied?: boolean
  signal?: string
  /** 结果只是「已转到后台」的回执，命令还在跑 */
  backgroundAck?: boolean
  /** 被停下（`[stopped: …]`、用户打断） */
  stopped?: boolean
  /** provider 报的执行耗时（Codex 的 `Wall time:`） */
  wallMs?: number
}

export interface ExecStatus {
  status: CmdStatus
  exit?: number
  /** 没有退出码标记、也没报错，按成功推断的 exit 0 */
  exitInferred: boolean
  flags: ExecFlags
  /** Codex 没跑完就先返回了：后续 write_stdin（session）/ wait（cell）的结果才是结局 */
  pending?: { kind: 'session' | 'cell'; id: string }
}

const BG_ACK = [
  // dsh
  /(?:^|\n)started background job \S+\s*$/,
  /(?:^|\n)\[still running after \d+ms; moved to background job [^\]\n]+\](?:\nThe command keeps running in the background\.[^\n]*)?\s*$/,
  // Claude Code 的 run_in_background
  /^Command running in background with ID: /,
]

/**
 * 从结果文本读出执行状态。看 dsh 的尾部标记（`[exit code: N]`、`[killed by signal: X]`、`[timed out after Nms]`、
 * `[stopped: …]`、`[sandbox: file access denied …]`、`[output truncated; …`、`[stderr]`、后台回执），
 * Codex 的 `Process exited with code N` / `Script completed|failed` 头，Claude Code 失败时的 `Exit code N` 头。
 * 都没有：provider 说失败就是 failed，否则按成功推断 exit 0。
 */
export function parseExecStatus(text: string, toolStatus?: string | null): ExecStatus {
  const flags: ExecFlags = {}
  const failed = toolFailed(toolStatus)
  const head = text.slice(0, 400)
  const wall = /^Wall time:? ([\d.]+) seconds?/m.exec(head)
  if (wall) flags.wallMs = Math.round(Number(wall[1]) * 1000)
  if (BG_ACK.some((re) => re.test(text))) {
    flags.backgroundAck = true
    return { status: 'running', exitInferred: false, flags }
  }
  // Codex：exec_command 让出时带 session，code mode 的 exec 让出时带 cell
  const yielded = /^(?:Chunk ID: [^\n]*\n)?(?:Wall time: [^\n]*\n)?Process running with session ID (\d+)/.exec(head)
  if (yielded) return { status: 'running', exitInferred: false, flags, pending: { kind: 'session', id: yielded[1]! } }
  const cell = /^Script running with cell ID (\w+)/.exec(head)
  if (cell) return { status: 'running', exitInferred: false, flags, pending: { kind: 'cell', id: cell[1]! } }
  if (/^Script failed\b/.test(head)) return { status: 'failed', exitInferred: false, flags }
  let exit: number | undefined
  const sig = /(?:^|\n)\[killed by signal: ([^\]\n]+)\]\s*$/.exec(text) ?? /(?:^|\n)\[shell killed by signal: ([^\]\n]+)\](?=\n|$)/.exec(text)
  if (sig) flags.signal = sig[1]
  else {
    const m =
      /(?:^|\n)\[exit code: (-?\d+)\]\s*$/.exec(text) ??
      /(?:^|\n)\[Command finished with exit code (-?\d+)\]\s*$/.exec(text) ??
      /(?:^|\n)\[shell exited: code (-?\d+)\](?=\n|$)/.exec(text) ??
      /^(?:Chunk ID: [^\n]*\n)?(?:Wall time: [^\n]*\n)?Process exited with code (-?\d+)/.exec(head) ??
      /^Exit code:? (-?\d+)\b/.exec(head)
    if (m) exit = Number(m[1])
    else if (/(?:^|\n)\[shell exited\](?=\n|$)/.test(text)) flags.stopped = true
  }
  if (/(?:^|\n)\[timed out after \d+ms\](?=\n|$)/.test(text) || /(?:^|\n)\[Command timed out or OOM\](?=\n|$)/.test(text) || /^Command timed out after /.test(head))
    flags.timedOut = true
  if (/(?:^|\n)\[stopped: [^\]\n]+\](?=\n|$)/.test(text)) flags.stopped = true
  if (/(?:^|\n)\[sandbox: file access denied under [^\]\n]+\](?=\n|$)/.test(text)) flags.denied = true
  if (/(?:^|\n)\[output truncated; /.test(text)) flags.truncated = true
  if (/(?:^|\n)\[stderr\]\n/.test(text)) flags.stderr = true
  return { ...settle(flags, exit, failed), flags }
}

function settle(flags: ExecFlags, exit: number | undefined, failed: boolean): Omit<ExecStatus, 'flags'> {
  if (flags.signal != null) return { status: 'killed', exit, exitInferred: false }
  if (flags.stopped) return { status: 'stopped', exit, exitInferred: false }
  if (exit == null) {
    if (flags.denied || flags.timedOut || failed) return { status: 'failed', exitInferred: false }
    return { status: 'completed', exit: 0, exitInferred: true }
  }
  return { status: exit === 0 ? 'completed' : 'failed', exit, exitInferred: false }
}

// ---------------------------------------------------------------------------
// 从事件流收集
// ---------------------------------------------------------------------------

export interface CmdRow {
  /** 唯一键：call_id，没有就 seq */
  key: string
  callId: string
  /** 调用事件的 seq；嵌套命令是 dispatch 行的 seq */
  seq: number
  /** 结果事件的 seq */
  resultSeq?: number
  /** 嵌套在 dsh run_code 里：外层调用的 seq */
  parentSeq?: number
  /** 在浏览器里定位用：结果（或外层结果）行，没有就调用行 */
  revealSeq: number
  /** 原始工具名 */
  tool: string
  /** sh / pwsh 是 shell 命令；code 是 run_code 的代码 */
  lang: ShellKind | 'code'
  cmd: string
  programs: string[]
  /** 主程序 = programs[0] */
  program: string
  description?: string
  cwd?: string
  timeoutMs?: number
  background: boolean
  sandbox?: string
  ts?: string
  /** 毫秒时间戳 */
  start?: number
  end?: number
  /** 墙钟时长；还在跑 / 没有结果时没有 */
  ms?: number
  status: CmdStatus
  exit?: number
  exitInferred: boolean
  /** 时长和状态是从调用 / 结果两端推的（没有 job 记录；并行调用会把时长算长） */
  derived: boolean
  flags: ExecFlags
  /** provider 报的失败 */
  err: boolean
  /** 结果的字符数 */
  outChars?: number
  turn?: number
  step?: number
}

/** ≥ 这个时长算「慢」 */
export const SLOW_MS = 10_000

const SHELL_TOOLS = new Set(['bash', 'powershell', 'pwsh', 'run_code', 'shell', 'shell_command', 'exec_command', 'exec', 'local_shell', 'container.exec'])
/** shell 类里不是在跑命令的：往已有会话里写 stdin（它的结果续在让出的那条命令上） */
const NOT_COMMANDS = new Set(['write_stdin'])

type Obj = Record<string, unknown>

function asObj(v: unknown): Obj | null {
  return v && typeof v === 'object' && !Array.isArray(v) ? (v as Obj) : null
}
function str(v: unknown): string | undefined {
  return typeof v === 'string' && v !== '' ? v : undefined
}
function num(v: unknown): number | undefined {
  const n = typeof v === 'string' && v.trim() ? Number(v) : v
  return typeof n === 'number' && Number.isFinite(n) ? n : undefined
}
function argsOf(input: unknown): Obj {
  const o = asObj(input)
  if (o) return o
  if (typeof input === 'string') {
    try {
      const v = JSON.parse(input)
      return asObj(v) ?? { command: input }
    } catch {
      return { command: input }
    }
  }
  return {}
}
function msOf(ts: string | undefined): number | undefined {
  if (!ts) return undefined
  const n = Date.parse(ts)
  return Number.isFinite(n) ? n : undefined
}

const SH_BIN = /^(?:bash|sh|zsh|dash|ksh|fish)$/i
const PWSH_BIN = /^(?:powershell|pwsh)$/i

/** Codex 的 argv（`["bash","-lc","…"]`、`["powershell.exe","-Command","…"]`）还原成脚本 */
function fromArgv(argv: string[]): { cmd: string; lang: ShellKind } {
  const bin = baseName(argv[0] ?? '')
  if (SH_BIN.test(bin)) {
    const k = argv.findIndex((a, i) => i > 0 && /^-[a-z]*c[a-z]*$/.test(a))
    if (k > 0 && argv[k + 1] != null) return { cmd: argv[k + 1]!, lang: 'sh' }
  }
  if (PWSH_BIN.test(bin)) {
    const k = argv.findIndex((a, i) => i > 0 && /^-(?:c|command)$/i.test(a))
    if (k > 0) return { cmd: argv.slice(k + 1).join(' '), lang: 'pwsh' }
  }
  if (/^cmd$/i.test(bin)) {
    const k = argv.findIndex((a, i) => i > 0 && /^\/[ck]$/i.test(a))
    if (k > 0) return { cmd: argv.slice(k + 1).join(' '), lang: 'sh' }
  }
  return { cmd: argv.map((a) => (/[\s"'$;&|]/.test(a) ? JSON.stringify(a) : a)).join(' '), lang: 'sh' }
}

interface Parsed {
  cmd: string
  lang: ShellKind | 'code'
  description?: string
  cwd?: string
  timeoutMs?: number
  background: boolean
  sandbox?: string
}

/** 各家的参数形状 → 命令文本和附带信息；认不出命令就 null */
function parseArgs(tool: string, input: unknown): Parsed | null {
  const name = tool.toLowerCase()
  // Codex 的 code mode：exec 的输入是一段 JS 原文
  if (name === 'exec' && typeof input === 'string') return input.trim() ? { cmd: input, lang: 'code', background: false } : null
  const a = argsOf(input)
  let lang: ShellKind | 'code' = name === 'powershell' || name === 'pwsh' ? 'pwsh' : name === 'run_code' ? 'code' : 'sh'
  let cmd: string | undefined
  if (lang === 'code') cmd = str(a.code) ?? str(a.command)
  else {
    const raw = a.command ?? a.cmd ?? asObj(a.action)?.command ?? a.script
    if (Array.isArray(raw)) {
      const argv = raw.filter((x): x is string => typeof x === 'string')
      if (argv.length) {
        const r = fromArgv(argv)
        cmd = r.cmd
        lang = r.lang
      }
    } else cmd = str(raw)
    // exec_command 可以指定 shell；Codex 没指定时猜
    const sh = str(a.shell)
    if (sh) lang = PWSH_BIN.test(baseName(sh)) ? 'pwsh' : 'sh'
    else if (cmd && (name === 'exec_command' || name === 'shell_command')) lang = guessLang(cmd)
  }
  if (!cmd) return null
  const timeout = num(a.timeoutMs) ?? num(a.timeout_ms) ?? num(a.timeout)
  const sandbox = str(a.sandbox_permissions) ?? str(a.sandbox)
  return {
    cmd,
    lang,
    description: str(a.description) ?? str(a.justification),
    cwd: str(a.workdir) ?? str(a.cwd) ?? str(a.working_directory),
    timeoutMs: timeout != null && timeout > 0 ? timeout : undefined,
    background: a.run_in_background === true || a.background === true,
    sandbox,
  }
}

/** 结果的文本：字符串原样；part 数组拼 text（tool-result 里嵌的也展开）；last 是最后一段，用来认尾部标记 */
function outputText(v: unknown): { full: string; last: string } {
  if (typeof v === 'string') return { full: v, last: v }
  const parts: string[] = []
  const walk = (x: unknown) => {
    if (typeof x === 'string') {
      parts.push(x)
      return
    }
    if (Array.isArray(x)) {
      for (const p of x) walk(p)
      return
    }
    const o = asObj(x)
    if (!o) return
    if (typeof o.text === 'string') parts.push(o.text)
    else if (Array.isArray(o.content)) walk(o.content)
    else if (typeof o.output === 'string') parts.push(o.output)
  }
  walk(v)
  return { full: parts.join('\n'), last: parts.at(-1) ?? '' }
}

interface JobRec {
  start?: number
  end?: number
  status?: 'completed' | 'failed' | 'killed'
  detail?: string
}

function jobCallId(d: Obj | null): string | undefined {
  return str(asObj(d?.toolCall)?.callId) ?? str(d?.callId)
}

/** job/settled 覆盖状态和时间：exit / signal 从 detail（`exit code: 1; signal: SIGTERM`）里读 */
function applyJob(r: CmdRow, j: JobRec | undefined): void {
  if (!j) return
  if (j.start != null) r.start = j.start
  if (j.end == null || !j.status) return
  r.end = j.end
  const exit = j.detail ? /(?:^|;\s*)exit code: (-?\d+)\b/.exec(j.detail)?.[1] : undefined
  const signal = j.detail ? /(?:^|;\s*)signal: ([^;]+)/.exec(j.detail)?.[1]?.trim() : undefined
  r.status = j.status === 'completed' && exit != null && exit !== '0' ? 'failed' : j.status
  if (exit != null) r.exit = Number(exit)
  if (signal) r.flags = { ...r.flags, signal }
  r.exitInferred = false
  r.derived = false
}

/** pfh 的结果是结构化的：{ stdout, stderr, exitCode, signal, timedOut, stdoutTruncated, cwd } 或后台的 { taskId, status } */
function structuredStatus(o: Obj): ExecStatus | null {
  if (str(o.taskId) && !('exitCode' in o)) return { status: 'running', exitInferred: false, flags: { backgroundAck: true } }
  if (!('exitCode' in o) && !('timedOut' in o)) return null
  const flags: ExecFlags = {}
  const exit = num(o.exitCode)
  if (str(o.signal)) flags.signal = str(o.signal)
  if (o.timedOut === true) flags.timedOut = true
  if (o.stdoutTruncated === true || o.stderrTruncated === true) flags.truncated = true
  if (typeof o.stderr === 'string' && o.stderr.trim()) flags.stderr = true
  return { ...settle(flags, exit, false), flags }
}

/** 结果的字符数：结构化的数 stdout + stderr */
function outChars(raw: unknown, text: string): number {
  const o = asObj(raw)
  if (o && ('stdout' in o || 'stderr' in o)) return String(o.stdout ?? '').length + String(o.stderr ?? '').length
  return text.length
}

interface ResultIn {
  raw: unknown
  status?: string
  ts?: string
  /** Claude Code 结果行的 toolUseResult */
  extra?: Obj | null
  /** provider 报的耗时（pfh 的 durationMs） */
  ms?: number
}

/** 结果到了：状态、标记、输出长度、结束时间；返回解析出的状态（Codex 让出时带 pending） */
function applyResult(r: CmdRow, res: ResultIn): ExecStatus {
  const text = outputText(res.raw)
  const o = asObj(res.raw)
  const st = (o && structuredStatus(o)) ?? parseExecStatus(text.last || text.full, res.status)
  if (o && str(o.cwd)) r.cwd = str(o.cwd)
  // Claude Code：toolUseResult 带 stdout / stderr / interrupted / backgroundTaskId
  const tur = res.extra
  if (tur) {
    if (typeof tur.stderr === 'string' && tur.stderr.trim()) st.flags.stderr = true
    if (str(tur.backgroundTaskId) && !st.flags.backgroundAck) {
      st.flags.backgroundAck = true
      st.status = 'running'
      st.exitInferred = false
      st.exit = undefined
    } else if (tur.interrupted === true) {
      st.status = 'stopped'
      st.flags.stopped = true
      st.exitInferred = false
      if (st.exit === 0) st.exit = undefined
    }
  }
  r.err = toolFailed(res.status)
  r.flags = { ...r.flags, ...st.flags }
  if (res.ms != null && res.ms >= 0) r.flags.wallMs = res.ms
  const chars = outChars(res.raw, text.full)
  if (chars) r.outChars = (r.outChars ?? 0) + chars
  if (st.flags.backgroundAck) {
    r.background = true
    return st
  }
  r.status = st.status
  if (st.status === 'running') return st
  // 代码执行的退出码说的是代码运行器，不是程序
  if (r.lang !== 'code') {
    r.exit = st.exit
    r.exitInferred = st.exitInferred
  }
  r.end = msOf(res.ts)
  r.derived = true
  return st
}

/** Codex code mode（exec）的 JS 里调用的 exec_command：取出 cmd 字符串和指定的 shell */
function codeCommands(code: string): { cmd: string; lang: ShellKind }[] {
  const out: { cmd: string; lang: ShellKind }[] = []
  const re = /exec_command\(\s*\{/g
  for (let m = re.exec(code); m; m = re.exec(code)) {
    const from = m.index + m[0].length
    const body = code.slice(from, from + 4000)
    const key = /(?:^|[,{\s])["']?cmd["']?\s*:\s*(["'`])/.exec(body)
    if (!key) continue
    const lit = jsString(body, key.index + key[0].length - 1)
    if (!lit) continue
    const head = body.slice(0, key.index) + body.slice(lit.end, lit.end + 300)
    const sh = /["']?shell["']?\s*:\s*["']([^"']+)["']/.exec(head)?.[1]
    out.push({ cmd: lit.value, lang: sh ? (PWSH_BIN.test(baseName(sh)) ? 'pwsh' : 'sh') : guessLang(lit.value) })
  }
  return out
}

/** 从 s[at]（引号）读一个 JS 字符串字面量并反转义；模板串里有 ${} 的原样留着 */
function jsString(s: string, at: number): { value: string; end: number } | null {
  const q = s[at]
  let v = ''
  for (let i = at + 1; i < s.length; i++) {
    const c = s[i]!
    if (c === q) return { value: v, end: i + 1 }
    if (c !== '\\') {
      v += c
      continue
    }
    const n = s[++i]
    if (n === 'n') v += '\n'
    else if (n === 't') v += '\t'
    else if (n === 'r') v += '\r'
    else if (n === 'u' && /^[0-9a-fA-F]{4}$/.test(s.slice(i + 1, i + 5))) {
      v += String.fromCharCode(parseInt(s.slice(i + 1, i + 5), 16))
      i += 4
    } else if (n != null) v += n
  }
  return null
}

/** Codex 没指定 shell 时猜一下：出现 PowerShell 的 Verb-Noun cmdlet、$env: 或 here-string 就当 pwsh（Windows 上默认 PowerShell） */
function guessLang(cmd: string): ShellKind {
  return /(?:^|[;|&(\n]\s*)(?:Get|Set|Select|Where|ForEach|Remove|New|Test|Write|Out|Invoke|Start|Stop|Copy|Move|Format|Sort|Measure|Import|Export|ConvertTo|ConvertFrom|Add|Resolve|Split|Join)-[A-Z][A-Za-z]+\b|\$env:|@['"]\r?\n/.test(cmd)
    ? 'pwsh'
    : 'sh'
}

/**
 * 会话里所有 shell / 代码执行，按调用顺序。
 * 时长：dsh 有 job/registered、job/settled 就用它们；其余是调用 → 结果的墙钟（derived），Codex 报了 Wall time 用它。
 */
export function collectCommands(events: Event[]): CmdRow[] {
  const results = new Map<string, Event>()
  const jobs = new Map<string, JobRec>()
  for (const ev of events) {
    if (ev.kind === 'tool_result' && ev.tool?.call_id) {
      results.set(ev.tool.call_id, ev)
      continue
    }
    const ty = str(ev.ext?.type)
    if (ty !== 'job/registered' && ty !== 'job/settled') continue
    const d = asObj(ev.ext.data)
    const id = jobCallId(d)
    if (!id) continue
    const j = jobs.get(id) ?? {}
    if (ty === 'job/registered') j.start = num(d?.startedAt) ?? j.start
    else {
      const st = d?.status
      const end = num(d?.finishedAt)
      if (end != null && (st === 'completed' || st === 'failed' || st === 'killed')) {
        j.end = end
        j.status = st
        j.detail = str(d?.detail)
      }
    }
    jobs.set(id, j)
  }

  const rows: CmdRow[] = []
  const byCall = new Map<string, CmdRow>()
  /** dsh run_code 里嵌套的命令：外层 call_id → 子命令，外层结果到了再定位 */
  const nested = new Map<string, CmdRow[]>()
  /** Codex 让出的命令：session / cell id → 那一行 */
  const pendingSessions = new Map<string, CmdRow>()
  const pendingCells = new Map<string, CmdRow>()

  const make = (base: { callId: string; seq: number; tool: string; ts?: string; turn?: number; step?: number }, p: Parsed): CmdRow => {
    // 代码执行：Codex 的 JS 里调了 exec_command 就数那些命令的程序，否则记成工具名（run_code / exec）
    const inner = p.lang === 'code' ? codeCommands(p.cmd).flatMap((c) => extractPrograms(c.cmd, c.lang)) : []
    const programs = p.lang !== 'code' ? extractPrograms(p.cmd, p.lang) : inner.length ? inner : [base.tool]
    const r: CmdRow = {
      key: base.callId || `s${base.seq}`,
      callId: base.callId,
      seq: base.seq,
      revealSeq: base.seq,
      tool: base.tool,
      lang: p.lang,
      cmd: p.cmd,
      programs,
      program: programs[0] ?? (p.lang === 'pwsh' ? 'pwsh' : 'shell'),
      background: p.background,
      ts: base.ts,
      start: msOf(base.ts),
      status: 'running',
      exitInferred: false,
      derived: false,
      flags: {},
      err: false,
      turn: base.turn,
      step: base.step,
    }
    if (p.description) r.description = p.description
    if (p.cwd) r.cwd = p.cwd
    if (p.timeoutMs != null) r.timeoutMs = p.timeoutMs
    if (p.sandbox) r.sandbox = p.sandbox
    return r
  }

  for (const ev of events) {
    const ty = str(ev.ext?.type)
    // dsh：run_code 里调用的工具，开始一行、结束一行
    if (ty === 'tool/ptc-dispatch-start' || ty === 'tool/code-dispatch-start' || ty === 'tool/ptc-dispatch' || ty === 'tool/code-dispatch') {
      const d = asObj(ev.ext.data)
      const name = str(d?.name) ?? ''
      const root = str(d?.rootCallId)
      const sub = str(d?.subCallId) ?? ''
      if (!root || !SHELL_TOOLS.has(name.toLowerCase())) continue
      const list = nested.get(root) ?? []
      const done = ty === 'tool/ptc-dispatch' || ty === 'tool/code-dispatch'
      let r = sub ? list.find((x) => x.callId === sub) : undefined
      if (!r) {
        const p = parseArgs(name, d?.arguments)
        if (!p) continue
        r = make({ callId: sub, seq: ev.seq, tool: name, ts: ev.ts, turn: ev.turn?.n ?? undefined, step: ev.step?.n ?? undefined }, p)
        // 只有结束行：不知道什么时候开始的
        if (done) r.start = undefined
        list.push(r)
        nested.set(root, list)
      }
      if (done) applyResult(r, { raw: d?.content, status: d?.isError === true ? 'error' : 'ok', ts: ev.ts })
      continue
    }
    if (ev.kind !== 'tool_call' || !ev.tool) continue
    const name = ev.tool.name
    const lower = name.toLowerCase()
    const res = ev.tool.call_id ? results.get(ev.tool.call_id) : undefined
    // Codex 的续写：write_stdin（session_id）/ wait（cell_id）的结果是之前让出的那条命令的结局
    if (lower === 'write_stdin' || lower === 'wait') {
      const a = argsOf(ev.tool.input)
      const id = lower === 'wait' ? str(a.cell_id) ?? (num(a.cell_id) != null ? String(a.cell_id) : undefined) : num(a.session_id) != null ? String(a.session_id) : str(a.session_id)
      const map = lower === 'wait' ? pendingCells : pendingSessions
      const r = id ? map.get(id) : undefined
      if (!r || !res?.tool) continue
      const st = applyResult(r, { raw: res.tool.output, status: res.tool.status, ts: res.ts })
      // 续写结果里的 Wall time 只是这一次等待的，总时长按调用 → 最终结果算
      delete r.flags.wallMs
      if (st.status !== 'running') {
        map.delete(id!)
        r.revealSeq = res.seq
      }
      continue
    }
    if (NOT_COMMANDS.has(lower) || (ev.tool.category !== 'shell' && !SHELL_TOOLS.has(lower))) continue
    const p = parseArgs(name, ev.tool.input)
    if (!p) continue
    const r = make({ callId: ev.tool.call_id, seq: ev.seq, tool: name, ts: ev.ts, turn: ev.turn?.n ?? undefined, step: ev.step?.n ?? undefined }, p)
    if (res?.tool) {
      r.resultSeq = res.seq
      r.revealSeq = res.seq
      const ext = res.ext as Obj
      const meta = asObj(asObj(ext.data)?.meta)
      const cwd = str(meta?.cwd)
      if (cwd) r.cwd = cwd
      const st = applyResult(r, { raw: res.tool.output, status: res.tool.status, ts: res.ts, extra: asObj(ext.toolUseResult), ms: num(ext.durationMs) })
      if (st.pending) (st.pending.kind === 'cell' ? pendingCells : pendingSessions).set(st.pending.id, r)
    }
    rows.push(r)
    if (r.callId) byCall.set(r.callId, r)
  }

  // 嵌套命令挂到外层 run_code 上：跳转看外层结果
  for (const [root, list] of nested) {
    const parent = byCall.get(root)
    for (const r of list) {
      if (parent) {
        r.parentSeq = parent.seq
        r.revealSeq = parent.resultSeq ?? parent.seq
      }
      rows.push(r)
    }
  }

  for (const r of rows) {
    if (r.callId) applyJob(r, jobs.get(r.callId))
    if (r.flags.wallMs != null && r.status !== 'running') {
      r.ms = r.flags.wallMs
      r.derived = false
    } else if (r.start != null && r.end != null && r.status !== 'running') r.ms = Math.max(0, r.end - r.start)
  }
  return rows.sort((a, b) => (a.parentSeq ?? a.seq) - (b.parentSeq ?? b.seq) || a.seq - b.seq)
}

// ---------------------------------------------------------------------------
// 过滤、排序、汇总
// ---------------------------------------------------------------------------

export type CmdFilter = 'all' | 'failed' | 'slow' | 'background'
export type CmdSort = 'latest' | 'slowest'

export function isFailed(r: CmdRow): boolean {
  return r.status === 'failed' || r.status === 'killed' || r.status === 'stopped'
}

export function matchFilter(r: CmdRow, f: CmdFilter): boolean {
  if (f === 'failed') return isFailed(r)
  if (f === 'slow') return r.ms != null && r.ms >= SLOW_MS
  if (f === 'background') return r.background
  return true
}

/** 搜索：命令、描述、cwd、程序名里包含（不分大小写） */
export function matchQuery(r: CmdRow, q: string): boolean {
  const s = q.trim().toLowerCase()
  if (!s) return true
  return (
    r.cmd.toLowerCase().includes(s) ||
    (r.description?.toLowerCase().includes(s) ?? false) ||
    (r.cwd?.toLowerCase().includes(s) ?? false) ||
    r.programs.some((p) => p.toLowerCase().includes(s))
  )
}

export function sortCommands(rows: CmdRow[], by: CmdSort): CmdRow[] {
  const latest = (a: CmdRow, b: CmdRow) => (b.start ?? 0) - (a.start ?? 0) || b.seq - a.seq
  return rows.slice().sort(by === 'slowest' ? (a, b) => (b.ms ?? -1) - (a.ms ?? -1) || latest(a, b) : latest)
}

export interface CmdSummary {
  /** 结束了的 */
  runs: number
  failed: number
  running: number
  /** 结束了的墙钟时长之和 */
  ms: number
  background: number
}

export function summarizeCommands(rows: CmdRow[]): CmdSummary {
  const s: CmdSummary = { runs: 0, failed: 0, running: 0, ms: 0, background: 0 }
  for (const r of rows) {
    if (r.background) s.background++
    if (r.status === 'running') {
      s.running++
      continue
    }
    s.runs++
    if (isFailed(r)) s.failed++
    s.ms += r.ms ?? 0
  }
  return s
}

export interface CmdNotable {
  /** 最慢的一次（≥ 1s 才算） */
  slowest?: CmdRow
  /** 失败最多的程序 */
  failing?: { program: string; count: number }
  /** 重复最多的同一条命令（≥ 2 次） */
  repeats?: { cmd: string; count: number }
  background?: number
}

export function notableCommands(rows: CmdRow[]): CmdNotable {
  const out: CmdNotable = {}
  const fails = new Map<string, number>()
  const repeats = new Map<string, number>()
  let bg = 0
  for (const r of rows) {
    if (r.background) bg++
    if (isFailed(r)) for (const p of new Set(r.programs)) fails.set(p, (fails.get(p) ?? 0) + 1)
    if (r.status === 'running') continue
    if (r.ms != null && r.ms >= 1000 && (!out.slowest || r.ms > (out.slowest.ms ?? 0))) out.slowest = r
    repeats.set(r.cmd, (repeats.get(r.cmd) ?? 0) + 1)
  }
  for (const [program, count] of fails) if (!out.failing || count > out.failing.count) out.failing = { program, count }
  for (const [cmd, count] of repeats) if (count >= 2 && (!out.repeats || count > out.repeats.count)) out.repeats = { cmd, count }
  if (bg) out.background = bg
  return out
}

export interface ProgramStat {
  name: string
  /** 调用次数（一条命令里出现几次算几次） */
  n: number
  /** 含它的命令条数 */
  runs: number
}

export function programStats(rows: CmdRow[]): ProgramStat[] {
  const m = new Map<string, ProgramStat>()
  for (const r of rows) {
    const seen = new Set<string>()
    for (const p of r.programs) {
      let s = m.get(p)
      if (!s) m.set(p, (s = { name: p, n: 0, runs: 0 }))
      s.n++
      if (!seen.has(p)) {
        s.runs++
        seen.add(p)
      }
    }
  }
  return [...m.values()].sort((a, b) => b.n - a.n || a.name.localeCompare(b.name))
}

export interface CommandLog {
  rows: CmdRow[]
  summary: CmdSummary
  notable: CmdNotable
  programs: ProgramStat[]
}

/** 一次算齐：行、摘要、值得注意、程序分布（后三者与列表过滤无关） */
export function commandLog(events: Event[]): CommandLog {
  const rows = collectCommands(events)
  return { rows, summary: summarizeCommands(rows), notable: notableCommands(rows), programs: programStats(rows) }
}
