<script setup lang="ts">
// 侧栏底部的版本信息：平时只低调地显示当前版本；有新版本时换成提示。能自更新的点一下就下载、换上、重启，页面等新版本起来后自动刷新；
// 不能自更新（开发构建、没有本平台的包……）的只给 release 链接。
// 后端自己换了版本（自动更新重启过）时也出现，提示页面过时，点一下刷新。
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { api, type VersionInfo } from '../api'
import { describe } from '../lib/sessions'
import AtomSpinner from './AtomSpinner.vue'

/** 后端自己缓存 6 小时，这里只是让开着不关的页面也能看到 */
const RECHECK_MS = 30 * 60_000
/** 重启期间轮询的间隔和上限 */
const POLL_MS = 1500
const POLL_LIMIT_MS = 120_000

const { t } = useI18n()
const info = ref<VersionInfo | null>(null)
/**
 * idle → updating（下载、替换）→ restarting（等新版本起来，起来了就刷新页面）；任一步失败 → error，可以重试。
 * stale：后端已经换成了别的版本（自动更新后重启过），这个页面还是旧的，点一下刷新
 */
type Phase =
  | { kind: 'idle' }
  | { kind: 'updating' }
  | { kind: 'restarting' }
  | { kind: 'error'; message: string }
  | { kind: 'stale'; version: string }
const phase = ref<Phase>({ kind: 'idle' })
const busy = computed(() => phase.value.kind === 'updating' || phase.value.kind === 'restarting')
const errorMessage = computed(() => (phase.value.kind === 'error' ? phase.value.message : ''))
/** 查到的最新版就是当前版本时，它的 release 页就是当前版本的 */
const currentUrl = computed(() => (info.value?.latest?.version === info.value?.current ? info.value?.latest?.url ?? null : null))
const currentTitle = computed(() => {
  const v = info.value
  if (!v) return undefined
  if (!v.latest) return t('common.update.versionUnknown')
  return v.latest.version === v.current ? t('common.update.upToDate') : undefined
})

/** 页面打开后第一次查到的后端版本；之后 current 变了，说明后端换过版本、页面过时了 */
let seen: string | null = null

async function check() {
  if (busy.value) return
  let v: VersionInfo
  try {
    v = await api.version()
  } catch {
    // 查不到就不显示，不打扰
    return
  }
  info.value = v
  seen ??= v.current
  if (v.current !== seen) {
    // 没人在看就直接换上新页面；正看着的话不打断，提示一下让人自己点
    if (document.hidden) location.reload()
    else phase.value = { kind: 'stale', version: v.current }
    return
  }
  // 更新到一半页面被刷新了（或者是后台自动更新正在装）：接着显示进度，别回到 idle
  if (v.phase === 'installing' || v.phase === 'installed') void follow(v.phase)
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))

/** 轮询 version 直到 done 成立，然后刷新页面；重启的那几秒请求会失败，接着等 */
async function waitFor(done: (v: VersionInfo) => boolean) {
  const deadline = Date.now() + POLL_LIMIT_MS
  while (Date.now() < deadline) {
    await sleep(POLL_MS)
    const v = await api.version().catch(() => null)
    if (v && done(v)) {
      location.reload()
      return
    }
  }
  throw new Error(t('common.update.timeout'))
}

async function update() {
  if (busy.value) return
  phase.value = { kind: 'updating' }
  try {
    const { version } = await api.update()
    phase.value = { kind: 'restarting' }
    await waitFor((v) => v.current === version)
  } catch (e) {
    phase.value = { kind: 'error', message: describe(e) }
  }
}

/** 接上一次已经在进行的更新：不知道目标版本，等 current 变了就算起来了 */
async function follow(p: 'installing' | 'installed') {
  const from = seen
  phase.value = { kind: p === 'installing' ? 'updating' : 'restarting' }
  try {
    await waitFor((v) => {
      if (v.current !== from) return true
      if (v.phase === 'installed') phase.value = { kind: 'restarting' }
      // 还是老进程却回到了 idle：这次安装失败了
      else if (v.phase !== 'installing') throw new Error(t('common.update.interrupted'))
      return false
    })
  } catch (e) {
    phase.value = { kind: 'error', message: describe(e) }
  }
}

const reload = () => location.reload()

let timer: ReturnType<typeof setInterval> | undefined
onMounted(() => {
  check()
  timer = setInterval(check, RECHECK_MS)
})
onUnmounted(() => clearInterval(timer))
</script>

<template>
  <div
    v-if="info && (info.update_available || phase.kind !== 'idle')"
    class="mb-1 flex items-center gap-2 rounded-lg px-2 py-1.5 text-xs surface"
    role="status"
    aria-live="polite"
  >
    <i class="i-jannchie-upgrade h-3.5 w-3.5 shrink-0 ink-accent" aria-hidden="true" />
    <span class="min-w-0 flex-1 truncate" :title="errorMessage || info.latest?.notes || undefined">
      <template v-if="phase.kind === 'updating'">
        {{ info.latest ? t('common.update.downloading', { version: info.latest.version }) : t('common.update.installing') }}
      </template>
      <template v-else-if="phase.kind === 'restarting'">{{ t('common.update.restarting') }}</template>
      <i18n-t v-else-if="phase.kind === 'error'" keypath="common.update.failedWith" scope="global">
        <template #failed><span class="ink-danger">{{ t('common.update.failed') }}</span></template>
        <template #error>{{ errorMessage }}</template>
      </i18n-t>
      <i18n-t v-else-if="phase.kind === 'stale'" keypath="common.update.stale" scope="global">
        <template #version><span class="num ink-strong">{{ phase.version }}</span></template>
      </i18n-t>
      <template v-else-if="info.latest">
        <i18n-t keypath="common.update.newVersion" scope="global">
          <template #version><span class="num ink-strong">{{ info.latest.version }}</span></template>
        </i18n-t>
        {{ ' ' }}
        <i18n-t keypath="common.update.current" scope="global" tag="span" class="ink-soft">
          <template #version><span class="num">{{ info.current }}</span></template>
        </i18n-t>
      </template>
    </span>
    <AtomSpinner v-if="busy" class="h-3.5 w-3.5" />
    <button
      v-else-if="phase.kind === 'stale'"
      type="button"
      class="shrink-0 rounded-md px-2 py-0.5 pill-selected kbd-ring"
      @click="reload"
    >
      {{ t('common.update.reload') }}
    </button>
    <button
      v-else-if="info.can_update"
      type="button"
      class="shrink-0 rounded-md px-2 py-0.5 pill-selected kbd-ring"
      @click="update"
    >
      {{ phase.kind === 'error' ? t('common.retry') : t('common.update.update') }}
    </button>
    <a
      v-else-if="info.latest"
      :href="info.latest.url"
      target="_blank"
      rel="noreferrer"
      class="shrink-0 ink-accent kbd-ring"
      :title="info.reason ?? undefined"
    >
      {{ t('common.update.view') }}
    </a>
  </div>
  <!-- 平时只低调地标出当前版本；查到的最新版就是它时链到它的 release -->
  <component
    :is="currentUrl ? 'a' : 'span'"
    v-else-if="info"
    :href="currentUrl ?? undefined"
    :target="currentUrl ? '_blank' : undefined"
    :rel="currentUrl ? 'noreferrer' : undefined"
    class="mb-1 self-start rounded px-2 text-xs ink-soft kbd-ring"
    :class="currentUrl ? 'transition-colors hover:ink-strong' : ''"
    :title="currentTitle"
  >
    harl <span class="num">v{{ info.current }}</span>
  </component>
</template>
