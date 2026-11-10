// 主线程这边的 fleet worker：一个共用的 worker，按 id 对回结果。
// worker 起不来（不支持 module worker、脚本加载失败）就退回在主线程跑 fleetJob，结果一样，只是会卡一下
import type { FleetJob, FleetJobResult } from './fleetJob'

interface Pending {
  job: FleetJob
  signal: AbortSignal
  resolve: (r: FleetJobResult) => void
  reject: (e: unknown) => void
}

/** undefined：还没试过；null：用不了 */
let worker: Worker | null | undefined
let nextId = 0
const pending = new Map<number, Pending>()

function local(job: FleetJob, signal: AbortSignal): Promise<FleetJobResult> {
  return import('./fleetJob').then((m) => m.fleetJob(job, signal))
}

/** worker 挂了：以后都在主线程跑，手上没回来的也改在主线程重跑 */
function giveUp() {
  worker?.terminate()
  worker = null
  const list = [...pending.values()]
  pending.clear()
  for (const p of list) if (!p.signal.aborted) local(p.job, p.signal).then(p.resolve, p.reject)
}

function getWorker(): Worker | null {
  if (worker !== undefined) return worker
  try {
    const w = new Worker(new URL('./fleet.worker.ts', import.meta.url), { type: 'module' })
    w.onmessage = (e: MessageEvent<{ id: number; result?: FleetJobResult; error?: string }>) => {
      const p = pending.get(e.data.id)
      if (!p) return
      pending.delete(e.data.id)
      if (e.data.error != null) p.reject(new Error(e.data.error))
      else p.resolve(e.data.result ?? {})
    }
    w.onerror = (e) => {
      e.preventDefault()
      giveUp()
    }
    worker = w
  } catch {
    worker = null
  }
  return worker
}

function abortError() {
  return new DOMException('aborted', 'AbortError')
}

/** 在 worker 里拉一个会话的事件、算卡片数字 / 协作记录 */
export function runFleetJob(job: FleetJob, signal: AbortSignal): Promise<FleetJobResult> {
  if (signal.aborted) return Promise.reject(abortError())
  const w = getWorker()
  if (!w) return local(job, signal)
  return new Promise((resolve, reject) => {
    const id = ++nextId
    pending.set(id, { job, signal, resolve, reject })
    w.postMessage({ id, job })
    signal.addEventListener(
      'abort',
      () => {
        if (!pending.delete(id)) return
        worker?.postMessage({ cancel: id })
        reject(abortError())
      },
      { once: true },
    )
  })
}
