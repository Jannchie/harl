<script setup lang="ts">
import { ref, watch } from 'vue'
import { RouterView, useRoute } from 'vue-router'
import AppSidebar from './components/AppSidebar.vue'

const route = useRoute()
/** 窄屏下侧栏是抽屉；换页就收起 */
const asideOpen = ref(false)
watch(
  () => route.fullPath,
  () => (asideOpen.value = false),
)
</script>

<template>
  <div class="h-dvh flex overflow-hidden page-ground text-neutral-900 dark:text-neutral-100">
    <Transition name="fade">
      <div v-if="asideOpen" class="fixed inset-0 z-40 bg-black/40 md:hidden" @click="asideOpen = false" />
    </Transition>

    <!-- 宽屏是一列，窄屏是盖在上面的抽屉：同一个实例，滚动位置和展开状态不丢 -->
    <div
      class="shrink-0 flex max-md:fixed max-md:inset-y-0 max-md:left-0 max-md:z-50 max-md:bg-neutral-50 max-md:shadow-xl max-md:transition-transform max-md:duration-150 max-md:dark:bg-neutral-950"
      :class="asideOpen ? '' : 'max-md:-translate-x-full'"
    >
      <AppSidebar />
    </div>

    <main class="relative min-w-0 flex-1 overflow-y-auto" data-scroll-root>
      <button
        type="button"
        class="icon-button fixed left-2 top-2 z-30 md:hidden page-ground"
        aria-label="打开侧栏"
        @click="asideOpen = true"
      >
        <i class="i-carbon-menu h-4 w-4" />
      </button>
      <RouterView />
    </main>
  </div>
</template>
