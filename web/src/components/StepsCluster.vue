<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import type { Item } from '../lib/group'
import { stepsTally } from '../lib/group'
import { fmtDuration, tn } from '../lib/format'

// 照 pfh 的 StepsClusterRow：一串连续的工具调用 / 思考收成一行，默认折着。
// 失败不自动展开，只在头上计数，读的人自己决定要不要点开
const props = defineProps<{ items: Item[]; expanded: boolean }>()
defineEmits<{ toggle: [] }>()
const { t } = useI18n()

const tally = computed(() => stepsTally(props.items))
const duration = computed(() => fmtDuration(tally.value.firstTs, tally.value.lastTs))
const work = computed(() =>
  [
    { icon: 'i-jannchie-terminal', count: tally.value.commands, title: t('session.stepsCluster.commands') },
    { icon: 'i-jannchie-file-text', count: tally.value.read, title: t('session.stepsCluster.readFiles') },
    { icon: 'i-jannchie-edit', count: tally.value.edited, title: t('session.stepsCluster.editedFiles') },
  ].filter((w) => w.count > 0),
)
</script>

<template>
  <button
    type="button"
    class="w-full flex items-center gap-2 overflow-hidden rounded-lg px-2.5 py-1.5 text-left text-xs kbd-ring"
    :class="expanded ? 'surface-strong' : 'surface-hover'"
    :aria-expanded="expanded"
    @click="$emit('toggle')"
  >
    <i class="i-jannchie-list h-3.5 w-3.5 shrink-0 ink-soft" />
    <span class="shrink-0 num ink-soft">{{ tn('session.stepsCluster.actions', items.length) }}</span>
    <span v-if="tally.failed" class="flex shrink-0 items-center gap-1 ink-danger" :title="tn('session.stepsCluster.failures', tally.failed)">
      <i class="i-jannchie-alert-triangle h-3.5 w-3.5" />{{ tally.failed }}
    </span>
    <span class="min-w-0 flex-1" />
    <span v-for="w in work" :key="w.icon" class="flex shrink-0 items-center gap-0.5 num ink-muted" :title="w.title">
      <i class="h-3.5 w-3.5" :class="w.icon" />{{ w.count }}
    </span>
    <span v-if="duration" class="shrink-0 num ink-muted">{{ duration }}</span>
    <i class="i-jannchie-chevron-down fold-chevron" :class="expanded ? 'rotate-180' : ''" />
  </button>
</template>
