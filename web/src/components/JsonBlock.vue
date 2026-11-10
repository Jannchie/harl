<script setup lang="ts">
import { computed, inject, onUnmounted, ref, shallowRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { elidedIn } from '../api'
import { fmtBytes, fmtInt, prettyJson } from '../lib/format'
import { restoreElided, SNAPSHOT_ID } from '../lib/elided'
import { describe } from '../lib/sessions'

const props = withDefaults(
  defineProps<{
    /** 已经是字符串就原样显示，否则 pretty JSON */
    value: unknown
    /** 超过这个字符数先截断 */
    limit?: number
  }>(),
  { limit: 20_000 },
)

const { t } = useI18n()
const snapshot = inject(SNAPSHOT_ID, null)

/** slim 事件里省略掉的 base64 取回来之后的全文 */
const restored = shallowRef<string | null>(null)
const restoring = ref(false)
const restoreError = ref<string | null>(null)
let ac: AbortController | null = null
watch(
  () => props.value,
  () => {
    ac?.abort()
    restored.value = null
    restoring.value = false
    restoreError.value = null
  },
)
onUnmounted(() => ac?.abort())

const full = computed(() => restored.value ?? (typeof props.value === 'string' ? props.value : prettyJson(props.value)))
const elided = computed(() => (snapshot && restored.value == null ? elidedIn(full.value) : []))
const elidedBytes = computed(() => elided.value.reduce((a, e) => a + e.len, 0))
const expanded = ref(false)
const truncated = computed(() => full.value.length > props.limit && !expanded.value)
const shown = computed(() => (truncated.value ? full.value.slice(0, props.limit) : full.value))

async function restore() {
  if (!snapshot || restoring.value) return
  ac?.abort()
  ac = new AbortController()
  const signal = ac.signal
  restoring.value = true
  restoreError.value = null
  try {
    const text = await restoreElided(snapshot.value, full.value, signal)
    if (!signal.aborted) restored.value = text
  } catch (e) {
    if (!signal.aborted) restoreError.value = describe(e)
  } finally {
    if (!signal.aborted) restoring.value = false
  }
}
</script>

<template>
  <div>
    <pre class="payload-block max-h-[60vh] leading-relaxed">{{ shown }}</pre>
    <div v-if="truncated || elided.length" class="mt-1 flex flex-wrap items-center gap-1">
      <button v-if="truncated" type="button" class="rounded-md px-2 py-0.5 text-xs ink-soft surface-hover kbd-ring" @click="expanded = true">
        {{ t('session.jsonBlock.showAll', { n: fmtInt(full.length) }) }}
      </button>
      <button
        v-if="elided.length"
        type="button"
        class="rounded-md px-2 py-0.5 text-xs ink-soft surface-hover kbd-ring"
        :disabled="restoring"
        @click="restore"
      >
        {{ restoring ? t('session.jsonBlock.loadingElided') : t('session.jsonBlock.loadElided', { n: elided.length, size: fmtBytes(elidedBytes) }) }}
      </button>
      <span v-if="restoreError" class="text-xs ink-danger">{{ t('session.jsonBlock.elidedFailed', { error: restoreError }) }}</span>
    </div>
  </div>
</template>
