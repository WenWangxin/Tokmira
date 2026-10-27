import { ref } from 'vue'

export type ToastTone = 'success' | 'warning' | 'error' | 'info'

export interface Toast {
  id: number
  text: string
  tone: ToastTone
}

/** Messages shown at the top of the window, newest last. */
export const toasts = ref<Toast[]>([])

const MAX_VISIBLE = 3
const DURATION: Record<ToastTone, number> = { success: 3000, info: 3000, warning: 5000, error: 6000 }
const timers = new Map<number, ReturnType<typeof setTimeout>>()
let nextId = 1

export function dismissToast(id: number) {
  const timer = timers.get(id)
  if (timer) clearTimeout(timer)
  timers.delete(id)
  toasts.value = toasts.value.filter(item => item.id !== id)
}

/** Shows a short message that closes by itself; the same text is not stacked twice. */
export function showToast(text: string, tone: ToastTone = 'info', duration = DURATION[tone]): number {
  const message = text.trim()
  if (!message) return 0
  const existing = toasts.value.find(item => item.text === message && item.tone === tone)
  if (existing) dismissToast(existing.id)
  const id = nextId++
  toasts.value = [...toasts.value, { id, text: message, tone }].slice(-MAX_VISIBLE)
  timers.set(id, setTimeout(() => dismissToast(id), duration))
  return id
}
