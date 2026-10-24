<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue'

withDefaults(defineProps<{
  title: string
  confirmLabel?: string
  danger?: boolean
  busy?: boolean
}>(), {
  confirmLabel: '确认',
  danger: false,
  busy: false,
})
const emit = defineEmits<{ cancel: []; confirm: [] }>()
const panel = ref<HTMLElement | null>(null)

function onKey(event: KeyboardEvent) {
  if (event.key !== 'Escape') return
  event.preventDefault()
  emit('cancel')
}
onMounted(() => {
  document.addEventListener('keydown', onKey)
  panel.value?.focus()
})
onUnmounted(() => document.removeEventListener('keydown', onKey))
</script>

<template>
  <div class="confirm-mask" @click.self="emit('cancel')">
    <div ref="panel" class="confirm-dialog" role="dialog" aria-modal="true" aria-labelledby="confirm-dialog-title" tabindex="-1">
      <h2 id="confirm-dialog-title">{{ title }}</h2>
      <div class="confirm-body"><slot /></div>
      <div class="confirm-actions">
        <button type="button" class="confirm-cancel" :disabled="busy" @click="emit('cancel')">取消</button>
        <button type="button" class="confirm-ok" :class="{ danger }" :disabled="busy" @click="emit('confirm')">{{ confirmLabel }}</button>
      </div>
    </div>
  </div>
</template>
