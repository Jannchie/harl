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
    // 跟着 style.css 的 token 走，深浅色在那边一处定（文字色都按 AAA 对比度挑过）
    colors: {
      ground: 'var(--bg)',
      rail: 'var(--rail)',
      elevated: 'var(--elevated)',
      'elevated-hover': 'var(--elevated-hover)',
      line: 'var(--line)',
      accent: 'var(--accent)',
      ink: {
        DEFAULT: 'var(--fg)',
        strong: 'var(--fg-strong)',
        muted: 'var(--fg-1)',
        soft: 'var(--fg-2)',
        accent: 'var(--accent-ink)',
        danger: 'var(--danger-ink)',
        warning: 'var(--warning-ink)',
        success: 'var(--ok-ink)',
        selected: 'var(--selected-fg)',
      },
    },
  },
  shortcuts: {
    'max-w-chat': 'max-w-760px',
    'leading-chat': 'leading-[1.7]',
    'chat-text': 'text-sm',
    'chat-prose':
      'max-w-none prose prose-neutral prose-code:text-sm prose-h1:text-xl prose-h2:text-lg prose-h3:text-base prose-h3:font-semibold prose-h4:text-base [&>*]:mt-0 dark:prose-invert prose-code:after:content-none prose-code:before:content-none',
    'page-ground': 'bg-ground',
    surface: 'bg-black/4 dark:bg-white/5',
    // 深色悬停只叠 6%：卡片里再悬停时次要文字仍要 ≥ 7:1
    'surface-hover': 'hover:bg-black/6 dark:hover:bg-white/6',
    'surface-strong': 'bg-black/6 dark:bg-white/6',
    'ink-muted': 'text-ink-muted',
    'ink-strong': 'text-ink-strong',
    'ink-soft': 'text-ink-soft',
    'ink-danger': 'text-ink-danger',
    'ink-warning': 'text-ink-warning',
    'ink-accent': 'text-ink-accent',
    'ink-success': 'text-ink-success',
    // 名字不能以 focus- 开头：UnoCSS 会把它读成 focus: 变体
    'kbd-ring':
      'outline-none focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-1 focus-visible:outline-blue-500/70',
    'kbd-ring-flush': 'outline-none focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-blue-500/70',
    'transition-quick': 'transition-all duration-150',
    popover:
      'z-50 rounded-xl bg-elevated p-1 shadow-xl ring-1 ring-black/10 dark:ring-white/10',
    field:
      'w-full rounded-lg border border-black/12 bg-white px-3 py-2 text-sm placeholder:text-ink-soft kbd-ring-flush dark:border-white/12 dark:bg-white/4',
    'button-secondary':
      'flex items-center gap-2 rounded-lg border border-black/12 px-3 py-1.5 text-sm surface-hover kbd-ring disabled:opacity-50 dark:border-white/12',
    'settings-card': 'rounded-xl border border-line',
    'settings-divide': 'divide-y divide-line',
    segmented: 'inline-flex flex-wrap gap-0.5 rounded-lg p-0.5 surface',
    segment: 'flex items-center gap-1.5 rounded-md px-2.5 py-0.5 text-xs kbd-ring transition-quick',
    'segment-selected': 'bg-white text-ink shadow-sm dark:bg-white/12',
    'menu-item': 'w-full flex items-center gap-2 rounded-lg px-2 py-1.5 text-left text-sm surface-hover kbd-ring',
    'pill-selected': 'bg-[var(--selected-bg)] text-ink-selected',
    // 侧栏的一行（导航、会话）：当前所在的那行是比悬停深一档的中性底，不用强调色（选中不是状态）
    'rail-item': 'flex items-center rounded-md py-1.5 text-left kbd-ring transition-colors duration-150',
    'rail-item-idle': 'ink-muted surface-hover hover:text-ink',
    'row-selected': 'bg-[var(--row-selected-bg)] text-ink font-medium',
    // 展开的行把自己那一行钉在顶上；--pin-top 是上面已经钉着的东西（会话页的头）
    'row-pin': 'sticky top-[var(--pin-top,0px)] z-2 page-ground',
    'fold-chevron': 'h-3.5 w-3.5 shrink-0 ink-muted transition-transform duration-150',
    'meter-track': 'overflow-hidden rounded-full bg-black/10 dark:bg-white/10',
    'meter-fill': 'block h-full rounded-full',
    // 会变的数字：正文字体 + 等宽数位（宽度稳定）。等宽字体只给代码、ID、路径、URL、键帽这类要逐字符对齐的东西（font-mono）
    num: 'tabular-nums',
    'row-action':
      'shrink-0 opacity-0 transition-quick group-hover:opacity-100 focus-visible:opacity-100 group-focus-within:opacity-100',
    'payload-block':
      'm-0 overflow-auto whitespace-pre-wrap break-words rounded-lg bg-black/4 p-2.5 text-xs font-mono dark:bg-white/5',
    'icon-button-base': 'flex items-center justify-center rounded-md ink-soft surface-hover kbd-ring transition-quick',
    'icon-button': 'h-8 w-8 icon-button-base',
    'icon-button-sm': 'h-7 w-7 icon-button-base',
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
      collections: {
        jannchie: () => import('@jannchie/iconify-json/icons.json').then((m) => m.default),
      },
      extraProperties: {
        display: 'inline-block',
        'vertical-align': 'middle',
      },
    }),
    // preset 自带的灰（序号、图注、引文是 neutral-400/500）达不到 AAA，文字类的颜色指到 token。
    // token 自己会随深浅色切换，所以 invert 那一半填同一个值
    presetTypography({
      colorScheme: Object.fromEntries(
        Object.entries({
          body: '--fg',
          headings: '--fg',
          lead: '--fg-1',
          links: '--accent-ink',
          bold: '--fg',
          counters: '--fg-2',
          quotes: '--fg-1',
          captions: '--fg-2',
          code: '--fg',
          kbd: '--fg',
        }).map(([k, v]) => [k, [`var(${v})`, `var(${v})`]]),
      ),
      cssExtend: {
        code: { fontFamily: 'inherit' },
      },
    }),
    presetWebFonts({
      provider: 'google',
      fonts: {
        mono: [...MONO, { name: 'Noto Sans SC', weights: ['400..900'], italic: false }],
      },
    }),
  ],
})
