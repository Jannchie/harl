<script setup lang="ts">
// 侧栏底部的语言切换：图标按钮 + 向上弹出的单选菜单（WAI-ARIA menu + menuitemradio）。
// 打开时焦点落在当前语言上，↑↓ / Home / End 移动，Enter / Space 选中，Esc 或点外面关闭并把焦点还给按钮。
import { nextTick, onUnmounted, ref, useId } from 'vue'
import { useI18n } from 'vue-i18n'
import { LOCALES, locale, setLocale, type Locale } from '../i18n'

const { t } = useI18n()
const menuId = `${useId()}-menu`
const open = ref(false)
const root = ref<HTMLElement | null>(null)
const button = ref<HTMLButtonElement | null>(null)
const items = ref<HTMLElement[]>([])

function onDocDown(e: PointerEvent) {
  if (!root.value?.contains(e.target as Node)) close(false)
}

async function show() {
  open.value = true
  document.addEventListener('pointerdown', onDocDown)
  await nextTick()
  const i = LOCALES.findIndex((l) => l.id === locale.value)
  items.value[Math.max(0, i)]?.focus()
}

function close(refocus = true) {
  if (!open.value) return
  open.value = false
  document.removeEventListener('pointerdown', onDocDown)
  if (refocus) button.value?.focus()
}
onUnmounted(() => document.removeEventListener('pointerdown', onDocDown))

function pick(id: Locale) {
  setLocale(id)
  close()
}

function onMenuKey(e: KeyboardEvent) {
  const i = items.value.indexOf(document.activeElement as HTMLElement)
  const n = items.value.length
  const go = (j: number) => {
    e.preventDefault()
    items.value[(j + n) % n]?.focus()
  }
  if (e.key === 'ArrowDown') go(i + 1)
  else if (e.key === 'ArrowUp') go(i - 1)
  else if (e.key === 'Home') go(0)
  else if (e.key === 'End') go(n - 1)
  else if (e.key === 'Escape') {
    e.preventDefault()
    close()
  } else if (e.key === 'Tab') close(false)
}
</script>

<template>
  <div ref="root" class="relative">
    <button
      ref="button"
      type="button"
      class="icon-button-sm"
      :title="t('common.language')"
      :aria-label="t('common.language')"
      aria-haspopup="menu"
      :aria-expanded="open"
      :aria-controls="open ? menuId : undefined"
      @click="open ? close() : show()"
      @keydown.down.prevent="show()"
      @keydown.up.prevent="show()"
    >
      <i class="i-jannchie-languages h-3.5 w-3.5" aria-hidden="true" />
    </button>
    <div
      v-if="open"
      :id="menuId"
      role="menu"
      :aria-label="t('common.language')"
      class="popover absolute bottom-full right-0 mb-1 min-w-36"
      @keydown="onMenuKey"
    >
      <button
        v-for="l in LOCALES"
        :key="l.id"
        ref="items"
        type="button"
        role="menuitemradio"
        :aria-checked="l.id === locale"
        :lang="l.id"
        tabindex="-1"
        class="menu-item"
        @click="pick(l.id)"
      >
        <span class="min-w-0 flex-1">{{ l.label }}</span>
        <i v-if="l.id === locale" class="i-jannchie-check h-3.5 w-3.5 ink-accent" aria-hidden="true" />
      </button>
    </div>
  </div>
</template>
