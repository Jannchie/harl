<script setup lang="ts">
import type { Provider } from '../api'
import { providerDot } from '../lib/providers'

defineProps<{ provider: Provider }>()
</script>

<template>
  <span class="inline-flex items-center gap-1.5 whitespace-nowrap">
    <span class="h-1.5 w-1.5 rounded-full" :class="providerDot(provider)" />
    {{ provider }}
  </span>
</template>
