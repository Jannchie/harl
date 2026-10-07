import { onMounted, onUnmounted, ref, type Ref } from 'vue'

/** 媒体查询的响应式结果，跟着变化更新；组件卸载时摘掉监听 */
export function useMediaQuery(query: string): Ref<boolean> {
  const matches = ref(false)
  let mq: MediaQueryList | null = null
  const update = () => (matches.value = !!mq?.matches)
  onMounted(() => {
    mq = window.matchMedia(query)
    update()
    mq.addEventListener('change', update)
  })
  onUnmounted(() => mq?.removeEventListener('change', update))
  return matches
}

/** 窄屏：和模板里 `max-md:` 同一条（UnoCSS wind4 的 md 是 48rem，生成的就是 width < 48rem） */
export const NARROW = '(width < 48rem)'

/** 宽屏：和模板里 `xl:` 同一条（wind4 的 xl 是 80rem） */
export const WIDE = '(width >= 80rem)'
