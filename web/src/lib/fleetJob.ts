// Agent 网络里一张别的会话的卡片要的数字和协作记录：拉事件、折一遍、扫一遍。
// 一个家族可能有几十上百个子会话，每个都要 JSON.parse + 折叠，放在 worker 里跑（fleet.worker.ts），
// 主线程只收结果；worker 起不来时 fleetWorker.ts 退回在主线程直接调这里
import { api, type Provider } from '../api'
import { foldContext } from './context'
import { fleetStats, summarize, type FleetStats } from './ctxView'
import { scanCollab, type CollabDigest } from './fleet'
import { ensurePricing } from './pricing'

export interface FleetJob {
  /** 快照 id */
  id: string
  session: string
  provider: Provider
  /** 要卡片数字 */
  stats: boolean
  /** 要协作记录 */
  collab: boolean
}

export interface FleetJobResult {
  stats?: FleetStats
  collab?: CollabDigest
}

const tick = () => new Promise<void>((r) => setTimeout(r, 0))

export async function fleetJob(job: FleetJob, signal?: AbortSignal): Promise<FleetJobResult> {
  const pricing = ensurePricing()
  const evs = await api.events(job.id, signal)
  const out: FleetJobResult = {}
  if (job.stats) {
    await pricing
    const f = foldContext(evs)
    out.stats = fleetStats(f, summarize(f))
  }
  if (job.collab) {
    // 主线程上跑时大会话扫起来要一会儿，先让出一下
    await tick()
    out.collab = scanCollab(job.session, job.provider, evs)
  }
  return out
}
