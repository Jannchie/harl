// 模型 id → 上下文窗口 / 最大输出。dsh 的 request/context 自己报窗口，其他 provider 的日志不记，只能查目录。
// 目录就是价格用的那份 models.dev api.json：llm-pricing 只解析价格、不暴露 limit，
// 所以等 ensurePricing() 拉完后，从同一个 Cache API 桶（'llm-pricing'，键是源 URL）读原文自己解析 limit。
// 不另外联网：Cache API 不可用（非安全上下文）或拉取失败只剩内置快照时，一律返回 null（显示"日志没记上下文窗口"）。
import { computed, onMounted, ref, type ComputedRef } from 'vue'
import { DEFAULT_PROVIDER_PRIORITY, peelRoutingTiers, pricingCandidates } from 'llm-pricing'
import { ensurePricing } from './pricing'

/** 和 llm-pricing 的 modelsDevSource() 默认 URL 一致（也是它写缓存用的键） */
const MODELS_DEV_URL = 'https://models.dev/api.json'
/** pricing.ts 里 browserCache() 打开的桶名 */
const CACHE_NAME = 'llm-pricing'

export interface ModelLimit {
  /** 上下文窗口（models.dev 的 limit.context） */
  context: number
  /** 单次最大输出（limit.output）；目录没写就是 null */
  output: number | null
  /** 部分模型的输入上限比窗口小（如 OpenAI 的 limit.input）；没写就是 null */
  input: number | null
}

let limits: Map<string, ModelLimit> | null = null
let inflight: Promise<void> | null = null
/** 每次读到目录 +1，让依赖它的 computed 重算 */
const version = ref(0)

const posInt = (v: unknown): number | null => (typeof v === 'number' && Number.isFinite(v) && v > 0 ? Math.floor(v) : null)

/** 按 llm-pricing 的 parseModelsDev 同样的键建索引：provider/model 全键；裸 id 按 provider 优先级先到先得；带 / 的复合 id 最后补 */
function indexLimits(json: unknown): Map<string, ModelLimit> {
  const map = new Map<string, ModelLimit>()
  if (!json || typeof json !== 'object') return map
  const root = json as Record<string, { models?: Record<string, { limit?: Record<string, unknown> }> }>
  const rank = new Map<string, number>(DEFAULT_PROVIDER_PRIORITY.map((id, i) => [id, i]))
  const providers = Object.keys(root).sort((a, b) => {
    const ra = rank.get(a) ?? Number.POSITIVE_INFINITY
    const rb = rank.get(b) ?? Number.POSITIVE_INFINITY
    return ra === rb ? a.localeCompare(b) : ra - rb
  })
  const compound = new Map<string, ModelLimit>()
  for (const p of providers) {
    const models = root[p]?.models
    if (!models || typeof models !== 'object') continue
    for (const [id, m] of Object.entries(models)) {
      const context = posInt(m?.limit?.context)
      if (context == null) continue
      const lim: ModelLimit = { context, output: posInt(m?.limit?.output), input: posInt(m?.limit?.input) }
      const bare = id.toLowerCase()
      map.set(`${p.toLowerCase()}/${bare}`, lim)
      if (bare.includes('/')) {
        if (!compound.has(bare)) compound.set(bare, lim)
      } else if (!map.has(bare)) map.set(bare, lim)
    }
  }
  for (const [k, v] of compound) if (!map.has(k)) map.set(k, v)
  return map
}

/** 从 Cache API 读 llm-pricing 存的 {fetchedAt, body}；没有就 null */
async function readCatalog(): Promise<unknown> {
  if (typeof caches === 'undefined') return null
  const res = await (await caches.open(CACHE_NAME)).match(MODELS_DEV_URL)
  if (!res) return null
  const entry = JSON.parse(await res.text()) as { body?: unknown }
  return typeof entry.body === 'string' ? JSON.parse(entry.body) : null
}

/** 加载一次目录里的 limit。成功后就不再读；没读到（还没缓存）下次调用会再试。失败不抛 */
export function ensureModelWindows(): Promise<void> {
  if (limits) return Promise.resolve()
  inflight ??= (async () => {
    try {
      await ensurePricing()
      const map = indexLimits(await readCatalog())
      if (map.size) {
        limits = map
        version.value++
      }
    } catch (err) {
      console.warn('[modelWindow]', err)
    } finally {
      inflight = null
    }
  })()
  return inflight
}

/** 同步查：目录还没读到、模型不在目录里都是 null。名字的变体（日期后缀、点号、路由前缀）沿用 llm-pricing 的解析 */
export function modelLimit(model: string | null | undefined): ModelLimit | null {
  void version.value
  if (!model || !limits) return null
  for (const form of [model, ...peelRoutingTiers(model)]) {
    for (const c of pricingCandidates(form)) {
      const hit = limits.get(c)
      if (hit) return hit
    }
  }
  return null
}

/** 组件里用：挂载时触发加载，目录读到后自动重算 */
export function useModelLimit(model: () => string | null | undefined): ComputedRef<ModelLimit | null> {
  onMounted(() => void ensureModelWindows())
  return computed(() => modelLimit(model()))
}
