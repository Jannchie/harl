<script setup lang="ts">
import { nextTick, ref, watch } from 'vue'
import { RouterView, useRoute } from 'vue-router'
import AppSidebar from './components/AppSidebar.vue'
import { NARROW, useMediaQuery } from './lib/media'

const route = useRoute()
/** 窄屏下侧栏是抽屉；换页就收起 */
const asideOpen = ref(false)
watch(
  () => route.fullPath,
  () => (asideOpen.value = false),
)

// 窄屏（< md）时侧栏是抽屉：收起时整个 inert（不然移出屏幕的链接还在 Tab 序列里），
// 打开时它是模态对话框，主区 inert，Esc 关、焦点回到菜单按钮
const narrow = useMediaQuery(NARROW)

const drawer = ref<HTMLElement | null>(null)
const menuButton = ref<HTMLButtonElement | null>(null)

watch(asideOpen, async (open) => {
  if (!narrow.value) return
  await nextTick()
  if (open) drawer.value?.querySelector<HTMLElement>('input, a, button')?.focus()
  else if (drawer.value?.contains(document.activeElement) || document.activeElement === document.body) menuButton.value?.focus()
})
// 变宽了抽屉就不存在了
watch(narrow, (n) => !n && (asideOpen.value = false))

function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape' && asideOpen.value && narrow.value) {
    e.preventDefault()
    asideOpen.value = false
  }
}
</script>

<template>
  <div class="h-dvh flex overflow-hidden page-ground text-neutral-900 dark:text-neutral-100" @keydown="onKey">
    <a
      href="#main"
      class="fixed left-2 top-2 z-60 rounded-lg px-3 py-1.5 text-sm page-ground shadow-lg kbd-ring -translate-y-20 focus:translate-y-0"
    >
      跳到主要内容
    </a>

    <Transition name="fade">
      <div v-if="asideOpen" class="fixed inset-0 z-40 bg-black/40 md:hidden" aria-hidden="true" @click="asideOpen = false" />
    </Transition>

    <!-- 宽屏是一列，窄屏是盖在上面的抽屉：同一个实例，滚动位置和展开状态不丢 -->
    <div
      id="app-drawer"
      ref="drawer"
      class="shrink-0 flex max-md:fixed max-md:inset-y-0 max-md:left-0 max-md:z-50 max-md:bg-neutral-50 max-md:shadow-xl max-md:transition-transform max-md:duration-150 max-md:dark:bg-neutral-950"
      :class="asideOpen ? '' : 'max-md:-translate-x-full'"
      :role="narrow && asideOpen ? 'dialog' : undefined"
      :aria-modal="narrow && asideOpen ? 'true' : undefined"
      :aria-label="narrow ? '侧栏' : undefined"
      :inert="(narrow && !asideOpen) || undefined"
    >
      <AppSidebar />
    </div>

    <main id="main" tabindex="-1" class="relative min-w-0 flex-1 overflow-y-auto outline-none" data-scroll-root :inert="(narrow && asideOpen) || undefined">
      <button
        ref="menuButton"
        type="button"
        class="icon-button fixed left-2 top-2 z-30 md:hidden page-ground"
        aria-label="打开侧栏"
        aria-controls="app-drawer"
        :aria-expanded="asideOpen"
        @click="asideOpen = true"
      >
        <i class="i-jannchie-menu h-4 w-4" aria-hidden="true" />
      </button>
      <RouterView />
    </main>
  </div>
</template>
