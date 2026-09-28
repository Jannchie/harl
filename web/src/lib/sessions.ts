// 会话列表：侧栏和会话页共用一份，只在首次用到和手动刷新时拉。
import { computed, shallowRef } from 'vue'
import { api, ApiError, type SessionInfo, type Status } from '../api'

export const sessions = shallowRef<SessionInfo[] | null>(null)
export const sessionsError = shallowRef<string | null>(null)
export const status = shallowRef<Status | null>(null)
export const loading = shallowRef(false)

/** machine id → 主机名；还没加载到 status、或是没登记过的机器，就用 id 本身 */
export function machineName(id: string): string {
  return status.value?.machines?.[id] ?? id
}

export function describe(e: unknown): string {
  if (e instanceof ApiError) return e.message
  return e instanceof Error ? e.message : String(e)
}

let inflight: Promise<void> | null = null

export function loadSessions(force = false): Promise<void> {
  if (inflight) return inflight
  if (sessions.value && !force) return Promise.resolve()
  loading.value = true
  sessionsError.value = null
  inflight = (async () => {
    const [s, st] = await Promise.allSettled([api.sessions(), api.status()])
    if (s.status === 'fulfilled') sessions.value = s.value
    else sessionsError.value = describe(s.reason)
    if (st.status === 'fulfilled') status.value = st.value
  })().finally(() => {
    loading.value = false
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
