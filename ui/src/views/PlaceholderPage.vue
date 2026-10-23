<script setup lang="ts">
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
    <section class="placeholder-card" :aria-label="`${title}内容`">
      <p>即将实现</p>
    </section>
  </div>
</template>
