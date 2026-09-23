// 配色：跟随系统 / 浅 / 深，写在 <html data-scheme>（UnoCSS 的 dark: 变体和 style.css 的 token 都认它）。
// 首帧由 index.html 里的内联脚本设好，这里接管之后的切换和系统配色变化。
import { ref, watch } from 'vue'

export type SchemeMode = 'system' | 'light' | 'dark'

const KEY = 'harl:scheme'
const mq = window.matchMedia('(prefers-color-scheme: dark)')

function stored(): SchemeMode {
  try {
    const v = localStorage.getItem(KEY)
    if (v === 'light' || v === 'dark') return v
  } catch {
    // 隐私模式等拿不到 storage，就跟随系统
  }
  return 'system'
}

export const schemeMode = ref<SchemeMode>(stored())
export const scheme = ref<'light' | 'dark'>(document.documentElement.dataset.scheme === 'dark' ? 'dark' : 'light')

function apply() {
  const dark = schemeMode.value === 'dark' || (schemeMode.value === 'system' && mq.matches)
  scheme.value = dark ? 'dark' : 'light'
  document.documentElement.dataset.scheme = scheme.value
}

watch(schemeMode, (m) => {
  try {
    if (m === 'system') localStorage.removeItem(KEY)
    else localStorage.setItem(KEY, m)
  } catch {
    // 同上
  }
  apply()
})
mq.addEventListener('change', apply)

/** 系统 → 浅 → 深 → 系统 */
export function cycleScheme() {
  schemeMode.value = schemeMode.value === 'system' ? 'light' : schemeMode.value === 'light' ? 'dark' : 'system'
}
