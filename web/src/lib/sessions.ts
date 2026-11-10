// 会话列表：侧栏和会话页共用一份，首次用到时拉；路由 id 是快照哈希，每次 capture 都会变，
// 所以用的地方发现列表里没有当前 id 时会强制再拉一次（见 SessionView）。
import { computed, shallowRef } from 'vue'
import { api, ApiError, type SessionInfo, type Status } from '../api'

/** 一份远端数据的状态。重新加载、加载失败时 prev 留着上一次成功的数据，界面不必清空 */
export type Resource<T> =
  | { kind: 'idle' }
  | { kind: 'loading'; prev?: T }
  | { kind: 'ready'; data: T; at: number }
  | { kind: 'failed'; error: string; prev?: T }

function dataOf<T>(r: Resource<T>): T | null {
  if (r.kind === 'ready') return r.data
  if (r.kind === 'loading' || r.kind === 'failed') return r.prev ?? null
  return null
}

const sessionsRes = shallowRef<Resource<SessionInfo[]>>({ kind: 'idle' })
const statusRes = shallowRef<Resource<Status>>({ kind: 'idle' })

/** 只读：要改走 loadSessions */
export const sessionsState = computed(() => sessionsRes.value)
export const statusState = computed(() => statusRes.value)
export const sessions = computed(() => dataOf(sessionsRes.value))
export const sessionsError = computed(() => (sessionsRes.value.kind === 'failed' ? sessionsRes.value.error : null))
export const status = computed(() => dataOf(statusRes.value))
export const statusError = computed(() => (statusRes.value.kind === 'failed' ? statusRes.value.error : null))
export const loading = computed(() => sessionsRes.value.kind === 'loading' || statusRes.value.kind === 'loading')

/** machine id → 主机名；还没加载到 status、或是没登记过的机器，就用 id 本身 */
export function machineName(id: string): string {
  return status.value?.machines?.[id] ?? id
}

export function describe(e: unknown): string {
  if (e instanceof ApiError) return e.message
  return e instanceof Error ? e.message : String(e)
}

function settle<T>(res: PromiseSettledResult<T>, prev: T | undefined): Resource<T> {
  return res.status === 'fulfilled'
    ? { kind: 'ready', data: res.value, at: Date.now() }
    : { kind: 'failed', error: describe(res.reason), prev }
}

let inflight: Promise<void> | null = null

export function loadSessions(force = false): Promise<void> {
  if (inflight) return inflight
  if (!force && sessionsRes.value.kind === 'ready' && statusRes.value.kind === 'ready') return Promise.resolve()
  const prevSessions = dataOf(sessionsRes.value) ?? undefined
  const prevStatus = dataOf(statusRes.value) ?? undefined
  sessionsRes.value = { kind: 'loading', prev: prevSessions }
  statusRes.value = { kind: 'loading', prev: prevStatus }
  inflight = (async () => {
    const [s, st] = await Promise.allSettled([api.sessions(), api.status()])
    sessionsRes.value = settle(s, prevSessions)
    statusRes.value = settle(st, prevStatus)
  })().finally(() => {
    inflight = null
  })
  return inflight
}

/** session id（文件名那个）→ 列表项；同名多条（多机器 / 多路径）取最新 captured 的 */
export const bySession = computed(() => {
  const m = new Map<string, SessionInfo>()
  const list = [...(sessions.value ?? [])].sort((a, b) => (a.captured_at < b.captured_at ? -1 : 1))
  for (const s of list) m.set(s.session, s)
  return m
})
