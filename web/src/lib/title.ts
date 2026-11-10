// 页面标题：标签页、历史记录和读屏的换页播报都靠 document.title。
// 默认是路由 meta.title（消息 key），页面可以用 usePageTitle 覆盖（会话页用会话标题）；
// 两者和界面语言都在同一个 watchEffect 里，换页、换语言、覆盖值变化都会重算。
// 另有会话显示名（sessionTitle，定义在不依赖界面的 agents.ts，worker 里也能用）：侧栏、会话页、Fleet、仪表盘都走它，
// 同一个会话到处叫同一个名字。
import { computed, onUnmounted, ref, watchEffect } from 'vue'
import type { Router } from 'vue-router'
import type { SessionInfo } from '../api'
import { t } from '../i18n'
import { sessionTitle } from './agents'
import { sessions } from './sessions'

export { sessionTitle }

const byId = computed(() => new Map((sessions.value ?? []).map((s) => [s.id, s] as const)))

/** 只有快照 id 和标题的地方（仪表盘的会话行）：会话列表里有这个快照就用它的显示名，否则用给的标题 */
export function titleOf(id: string, title: string | null): string | null {
  const s: SessionInfo | undefined = byId.value.get(id)
  return s ? (sessionTitle(s) ?? title) : title
}

const override = ref<string | null>(null)

/** 当前页面的标题覆盖路由默认值；组件卸载时撤掉 */
export function usePageTitle(getter: () => string | null | undefined) {
  const stop = watchEffect(() => (override.value = getter() ?? null))
  onUnmounted(() => {
    stop()
    override.value = null
  })
}

export function installTitle(router: Router) {
  watchEffect(() => {
    const key = router.currentRoute.value.meta.title as string | undefined
    const title = override.value ?? (key ? t(key) : null)
    document.title = title ? `${title} · harl` : 'harl'
  })
}
