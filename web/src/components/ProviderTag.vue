<script setup lang="ts">
import type { Provider } from '../api'

defineProps<{ provider: Provider }>()
</script>

<template>
  <span class="badge" :class="`badge-${provider}`">{{ provider }}</span>
</template>
