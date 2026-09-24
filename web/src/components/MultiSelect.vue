<script setup lang="ts">
import { computed, nextTick, onUnmounted, ref, useId, watch } from 'vue'

// 照 pfh 的 MultiSelect：点选不关菜单（选八个就是八下，不是二十四下），选项多时带过滤框。
// 触发器是一个小胶囊：没选时安静，选了就用选中色，一眼看出哪些维度在过滤。
//
// 键盘与读屏按 WAI-ARIA APG：
// - 选项多（带搜索）：combobox + listbox。焦点一直在输入框，方向键移动的是 aria-activedescendant
// - 选项少：listbox 本身拿焦点，同样用 aria-activedescendant；可以直接打字跳到开头匹配的选项
// 两种都是多选 listbox（aria-multiselectable + aria-selected），Enter 切换选中、菜单不关。
//
// 触发器：↓ / ↑ / Enter / Space 打开（↑ 高亮到最后一项）
// 菜单里：↑↓ 移动，PageUp / PageDown 跳 10 项，Home / End 到头尾（带搜索时 Home / End 留给输入框的光标），
//        Enter 切换（不带搜索时 Space 也行），Ctrl/⌘+A 全选或全不选（当前可见的），
//        Esc 先清空搜索词、再关闭并把焦点还给触发器，Alt+↑ 直接关闭，Tab 自然移走焦点时关闭。
// 输入法组字时（isComposing）不拦任何按键，不然选字的回车会被当成"选中"。
export interface Option {
  id: string
  label: string
  /** 选项前的小圆点（颜色类） */
  dot?: string
}

const props = defineProps<{
  options: readonly Option[]
  /** 维度名：「Agent」「模型」 */
  label: string
  disabled?: boolean
}>()

const selected = defineModel<string[]>({ required: true })

/** 超过这么多选项才给过滤框 */
const SEARCH_THRESHOLD = 8
const PAGE = 10

const uid = useId()
const listId = `${uid}-list`
const optionId = (i: number) => `${uid}-opt-${i}`

const open = ref(false)
const root = ref<HTMLElement | null>(null)
const trigger = ref<HTMLButtonElement | null>(null)
const search = ref<HTMLInputElement | null>(null)
const list = ref<HTMLElement | null>(null)
const query = ref('')
/** 高亮的选项在 visible 里的下标；-1 是没有 */
const active = ref(-1)

const searchable = computed(() => props.options.length > SEARCH_THRESHOLD)
const chosen = computed(() => new Set(selected.value))
const summary = computed(() => {
  const n = selected.value.length
  if (n === 0) return '全部'
  if (n === 1) return props.options.find((o) => o.id === selected.value[0])?.label ?? selected.value[0]
  return `${n} 项`
})
const visible = computed(() => {
  const q = query.value.trim().toLowerCase()
  return q ? props.options.filter((o) => o.label.toLowerCase().includes(q)) : props.options
})

/** 渲染用：选项文字按搜索词切成段（命中的加粗）。只随过滤结果重算，悬停移动高亮不重算 */
const rows = computed(() => {
  const q = query.value.trim().toLowerCase()
  return visible.value.map((o) => {
    const i = q ? o.label.toLowerCase().indexOf(q) : -1
    const parts =
      i < 0
        ? [{ text: o.label, hit: false }]
        : [
            { text: o.label.slice(0, i), hit: false },
            { text: o.label.slice(i, i + q.length), hit: true },
            { text: o.label.slice(i + q.length), hit: false },
          ].filter((p) => p.text)
    return { o, parts }
  })
})

const activeId = computed(() => (open.value && active.value >= 0 ? optionId(active.value) : undefined))

/** 给读屏的一句话：过滤后剩几项 */
const status = computed(() => {
  if (!open.value || !query.value.trim()) return ''
  const n = visible.value.length
  return n ? `${n} 个结果` : `没有匹配的${props.label}`
})

/** 写回选中：按选项自己的顺序存，而不是点的顺序 */
function commit(next: Set<string>) {
  selected.value = props.options.filter((o) => next.has(o.id)).map((o) => o.id)
}

function toggle(id: string) {
  const next = new Set(chosen.value)
  if (!next.delete(id)) next.add(id)
  commit(next)
}

function toggleActive() {
  const o = visible.value[active.value]
  if (o) toggle(o.id)
}

/** 当前可见的都选上；已经全选了就都取消 */
function toggleAllVisible() {
  const ids = visible.value.map((o) => o.id)
  const next = new Set(chosen.value)
  const all = ids.every((id) => next.has(id))
  for (const id of ids) {
    if (all) next.delete(id)
    else next.add(id)
  }
  commit(next)
}

/** 键盘移动高亮：夹在范围内，并把它滚进视野（只在列表内部滚）。鼠标悬停直接改 active，不滚 */
function setActive(i: number) {
  const n = visible.value.length
  active.value = n === 0 ? -1 : Math.max(0, Math.min(n - 1, i))
  const id = activeId.value
  if (id) void nextTick(() => document.getElementById(id)?.scrollIntoView({ block: 'nearest' }))
}

// 过滤词变了：高亮回到第一个结果；只是选项刷新了（打开期间汇总重算）：按 id 保住高亮
watch([visible, query], ([now, q], [before, prevQ]) => {
  if (q !== prevQ) return setActive(0)
  const id = before[active.value]?.id
  const i = id ? now.findIndex((o) => o.id === id) : -1
  if (i >= 0) active.value = i
  else if (active.value >= now.length) setActive(now.length - 1)
})

/** 菜单里拿焦点的那个：带搜索是输入框，不带是列表 */
function focusMenu() {
  ;(searchable.value ? search.value : list.value)?.focus()
}

async function openMenu(at: 'first' | 'last' | 'selected' = 'selected') {
  if (props.disabled) return
  open.value = true
  const firstChosen = visible.value.findIndex((o) => chosen.value.has(o.id))
  setActive(at === 'last' ? visible.value.length - 1 : at === 'first' || firstChosen < 0 ? 0 : firstChosen)
  await nextTick()
  focusMenu()
}

function closeMenu(returnFocus: boolean) {
  if (!open.value) return
  open.value = false
  if (returnFocus) trigger.value?.focus()
}

function onTriggerKey(e: KeyboardEvent) {
  if (e.isComposing) return
  if (e.key === 'ArrowDown' || e.key === 'Enter' || e.key === ' ') {
    e.preventDefault()
    void openMenu(e.key === 'ArrowDown' ? 'first' : 'selected')
  } else if (e.key === 'ArrowUp') {
    e.preventDefault()
    void openMenu('last')
  }
}

// 不带搜索时打字跳转：连续按键拼成前缀，停顿 500ms 清空；同一个字母反复按就在同首字母的选项间轮换
let typed = ''
let typedTimer: ReturnType<typeof setTimeout> | undefined
function typeahead(ch: string) {
  clearTimeout(typedTimer)
  typedTimer = setTimeout(() => (typed = ''), 500)
  typed = typed === ch ? ch : typed + ch
  const opts = visible.value
  const n = opts.length
  const from = typed.length === 1 ? active.value + 1 : active.value
  for (let k = 0; k < n; k++) {
    const i = (from + k + n) % n
    if (opts[i]!.label.toLowerCase().startsWith(typed)) return setActive(i)
  }
}

function onMenuKey(e: KeyboardEvent) {
  if (e.isComposing) return
  // 焦点在"清除选择"之类的按钮上时，除了 Esc 其余键都交还给按钮自己
  const inMenu = e.target === search.value || e.target === list.value
  if (e.key === 'Escape') {
    e.preventDefault()
    e.stopPropagation()
    if (inMenu && query.value) query.value = ''
    else closeMenu(true)
    return
  }
  if (!inMenu) return
  const n = visible.value.length
  switch (e.key) {
    case 'ArrowDown':
      e.preventDefault()
      setActive(active.value < 0 ? 0 : active.value + 1)
      return
    case 'ArrowUp':
      e.preventDefault()
      if (e.altKey) return closeMenu(true)
      setActive(active.value < 0 ? n - 1 : active.value - 1)
      return
    case 'PageDown':
      e.preventDefault()
      setActive(active.value + PAGE)
      return
    case 'PageUp':
      e.preventDefault()
      setActive(active.value - PAGE)
      return
    case 'Home':
    case 'End':
      // 输入框里的 Home / End 是移光标
      if (searchable.value) return
      e.preventDefault()
      setActive(e.key === 'Home' ? 0 : n - 1)
      return
    case 'Enter':
      e.preventDefault()
      toggleActive()
      return
  }
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'a') {
    // 输入框里有字时 Ctrl+A 留给"全选文字"
    if (searchable.value && query.value) return
    e.preventDefault()
    toggleAllVisible()
    return
  }
  if (!searchable.value && !e.ctrlKey && !e.metaKey && !e.altKey) {
    if (e.key === ' ') {
      e.preventDefault()
      // 打字跳转中途的空格算前缀的一部分
      if (typed) typeahead(' ')
      else toggleActive()
    } else if (e.key.length === 1) {
      e.preventDefault()
      typeahead(e.key.toLowerCase())
    }
  }
}

/** 清除按钮点完就不在了：焦点挪回输入框 / 列表，别掉到 body 上把菜单关掉 */
function clearAll() {
  selected.value = []
  focusMenu()
}

/** 焦点离开整个组件（Tab 走了、点了别处的输入框）就收起，不抢焦点 */
function onFocusOut(e: FocusEvent) {
  const next = e.relatedTarget as Node | null
  if (next && root.value?.contains(next)) return
  closeMenu(false)
}

function onOutside(e: PointerEvent) {
  if (root.value && !root.value.contains(e.target as Node)) closeMenu(false)
}

watch(open, (v) => {
  if (v) document.addEventListener('pointerdown', onOutside)
  else {
    document.removeEventListener('pointerdown', onOutside)
    query.value = ''
    active.value = -1
    typed = ''
  }
})
watch(
  () => props.disabled,
  (d) => d && closeMenu(false),
)
onUnmounted(() => {
  document.removeEventListener('pointerdown', onOutside)
  clearTimeout(typedTimer)
})
</script>

<template>
  <div ref="root" class="relative" @focusout="onFocusOut">
    <button
      ref="trigger"
      type="button"
      class="flex items-center gap-1.5 rounded-full px-3 py-1 text-xs kbd-ring transition-quick disabled:opacity-50"
      :class="selected.length ? 'pill-selected' : 'surface surface-hover ink-muted'"
      :disabled="disabled"
      aria-haspopup="listbox"
      :aria-expanded="open"
      :aria-controls="open ? listId : undefined"
      :aria-label="`${label}：${summary}`"
      @click="open ? closeMenu(false) : openMenu()"
      @keydown="onTriggerKey"
    >
      <span :class="selected.length ? 'opacity-75' : 'ink-soft'" aria-hidden="true">{{ label }}</span>
      <span class="max-w-40 truncate" aria-hidden="true">{{ summary }}</span>
      <i class="i-carbon-chevron-down h-3 w-3 opacity-70 transition-transform duration-150" :class="open ? 'rotate-180' : ''" aria-hidden="true" />
    </button>

    <div v-if="open" class="popover absolute left-0 top-full mt-1 w-64 flex flex-col" @keydown="onMenuKey">
      <div v-if="searchable" class="relative mb-1">
        <i class="i-carbon-search pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 ink-soft" aria-hidden="true" />
        <input
          ref="search"
          v-model="query"
          type="text"
          role="combobox"
          autocomplete="off"
          spellcheck="false"
          aria-autocomplete="list"
          :aria-expanded="true"
          :aria-controls="listId"
          :aria-activedescendant="activeId"
          :aria-label="`搜索${label}`"
          :placeholder="`搜索${label}`"
          class="field py-1.5 pl-8 text-xs"
        />
      </div>
      <!-- 带搜索时焦点在输入框，列表不进 Tab 序列；不带搜索时列表自己拿焦点 -->
      <ul
        :id="listId"
        ref="list"
        role="listbox"
        aria-multiselectable="true"
        :aria-label="label"
        :aria-activedescendant="searchable ? undefined : activeId"
        :tabindex="searchable ? -1 : 0"
        class="m-0 max-h-72 list-none overflow-y-auto rounded-lg p-0 outline-none"
      >
        <li
          v-for="({ o, parts }, i) in rows"
          :id="optionId(i)"
          :key="o.id"
          role="option"
          :aria-selected="chosen.has(o.id)"
          class="flex cursor-pointer select-none items-center gap-2 rounded-lg px-2 py-1.5 text-xs"
          :class="i === active ? 'surface-strong ring-1 ring-inset ring-blue-500/50' : ''"
          @pointermove="active = i"
          @pointerdown.prevent
          @click="toggle(o.id)"
        >
          <i
            class="h-4 w-4 shrink-0"
            :class="chosen.has(o.id) ? 'i-carbon-checkbox-checked text-blue-500' : 'i-carbon-checkbox ink-soft'"
            aria-hidden="true"
          />
          <span v-if="o.dot" class="h-1.5 w-1.5 shrink-0 rounded-full" :class="o.dot" aria-hidden="true" />
          <span class="min-w-0 flex-1 truncate" :title="o.label">
            <template v-for="(p, k) in parts" :key="k">
              <strong v-if="p.hit" class="font-semibold ink-strong">{{ p.text }}</strong>
              <template v-else>{{ p.text }}</template>
            </template>
          </span>
        </li>
      </ul>
      <p v-if="visible.length === 0" class="m-0 px-2 py-3 text-center text-xs ink-soft" aria-hidden="true">没有匹配的{{ label }}</p>
      <button
        v-if="selected.length"
        type="button"
        class="menu-item mt-1 justify-center text-xs ink-soft"
        @click="clearAll"
      >
        清除选择（{{ selected.length }}）
      </button>
      <p class="sr-only" role="status" aria-live="polite">{{ status }}</p>
    </div>
  </div>
</template>
