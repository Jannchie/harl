const UNITS = ['B', 'KiB', 'MiB', 'GiB', 'TiB']

export function fmtBytes(n: number | undefined | null): string {
  if (n == null || !Number.isFinite(n)) return '–'
  let v = n
  let i = 0
  while (v >= 1024 && i < UNITS.length - 1) {
    v /= 1024
    i++
  }
  return `${i === 0 ? v : v < 10 ? v.toFixed(2) : v < 100 ? v.toFixed(1) : Math.round(v)} ${UNITS[i]}`
}

export function fmtInt(n: number | undefined | null): string {
  if (n == null || !Number.isFinite(n)) return '–'
  return n.toLocaleString('en-US')
}

/** 1234 → 1.2k，1234567 → 1.23M（token 数用） */
export function fmtCompact(n: number | undefined | null): string {
  if (n == null || !Number.isFinite(n)) return '–'
  if (n < 1000) return String(n)
  if (n < 1_000_000) return `${(n / 1000).toFixed(n < 10_000 ? 1 : 0)}k`
  return `${(n / 1_000_000).toFixed(2)}M`
}

/** 大数压缩：27.2k / 384.9M / 5.0B。小于 1 万照原样加千分位 */
export function fmtBig(n: number | undefined | null): string {
  if (n == null || !Number.isFinite(n)) return '–'
  const abs = Math.abs(n)
  if (abs < 10_000) return Math.round(n).toLocaleString('en-US')
  const units: [number, string][] = [
    [1e12, 'T'],
    [1e9, 'B'],
    [1e6, 'M'],
    [1e3, 'k'],
  ]
  for (const [div, u] of units) {
    if (abs >= div) {
      const v = n / div
      return `${v.toFixed(Math.abs(v) < 100 ? 1 : 0)}${u}`
    }
  }
  return String(n)
}

/** 美元：$0.35 / $153 / $3.1k / $1.2M */
export function fmtMoney(usd: number | undefined | null): string {
  if (usd == null || !Number.isFinite(usd)) return '–'
  const abs = Math.abs(usd)
  const sign = usd < 0 ? '-' : ''
  if (abs === 0) return '$0'
  if (abs < 0.01) return `${sign}$${abs.toFixed(4)}`
  if (abs < 10) return `${sign}$${abs.toFixed(2)}`
  if (abs < 1000) return `${sign}$${abs.toFixed(abs < 100 ? 1 : 0)}`
  if (abs < 1e6) return `${sign}$${(abs / 1e3).toFixed(abs < 1e5 ? 1 : 0)}k`
  return `${sign}$${(abs / 1e6).toFixed(1)}M`
}

/** 精确美元（表格 / tooltip）：$2,545.17 */
export function fmtMoneyExact(usd: number | undefined | null): string {
  if (usd == null || !Number.isFinite(usd)) return '–'
  return `$${usd.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`
}

/** 毫秒时长：812ms / 12.3s / 5m 12s / 97h 12m（小时一直用到 1000h 才换天） */
export function fmtMs(ms: number | undefined | null): string {
  if (ms == null || !Number.isFinite(ms)) return '–'
  if (ms < 1000) return `${Math.round(ms)}ms`
  const s = ms / 1000
  if (s < 60) return `${s.toFixed(1)}s`
  const m = Math.floor(s / 60)
  if (m < 60) return `${m}m ${pad(Math.round(s - m * 60))}s`
  const h = Math.floor(m / 60)
  if (h < 1000) return `${h}h ${pad(m - h * 60)}m`
  const d = Math.floor(h / 24)
  return `${d}d ${h - d * 24}h`
}

/** 0.83 → 83.2%；digits 控制小数位 */
export function fmtPct(ratio: number | undefined | null, digits = 1): string {
  if (ratio == null || !Number.isFinite(ratio)) return '–'
  return `${(ratio * 100).toFixed(digits)}%`
}

function pad(n: number): string {
  return n < 10 ? `0${n}` : String(n)
}

/** 本地时间 `YYYY-MM-DD HH:mm:ss` */
export function fmtDateTime(iso: string | undefined | null): string {
  if (!iso) return '–'
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return iso
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`
}

/** 本地时间 `HH:mm:ss`（同一天内看事件流用） */
export function fmtClock(iso: string | undefined | null): string {
  if (!iso) return '–'
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return iso
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`
}

export function fmtRelative(iso: string | undefined | null, now = Date.now()): string {
  if (!iso) return '–'
  const t = new Date(iso).getTime()
  if (Number.isNaN(t)) return iso
  const s = Math.round((now - t) / 1000)
  const abs = Math.abs(s)
  const suffix = s >= 0 ? '前' : '后'
  if (abs < 60) return `${abs} 秒${suffix}`
  if (abs < 3600) return `${Math.round(abs / 60)} 分钟${suffix}`
  if (abs < 86400) return `${Math.round(abs / 3600)} 小时${suffix}`
  if (abs < 86400 * 30) return `${Math.round(abs / 86400)} 天${suffix}`
  return fmtDateTime(iso)
}

/** 两个 RFC3339 之间的时长，`1.2s` / `3m 05s` */
export function fmtDuration(from: string | undefined, to: string | undefined): string | null {
  if (!from || !to) return null
  const ms = new Date(to).getTime() - new Date(from).getTime()
  if (!Number.isFinite(ms) || ms < 0) return null
  if (ms < 1000) return `${ms}ms`
  const s = ms / 1000
  if (s < 60) return `${s.toFixed(1)}s`
  const m = Math.floor(s / 60)
  const rs = Math.round(s - m * 60)
  if (m < 60) return `${m}m ${pad(rs)}s`
  const h = Math.floor(m / 60)
  return `${h}h ${pad(m - h * 60)}m`
}

/** 路径缩短：保留首段和末两段，中间用 … */
export function shortenPath(p: string, max = 60): string {
  if (p.length <= max) return p
  const parts = p.split('/')
  if (parts.length <= 3) return `…${p.slice(-max + 1)}`
  const tail = parts.slice(-2).join('/')
  // 绝对路径开头是 ''，`/…/x`；Windows 盘符或相对路径保留首段
  const out = parts[0] === '' ? `/${parts[1] ?? ''}/…/${tail}` : `${parts[0]}/…/${tail}`
  return out.length <= max ? out : `…/${tail}`.slice(-max)
}

export function truncate(s: string, max: number): string {
  if (s.length <= max) return s
  return `${s.slice(0, max - 1)}…`
}

/** 紧凑单行 JSON，超长截断 */
export function compactJson(v: unknown, max = 160): string {
  let s: string
  try {
    s = typeof v === 'string' ? v : JSON.stringify(v)
  } catch {
    s = String(v)
  }
  if (s == null) return ''
  s = s.replace(/\s+/g, ' ')
  return truncate(s, max)
}

export function prettyJson(v: unknown): string {
  try {
    return JSON.stringify(v, null, 2) ?? ''
  } catch {
    return String(v)
  }
}
