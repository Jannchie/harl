<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { RouterLink } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { fmtBytes, fmtInt, fmtPct, tn } from '../lib/format'
import { projectOf } from '../lib/project'
import { providerDot, providerName } from '../lib/providers'
import { loadSessions, machineName, sessions, status } from '../lib/sessions'
import TimeStamp from '../components/TimeStamp.vue'

const { t } = useI18n()

onMounted(() => loadSessions())

/** 存储的几个数：按「有多少、占多少、压到多少」的顺序读 */
const figures = computed(() => {
  const s = status.value
  if (!s) return []
  return [
    { key: 'files', label: t('session.sessionHome.sessionFiles'), value: fmtInt(s.sources), note: tn('session.sessionHome.snapshots', s.records) },
    { key: 'raw', label: t('session.sessionHome.rawSize'), value: fmtBytes(s.logical_bytes), note: t('session.sessionHome.rawSizeNote') },
    { key: 'packed', label: t('session.sessionHome.packSize'), value: fmtBytes(s.pack_bytes), note: tn('session.sessionHome.packs', s.packs) },
    {
      key: 'ratio',
      label: t('session.sessionHome.ratio'),
      value: s.logical_bytes ? fmtPct(s.pack_bytes / s.logical_bytes) : '–',
      note: tn('session.sessionHome.chunks', s.chunks),
    },
  ]
})

const byProvider = computed(() => {
  const m = new Map<string, number>()
  for (const s of sessions.value ?? []) m.set(s.provider, (m.get(s.provider) ?? 0) + 1)
  return [...m.entries()].sort((a, b) => b[1] - a[1])
})

const recent = computed(() =>
  [...(sessions.value ?? [])]
    .filter((s) => !s.parent_session)
    .sort((a, b) => (a.mtime < b.mtime ? 1 : -1))
    .slice(0, 8),
)
</script>

<template>
  <div class="mx-auto w-full max-w-chat px-4 pb-16 pt-14 md:px-6 md:pt-10">
    <h1 class="m-0 text-base font-semibold">{{ t('common.nav.archive') }}</h1>
    <i18n-t keypath="session.sessionHome.intro" tag="p" class="m-0 mt-1 text-sm ink-soft" scope="global">
      <template #cmd><code class="text-xs">harl capture</code></template>
    </i18n-t>

    <div class="mt-6 grid grid-cols-2 gap-3 md:grid-cols-4">
      <div v-for="f in figures" :key="f.key" class="rounded-2xl px-4 py-3 surface">
        <div class="text-xs ink-soft">{{ f.label }}</div>
        <div class="mt-1 text-xl font-semibold num">{{ f.value }}</div>
        <div class="mt-0.5 text-xs ink-soft">{{ f.note }}</div>
      </div>
    </div>

    <div v-if="status" class="mt-3 flex flex-wrap gap-x-4 gap-y-1 px-1 text-xs ink-soft">
      <span class="inline-flex items-center gap-1" :title="status.data_dir"><i class="i-jannchie-folder h-3.5 w-3.5" />{{ status.data_dir }}</span>
      <span class="inline-flex items-center gap-1" :title="t('session.sessionHome.machine', { id: status.machine })"><i class="i-jannchie-laptop h-3.5 w-3.5" aria-hidden="true" />{{ machineName(status.machine) }}</span>
      <span v-for="[p, n] in byProvider" :key="p" class="inline-flex items-center gap-1.5">
        <span class="h-1.5 w-1.5 rounded-full" :class="providerDot(p)" />{{ providerName(p) }} <span class="num">{{ fmtInt(n) }}</span>
      </span>
    </div>

    <h2 v-if="recent.length" class="m-0 mt-10 text-sm font-medium ink-strong">{{ t('session.sessionHome.recent') }}</h2>
    <div class="mt-2 flex flex-col gap-0.5">
      <RouterLink
        v-for="s in recent"
        :key="s.id"
        :to="{ name: 'session', params: { id: s.id } }"
        class="flex items-center gap-3 rounded-lg px-3 py-2 text-sm surface-hover kbd-ring"
      >
        <span class="h-1.5 w-1.5 shrink-0 rounded-full" :class="providerDot(s.provider)" />
        <span class="min-w-0 flex-1 truncate" :class="s.title ? '' : 'ink-soft'">{{ s.title ?? s.session }}</span>
        <span class="shrink-0 truncate text-xs ink-soft max-sm:hidden">{{ projectOf(s).label }}</span>
        <TimeStamp :ts="s.mtime" mode="rel" class="w-20 shrink-0 text-right text-xs ink-soft" />
      </RouterLink>
    </div>
  </div>
</template>
