// 界面语言：localStorage 里存过的优先，没有就按浏览器语言挑，都不匹配用英文。
// 文案在 src/locales/<语言>/<命名空间>.ts；zh-CN 是 schema，其他语言用 satisfies 对齐（缺键、多键 vue-tsc 都会报）。
// 组件里用 useI18n() 的 t，ts 模块里用这里导出的 t（读的是同一个响应式 locale，computed 里调用会跟着切换）。
import { computed } from 'vue'
import { createI18n } from 'vue-i18n'
import en from './locales/en'
import ja from './locales/ja'
import zhCN from './locales/zh-CN'

export type Messages = typeof zhCN

declare module 'vue-i18n' {
  // eslint-disable-next-line @typescript-eslint/no-empty-object-type
  export interface DefineLocaleMessage extends Messages {}
}

export const LOCALES = [
  { id: 'en', label: 'English' },
  { id: 'zh-CN', label: '简体中文' },
  { id: 'ja', label: '日本語' },
] as const

export type Locale = (typeof LOCALES)[number]['id']

const KEY = 'harl:locale'

function isLocale(v: unknown): v is Locale {
  return LOCALES.some((l) => l.id === v)
}

function detect(): Locale {
  try {
    const v = localStorage.getItem(KEY)
    if (isLocale(v)) return v
  } catch {
    // 隐私模式等拿不到 storage，就按浏览器语言
  }
  for (const tag of navigator.languages ?? [navigator.language]) {
    const lower = tag.toLowerCase()
    if (lower.startsWith('zh')) return 'zh-CN'
    if (lower.startsWith('ja')) return 'ja'
    if (lower.startsWith('en')) return 'en'
  }
  return 'en'
}

export const i18n = createI18n<[Messages], Locale, false>({
  legacy: false,
  locale: detect(),
  fallbackLocale: 'en',
  messages: { en, 'zh-CN': zhCN, ja },
  // 文案里不会出现 HTML；关掉警告，也避免有人往里塞
  warnHtmlMessage: false,
})

export const t = i18n.global.t

/** 当前语言，Intl.* 和 toLocaleString 用它 */
export const locale = computed<Locale>(() => i18n.global.locale.value)

export function setLocale(l: Locale) {
  i18n.global.locale.value = l
  document.documentElement.lang = l
  try {
    localStorage.setItem(KEY, l)
  } catch {
    // 同上
  }
}

document.documentElement.lang = locale.value
