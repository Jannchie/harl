// Agent 网络卡片的后台计算（见 fleetJob.ts）。消息：{ id, job } 开始一个，{ cancel: id } 取消；
// 回 { id, result } 或 { id, error }
import { fleetJob, type FleetJob } from './fleetJob'

type In = { id: number; job: FleetJob } | { cancel: number }

const running = new Map<number, AbortController>()

self.onmessage = async (e: MessageEvent<In>) => {
  const msg = e.data
  if ('cancel' in msg) {
    running.get(msg.cancel)?.abort()
    running.delete(msg.cancel)
    return
  }
  const ac = new AbortController()
  running.set(msg.id, ac)
  try {
    const result = await fleetJob(msg.job, ac.signal)
    if (!ac.signal.aborted) self.postMessage({ id: msg.id, result })
  } catch (err) {
    if (!ac.signal.aborted) self.postMessage({ id: msg.id, error: err instanceof Error ? err.message : String(err) })
  } finally {
    running.delete(msg.id)
  }
}
