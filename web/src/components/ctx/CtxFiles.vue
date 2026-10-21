<script setup lang="ts">
// 文件活动：每个文件一行，读 / 写 / 搜索次数与估算的增删行；展开是它的每一次操作，点了在上下文浏览器里看那次结果。
// 趋势图上选了某次请求时，只算到那次请求为止。
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { aggregateOps, type FileOp, type FileRow } from '../../lib/fileOps'
import { usePref } from '../../lib/ctxView'
import { fmtInt, shortenPath } from '../../lib/format'
import DashCard from '../dash/DashCard.vue'
import Segmented from '../Segmented.vue'
import TimeStamp from '../TimeStamp.vue'

const props = defineProps<{
  ops: FileOp[]
  /** 截止的 seq（选中请求的快照点）；null 是整个会话 */
  upTo: number | null
  /** 截止的请求序号，显示用 */
  upToStep: number | null
  /** op 所在的请求序号 */
  stepOf: (seq: number) => number
}>()
const emit = defineEmits<{ jump: [op: FileOp]; clearUpTo: [] }>()
const { t } = useI18n()

type Filter = 'read' | 'write' | 'search' | 'image'
const filter = ref<Filter | null>(null)
const query = ref('')
const sort = usePref<'count' | 'latest' | 'path'>('files.sort', 'count')
const open = ref(new Set<string>())
const limit = ref(80)

const rows = computed(() => aggregateOps(props.ops, props.upTo ?? undefined))

const totals = computed(() => {
  const out = { read: 0, write: 0, search: 0, image: 0, added: 0, removed: 0 }
  for (const r of rows.value) {
    if (r.reads) out.read++
    if (r.writes) out.write++
    if (r.searches) out.search++
    if (r.form === 'image') out.image++
    out.added += r.added
    out.removed += r.removed
  }
  return out
})

const shown = computed(() => {
  const q = query.value.trim().toLowerCase()
  const f = filter.value
  const list = rows.value.filter(
    (r) =>
      (!q || r.path.toLowerCase().includes(q)) &&
      (!f || (f === 'read' ? r.reads : f === 'write' ? r.writes : f === 'search' ? r.searches : r.form === 'image')),
  )
  const total = (r: FileRow) => r.reads + r.writes + r.searches
  return list.sort((a, b) =>
    sort.value === 'path' ? a.path.localeCompare(b.path) : sort.value === 'latest' ? b.lastSeq - a.lastSeq : total(b) - total(a) || b.lastSeq - a.lastSeq,
  )
})

const FILTERS = computed(() => [
  { key: 'read' as const, label: t('ctx.files.read'), icon: 'i-jannchie-eye', n: totals.value.read },
  { key: 'write' as const, label: t('ctx.files.written'), icon: 'i-jannchie-clock-edit', n: totals.value.write },
  { key: 'search' as const, label: t('ctx.files.searched'), icon: 'i-jannchie-search', n: totals.value.search },
  { key: 'image' as const, label: t('ctx.files.images'), icon: 'i-jannchie-file-image', n: totals.value.image },
])
const SORTS = computed(() => [
  { value: 'count' as const, label: t('ctx.files.sortCount') },
  { value: 'latest' as const, label: t('ctx.files.sortLatest') },
  { value: 'path' as const, label: t('ctx.files.sortPath') },
])

function toggle(path: string) {
  const s = new Set(open.value)
  if (s.has(path)) s.delete(path)
  else s.add(path)
  open.value = s
}

const FORM_ICON: Record<FileRow['form'], string> = {
  file: 'i-jannchie-file-text',
  image: 'i-jannchie-file-image',
  dir: 'i-jannchie-folder',
  pattern: 'i-jannchie-search',
}
const KIND_ICON: Record<FileOp['kind'], string> = {
  read: 'i-jannchie-eye',
  write: 'i-jannchie-clock-edit',
  search: 'i-jannchie-search',
}

function dirOf(r: FileRow): string {
  const i = r.path.replace(/\/$/, '').lastIndexOf('/')
  return i > 0 ? shortenPath(r.path.slice(0, i), 56) : ''
}

function opDetail(o: FileOp): string {
  const parts: string[] = []
  if (o.window) parts.push(o.window.est ? t('ctx.files.windowEst', { count: o.window.count }) : t('ctx.files.window', { start: o.window.start, count: o.window.count }))
  if (o.hits) parts.push(t('ctx.files.hits', { n: o.hits }, o.hits))
  if (o.detail) parts.push(o.detail)
  if (o.program) parts.push(t('ctx.files.program', { desc: o.program }))
  return parts.join(' · ')
}
</script>

<template>
  <DashCard icon="i-jannchie-folder-code" :title="t('ctx.files.title')" :caption="t('ctx.files.caption')">
    <template #controls>
      <Segmented v-model="sort" :label="t('ctx.files.sortCount')" :options="SORTS" />
    </template>
    <div class="flex flex-col gap-2">
      <div class="flex flex-wrap items-center gap-2">
        <button
          v-for="f in FILTERS"
          :key="f.key"
          type="button"
          class="segment border-0"
          :class="filter === f.key ? 'pill-selected' : 'surface ink-muted'"
          :aria-pressed="filter === f.key"
          @click="filter = filter === f.key ? null : f.key"
        >
          <i class="h-3.5 w-3.5" :class="f.icon" aria-hidden="true" />{{ f.label }} <span class="num">{{ fmtInt(f.n) }}</span>
        </button>
        <span class="text-xs num">
          <span class="ink-success">+{{ fmtInt(totals.added) }}</span> <span class="ink-danger">−{{ fmtInt(totals.removed) }}</span>
        </span>
        <span class="flex-1" />
        <button
          v-if="upToStep != null"
          type="button"
          class="segment border-0 pill-selected"
          :title="t('ctx.files.all')"
          @click="emit('clearUpTo')"
        >
          {{ t('ctx.files.upTo', { n: upToStep + 1 }) }} <i class="i-jannchie-x h-3 w-3" aria-hidden="true" />
        </button>
        <input v-model="query" type="search" class="field max-w-56 py-1 text-xs" :placeholder="t('ctx.files.search')" :aria-label="t('ctx.files.search')" />
      </div>

      <p v-if="!shown.length" class="m-0 py-6 text-center text-xs ink-soft">{{ t('ctx.files.none') }}</p>
      <ul class="m-0 flex flex-col gap-px p-0">
        <li v-for="r in shown.slice(0, limit)" :key="r.path" class="list-none">
          <button
            type="button"
            class="w-full flex items-center gap-2 rounded-lg border-0 bg-transparent px-2 py-1 text-left text-xs surface-hover kbd-ring"
            :class="open.has(r.path) ? 'surface-strong' : ''"
            :aria-expanded="open.has(r.path)"
            :title="r.path"
            @click="toggle(r.path)"
          >
            <i class="h-3.5 w-3.5 shrink-0 ink-soft" :class="FORM_ICON[r.form]" aria-hidden="true" />
            <span class="min-w-0 flex-1 truncate">
              <span class="font-mono ink-strong">{{ r.name }}</span>
              <span class="ml-2 font-mono ink-soft">{{ dirOf(r) }}</span>
            </span>
            <span v-if="r.errors" class="shrink-0 ink-danger num">{{ r.errors }}×</span>
            <span class="w-10 shrink-0 text-right num" :class="r.reads ? 'ink-muted' : 'opacity-0'" :title="t('ctx.files.read')">{{ r.reads }}R</span>
            <span class="w-10 shrink-0 text-right num" :class="r.writes ? 'ink-muted' : 'opacity-0'" :title="t('ctx.files.written')">{{ r.writes }}W</span>
            <span class="w-10 shrink-0 text-right num" :class="r.searches ? 'ink-muted' : 'opacity-0'" :title="t('ctx.files.searched')">{{ r.searches }}S</span>
            <span class="w-20 shrink-0 text-right num">
              <template v-if="r.added || r.removed"><span class="ink-success">+{{ fmtInt(r.added) }}</span> <span class="ink-danger">−{{ fmtInt(r.removed) }}</span></template>
            </span>
            <TimeStamp v-if="r.lastTs" :ts="r.lastTs" class="w-16 shrink-0 text-right ink-soft max-sm:hidden" />
          </button>
          <ul v-if="open.has(r.path)" class="m-0 mb-1 ml-6 flex flex-col gap-px border-l border-line p-0 pl-2">
            <li v-for="o in r.ops" :key="`${o.seq}:${o.path}:${o.kind}`" class="list-none">
              <button
                type="button"
                class="w-full flex items-center gap-2 rounded-md border-0 bg-transparent px-2 py-0.5 text-left text-xs surface-hover kbd-ring"
                :title="t('ctx.files.jump')"
                @click="emit('jump', o)"
              >
                <i class="h-3 w-3 shrink-0" :class="[KIND_ICON[o.kind], o.err ? 'ink-danger' : 'ink-soft']" aria-hidden="true" />
                <span class="shrink-0 font-mono ink-muted">{{ o.tool }}</span>
                <span class="min-w-0 flex-1 truncate ink-soft">{{ opDetail(o) }}</span>
                <span v-if="o.added || o.removed" class="shrink-0 num"><span class="ink-success">+{{ o.added }}</span> <span class="ink-danger">−{{ o.removed }}</span></span>
                <span class="w-10 shrink-0 text-right num ink-soft">#{{ stepOf(o.parent ?? o.seq) + 1 }}</span>
              </button>
            </li>
          </ul>
        </li>
      </ul>
      <button v-if="shown.length > limit" type="button" class="self-start rounded-md border-0 px-2 py-1 text-xs surface surface-hover kbd-ring" @click="limit += 100">
        {{ t('ctx.events.more', { n: fmtInt(Math.min(100, shown.length - limit)) }) }}
      </button>
    </div>
  </DashCard>
</template>
