<script setup lang="ts">
import { messages, m } from '../i18n/index.ts'
const t = messages
defineProps<{
  title: string
  subtitle: string
}>()
</script>

<template>
  <div class="page">
    <header class="page-header">
      <h1>{{ title }}</h1>
      <p>{{ subtitle }}</p>
    </header>
    <section class="placeholder-card" :aria-label="t.app.pageContent(title)">
      <p>{{ t.app.comingSoon }}</p>
    </section>
  </div>
</template>
