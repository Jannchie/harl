// 页面标题：标签页、历史记录和读屏的换页播报都靠 document.title。
// 默认是路由 meta.title（消息 key），页面可以用 usePageTitle 覆盖（会话页用会话标题）；
// 两者和界面语言都在同一个 watchEffect 里，换页、换语言、覆盖值变化都会重算。
import { onUnmounted, ref, watchEffect } from 'vue'
import type { Router } from 'vue-router'
import { t } from '../i18n'

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
