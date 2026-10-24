<script setup lang="ts">
import { computed } from 'vue'
const props = defineProps<{ values: (number | null)[] }>()
const plotted = computed(() => {
  const max = Math.max(1, ...props.values.filter((value): value is number => value !== null))
  return props.values.flatMap((value, index) => value === null ? [] : [{
    x: (index / Math.max(1, props.values.length - 1)) * 188,
    y: 42 - (value / max) * 34,
  }])
})
const points = computed(() => plotted.value.map(point => `${point.x},${point.y}`).join(' '))
const area = computed(() => plotted.value.length ? `${plotted.value[0]!.x},44 ${points.value} ${plotted.value.at(-1)!.x},44` : '')
</script>
<template>
  <svg class="sparkline" viewBox="0 0 188 44" preserveAspectRatio="none" aria-hidden="true">
    <polygon v-if="plotted.length" :points="area" fill="var(--primary)" fill-opacity="0.17" />
    <polyline v-if="plotted.length" :points="points" fill="none" stroke="var(--primary)" stroke-width="1.9" stroke-linejoin="round" stroke-linecap="round" />
  </svg>
</template>
<style scoped>
.sparkline { display: block; width: 100%; height: 44px; overflow: visible; }
</style>
