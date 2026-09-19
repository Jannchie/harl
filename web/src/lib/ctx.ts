import type { InjectionKey, Ref } from 'vue'
import type { SessionInfo } from '../api'

/** session id（文件名那个）→ 列表项，用来把 links.session 换成路由用的 tree id */
export const SESSIONS_KEY: InjectionKey<Ref<Map<string, SessionInfo>>> = Symbol('sessions')
