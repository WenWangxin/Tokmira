<script setup lang="ts">
defineProps<{
  checked: boolean
  disabled?: boolean
  label: string
}>()
const emit = defineEmits<{ toggle: [] }>()
</script>

<template>
  <button
    type="button"
    class="toggle"
    role="switch"
    :aria-checked="checked"
    :aria-label="label"
    :disabled="disabled"
    @click="emit('toggle')"
  >
    <span class="toggle-thumb" />
  </button>
</template>
