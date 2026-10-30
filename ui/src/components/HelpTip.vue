<script setup lang="ts">
import { nextTick, onUnmounted, ref, useId } from 'vue'
import { CircleHelp } from 'lucide-vue-next'

defineProps<{ text: string; label: string }>()

const WIDTH = 280
const GAP = 8
const EDGE = 16

const id = useId()
const button = ref<HTMLButtonElement | null>(null)
const popup = ref<HTMLElement | null>(null)
const open = ref(false)
const position = ref({ left: 0, top: 0, width: WIDTH })

// Fixed and teleported so a scrolling dialog cannot clip it; below the icon unless that
// would run off the window.
function place() {
  const rect = button.value?.getBoundingClientRect()
  if (!rect) return
  const width = Math.min(WIDTH, window.innerWidth - EDGE * 2)
  const left = Math.min(Math.max(EDGE, rect.left + rect.width / 2 - width / 2), window.innerWidth - width - EDGE)
  const height = popup.value?.offsetHeight ?? 0
  const below = rect.bottom + GAP
  const top = below + height > window.innerHeight - EDGE ? Math.max(EDGE, rect.top - GAP - height) : below
  position.value = { left, top, width }
}

async function show() {
  if (open.value) return
  open.value = true
  window.addEventListener('scroll', hide, true)
  window.addEventListener('resize', hide)
  await nextTick()
  place()
}

function hide() {
  open.value = false
  window.removeEventListener('scroll', hide, true)
  window.removeEventListener('resize', hide)
}

onUnmounted(hide)
</script>

<template>
  <button
    ref="button"
    type="button"
    class="help-tip"
    :aria-label="label"
    :aria-describedby="open ? id : undefined"
    :aria-expanded="open"
    @mouseenter="show"
    @mouseleave="hide"
    @focus="show"
    @blur="hide"
    @click="open ? hide() : show()"
    @keydown.escape.stop="hide"
  >
    <CircleHelp :size="14" aria-hidden="true" />
  </button>
  <Teleport to="body">
    <div
      v-if="open"
      :id="id"
      ref="popup"
      class="help-tip-popup"
      role="tooltip"
      :style="{ left: `${position.left}px`, top: `${position.top}px`, width: `${position.width}px` }"
    >{{ text }}</div>
  </Teleport>
</template>
