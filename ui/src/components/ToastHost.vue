<script setup lang="ts">
import { CircleAlert, CircleCheck, Info, TriangleAlert, X } from 'lucide-vue-next'
import { dismissToast, toasts, type ToastTone } from '../toast'

const icons: Record<ToastTone, unknown> = { success: CircleCheck, warning: TriangleAlert, error: CircleAlert, info: Info }
</script>

<template>
  <div class="toast-host" role="status" aria-live="polite">
    <TransitionGroup name="toast" tag="div" class="toast-stack">
      <div v-for="item in toasts" :key="item.id" class="toast" :class="`toast-${item.tone}`">
        <component :is="icons[item.tone]" class="toast-icon" :size="18" aria-hidden="true" />
        <span class="toast-text">{{ item.text }}</span>
        <button type="button" class="toast-close" aria-label="关闭提示" @click="dismissToast(item.id)"><X :size="15" aria-hidden="true" /></button>
      </div>
    </TransitionGroup>
  </div>
</template>
