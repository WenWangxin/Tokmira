<script setup lang="ts">
import { ChevronRight } from 'lucide-vue-next'
import ClaudeLogo from '../assets/logos/claude.svg'
import CodexLogo from './CodexLogo.vue'
import type { SourceRow } from '../sourceStatus'

defineProps<{ rows: SourceRow[]; logsUpdated: string | null; loadError: string; busy: boolean }>()
defineEmits<{ authorize: []; settings: [] }>()
</script>

<template>
  <section class="source-panel" role="dialog" aria-label="数据来源">
    <header class="source-panel-head">
      <strong>数据来源</strong>
      <span v-if="logsUpdated">本机日志实时监听 · {{ logsUpdated }}</span>
    </header>
    <p v-if="loadError" class="source-panel-error">{{ loadError }}</p>
    <ul class="source-rows">
      <li v-for="row in rows" :key="row.key" class="source-row">
        <span class="source-logo"><img v-if="row.tool === 'Claude'" :src="ClaudeLogo" alt="" /><CodexLogo v-else /></span>
        <span class="source-main">
          <strong>{{ row.title }}</strong>
          <span class="source-status" :class="`source-${row.tone}`"><i aria-hidden="true" />{{ row.status }}</span>
        </span>
        <button v-if="row.action === 'authorize'" type="button" class="source-action source-action-primary" :disabled="busy" @click="$emit('authorize')">授权</button>
        <button v-else-if="row.action === 'settings'" type="button" class="source-action" @click="$emit('settings')">去设置 <ChevronRight :size="14" aria-hidden="true" /></button>
      </li>
    </ul>
  </section>
</template>
