<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { formatAnimatedNumber, numericValue } from './motionNumber'

const props = defineProps<{ value: string | number; smallDecimal?: boolean; initialAnimation?: boolean }>()
const displayed = ref(String(props.value))
const parts = computed(() => displayed.value.split('.'))
let frame = 0
let mounted = false
let previous = 0
const reduced = window.matchMedia('(prefers-reduced-motion: reduce)')

function animate(value: string, initial = false) {
  cancelAnimationFrame(frame)
  const target = numericValue(value)
  if (target === null || reduced.matches) {
    displayed.value = value
    previous = target ?? 0
    return
  }
  const from = initial ? 0 : numericValue(displayed.value) ?? previous
  if (from === target) {
    displayed.value = value
    previous = target
    return
  }
  const start = performance.now()
  const duration = Number.parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--motion-count'))
  const tick = (now: number) => {
    const progress = Math.min(1, (now - start) / duration)
    displayed.value = formatAnimatedNumber(from, value, progress)
    if (progress < 1 && !reduced.matches) frame = requestAnimationFrame(tick)
    else displayed.value = value
  }
  frame = requestAnimationFrame(tick)
  previous = target
}

onMounted(() => { mounted = true; animate(String(props.value), props.initialAnimation !== false) })
watch(() => props.value, value => { if (mounted) animate(String(value)) })
onUnmounted(() => cancelAnimationFrame(frame))
</script>

<template><span class="animated-number"><template v-if="smallDecimal && parts.length === 2">{{ parts[0] }}<span class="animated-decimal">.{{ parts[1] }}</span></template><template v-else>{{ displayed }}</template></span></template>
