<script setup lang="ts">
// 侧栏底部的新版本提示：有新版本才出现。能自更新的点一下就下载、换上、重启，页面等新版本起来后自动刷新；
// 不能自更新（开发构建、没有本平台的包……）的只给 release 链接。
import { onMounted, onUnmounted, ref } from 'vue'
import { api, type VersionInfo } from '../api'
import AtomSpinner from './AtomSpinner.vue'

/** 后端自己缓存 6 小时，这里只是让开着不关的页面也能看到 */
const RECHECK_MS = 30 * 60_000
/** 重启期间轮询的间隔和上限 */
const POLL_MS = 1500
const POLL_LIMIT_MS = 120_000

const info = ref<VersionInfo | null>(null)
const phase = ref<'idle' | 'updating' | 'restarting' | 'error'>('idle')
const error = ref('')

async function check() {
  try {
    info.value = await api.version()
  } catch {
    // 查不到就不显示，不打扰
  }
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))

async function update() {
  phase.value = 'updating'
  error.value = ''
  try {
    const { version } = await api.update()
    phase.value = 'restarting'
    const deadline = Date.now() + POLL_LIMIT_MS
    while (Date.now() < deadline) {
      await sleep(POLL_MS)
      // 重启的那几秒请求会失败，接着等
      const v = await api.version().catch(() => null)
      if (v?.current === version) {
        location.reload()
        return
      }
    }
    throw new Error('新版本没有按时起来，看一下 harl 的日志')
  } catch (e) {
    phase.value = 'error'
    error.value = (e as Error).message
  }
}

let timer: ReturnType<typeof setInterval> | undefined
onMounted(() => {
  check()
  timer = setInterval(check, RECHECK_MS)
})
onUnmounted(() => clearInterval(timer))
</script>

<template>
  <div
    v-if="info?.update_available && info.latest"
    class="mb-1 flex items-center gap-2 rounded-lg px-2 py-1.5 text-xs surface"
    role="status"
    aria-live="polite"
  >
    <i class="i-jannchie-upgrade h-3.5 w-3.5 shrink-0 ink-accent" aria-hidden="true" />
    <span class="min-w-0 flex-1 truncate" :title="error || info.latest.notes || undefined">
      <template v-if="phase === 'updating'">正在下载 {{ info.latest.version }}…</template>
      <template v-else-if="phase === 'restarting'">正在重启…</template>
      <template v-else-if="phase === 'error'"><span class="ink-danger">更新失败</span>：{{ error }}</template>
      <template v-else>
        新版本 <span class="num ink-strong">{{ info.latest.version }}</span>
        <span class="ink-soft">（当前 <span class="num">{{ info.current }}</span>）</span>
      </template>
    </span>
    <AtomSpinner v-if="phase === 'updating' || phase === 'restarting'" class="h-3.5 w-3.5" />
    <button
      v-else-if="info.can_update"
      type="button"
      class="shrink-0 rounded-md px-2 py-0.5 pill-selected kbd-ring"
      @click="update"
    >
      {{ phase === 'error' ? '重试' : '更新' }}
    </button>
    <a
      v-else
      :href="info.latest.url"
      target="_blank"
      rel="noreferrer"
      class="shrink-0 ink-accent kbd-ring"
      :title="info.reason ?? undefined"
    >
      查看
    </a>
  </div>
</template>
