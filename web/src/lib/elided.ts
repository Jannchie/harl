// slim 事件里省略掉的 base64：会话页 provide 当前快照 id，JsonBlock 展开时按占位去取原文
import type { InjectionKey, Ref } from 'vue'
import { api, elidedIn } from '../api'

/** 当前会话页的快照 id；没有 provide（不在会话页）就不提供取原文 */
export const SNAPSHOT_ID: InjectionKey<Readonly<Ref<string>>> = Symbol('snapshot-id')

/** 把一段文本里的占位换回原文。base64 里没有 JSON 要转义的字符，pretty JSON 里也能直接替换 */
export async function restoreElided(id: string, text: string, signal?: AbortSignal): Promise<string> {
  const list = elidedIn(text)
  const bodies = await Promise.all(list.map((e) => api.blob(id, e.seq, e.ord, signal)))
  let out = text
  list.forEach((e, i) => (out = out.replaceAll(e.marker, bodies[i]!)))
  return out
}
