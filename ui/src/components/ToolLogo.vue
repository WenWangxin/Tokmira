<script setup lang="ts">
import type { Tool } from '../api'
import ClaudeLogo from '../assets/logos/claude.svg'
import DeepSeekLogo from '../assets/logos/deepseek.svg'
import CodexLogo from './CodexLogo.vue'

defineProps<{ tool: Tool }>()
</script>

<template>
  <img v-if="tool === 'Claude'" :src="ClaudeLogo" alt="" />
  <img v-else-if="tool === 'DeepSeek'" :src="DeepSeekLogo" alt="" />
  <CodexLogo v-else />
</template>
