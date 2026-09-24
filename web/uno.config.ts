// 设计语言照搬 preferred-harness（apps/web/uno.config.ts）：一个底色 + 一层半透明 surface，
// 区分靠字重和留白而不是叠面板；只有状态（失败、进行中）才带颜色。shortcut 名字与 pfh 保持一致。
import { defineConfig, presetIcons, presetTypography, presetWebFonts, presetWind4 } from 'unocss'

// 等宽首选 Berkeley Mono（@font-face 在 style.css，从 cdn.jannchie.com 拉）；
// 它没有中文，中文回退到本机的 Sarasa（中西文严格 2:1 等宽），再回退 Noto Sans SC
const MONO = ['Berkeley Mono', 'Sarasa Term SC Nerd', 'Sarasa Mono SC Nerd', 'Sarasa Term SC', 'Sarasa Mono SC'].map(
  (name) => ({ name, provider: 'none' as const }),
)

export default defineConfig({
  content: {
    pipeline: {
      // ts 模块里写的类名（工具图标表等）也要扫到
      include: [/\.(vue|[jt]sx|html)($|\?)/, /\/src\/.+\.[jt]s($|\?)/],
    },
  },
  theme: {
    colors: {
      elevated: '#1e1e1f',
      'elevated-hover': '#252526',
    },
    font: {
      num: 'ui-monospace, SFMono-Regular, Menlo, Consolas, monospace',
    },
  },
  shortcuts: {
    'max-w-chat': 'max-w-760px',
    'leading-chat': 'leading-[1.7]',
    'chat-text': 'text-sm',
    'chat-prose':
      'max-w-none prose prose-neutral prose-code:text-sm prose-h1:text-xl prose-h2:text-lg prose-h3:text-base prose-h3:font-semibold prose-h4:text-base [&>*]:mt-0 dark:prose-invert prose-code:after:content-none prose-code:before:content-none',
    'page-ground': 'bg-neutral-50 dark:bg-neutral-950',
    surface: 'bg-black/4 dark:bg-white/5',
    'surface-hover': 'hover:bg-black/6 dark:hover:bg-white/8',
    'surface-strong': 'bg-black/6 dark:bg-white/8',
    'ink-muted': 'text-neutral-600 dark:text-neutral-400',
    'ink-strong': 'text-neutral-700 dark:text-neutral-200',
    'ink-soft': 'text-neutral-500 dark:text-neutral-400',
    'ink-danger': 'text-red-800 dark:text-red-300',
    'ink-warning': 'text-amber-900 dark:text-amber-400',
    'ink-accent': 'text-blue-800 dark:text-blue-400',
    'ink-success': 'text-green-700 dark:text-green-400',
    // 名字不能以 focus- 开头：UnoCSS 会把它读成 focus: 变体
    'kbd-ring':
      'outline-none focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-1 focus-visible:outline-blue-500/70',
    'kbd-ring-flush': 'outline-none focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-blue-500/70',
    'transition-quick': 'transition-all duration-150',
    popover:
      'z-50 rounded-xl bg-neutral-50 p-1 shadow-xl ring-1 ring-black/10 dark:bg-elevated dark:ring-white/10',
    field:
      'w-full rounded-lg border border-neutral-300 bg-white px-3 py-2 text-sm placeholder:text-neutral-400 kbd-ring-flush dark:border-neutral-700 dark:bg-neutral-950 dark:placeholder:text-neutral-500',
    'button-secondary':
      'flex items-center gap-2 rounded-lg border border-neutral-300 px-3 py-1.5 text-sm surface-hover kbd-ring disabled:opacity-50 dark:border-neutral-700',
    'settings-card': 'rounded-xl border border-neutral-200 dark:border-neutral-800',
    'settings-divide': 'divide-y divide-neutral-200 dark:divide-neutral-800',
    segmented: 'inline-flex flex-wrap gap-0.5 rounded-lg p-0.5 surface',
    segment: 'flex items-center gap-1.5 rounded-md px-2.5 py-0.5 text-xs kbd-ring transition-quick',
    'segment-selected': 'bg-white text-neutral-900 shadow-sm dark:bg-neutral-700 dark:text-neutral-50',
    'menu-item': 'w-full flex items-center gap-2 rounded-lg px-2 py-1.5 text-left text-sm surface-hover kbd-ring',
    'pill-selected': 'bg-blue-500/25 text-blue-700 dark:text-blue-200',
    // 展开的行把自己那一行钉在顶上；--pin-top 是上面已经钉着的东西（会话页的头）
    'row-pin': 'sticky top-[var(--pin-top,0px)] z-2 page-ground',
    'fold-chevron': 'h-3.5 w-3.5 shrink-0 ink-muted transition-transform duration-150',
    'meter-track': 'overflow-hidden rounded-full bg-black/10 dark:bg-white/10',
    'meter-fill': 'block h-full rounded-full',
    num: 'font-num tabular-nums',
    'row-action':
      'shrink-0 opacity-0 transition-quick group-hover:opacity-100 focus-visible:opacity-100 group-focus-within:opacity-100',
    'payload-block':
      'm-0 overflow-auto whitespace-pre-wrap break-words rounded-lg bg-black/4 p-2.5 text-xs font-mono dark:bg-white/5',
    'icon-button':
      'h-8 w-8 flex items-center justify-center rounded-md ink-soft surface-hover kbd-ring transition-quick',
    // 仪表盘的一块：和 pfh 的 island 同一个形状
    card: 'rounded-2xl surface p-4',
    'card-title': 'flex items-center gap-2 text-sm font-medium ink-strong',
  },
  presets: [
    presetWind4({
      dark: {
        dark: '[data-scheme="dark"]',
        light: '[data-scheme="light"]',
      },
    }),
    presetIcons({
      extraProperties: {
        display: 'inline-block',
        'vertical-align': 'middle',
      },
    }),
    presetTypography({
      cssExtend: {
        code: { fontFamily: 'inherit' },
      },
    }),
    presetWebFonts({
      provider: 'google',
      fonts: {
        sans: [{ name: 'Inter', weights: [400, 500, 600], italic: false }],
        mono: [...MONO, { name: 'Noto Sans SC', weights: ['400..900'], italic: false }],
        num: MONO,
      },
    }),
  ],
})
