<script setup lang="ts">
defineProps<{ kind: 'advice' | 'usage' | 'usageDetail' | 'sessions' | 'sessionDetail' | 'settings' }>()
</script>

<template>
  <div class="page-skeleton" :class="`page-skeleton-${kind}`" aria-label="正在加载数据">
    <template v-if="kind === 'advice'">
      <div class="page-skeleton-stats"><span v-for="i in 3" :key="i" class="skeleton" /></div>
      <span v-for="i in 3" :key="i" class="skeleton page-skeleton-advice-row" />
    </template>
    <template v-else-if="kind === 'usage'">
      <div class="page-skeleton-tabs"><span v-for="i in 4" :key="i" class="skeleton" /></div>
      <span class="skeleton page-skeleton-overview" />
      <div class="page-skeleton-columns"><span class="skeleton" /><span class="skeleton" /></div>
    </template>
    <template v-else-if="kind === 'usageDetail'">
      <span class="skeleton page-skeleton-overview" />
      <div class="page-skeleton-columns"><span class="skeleton" /><span class="skeleton" /></div>
    </template>
    <template v-else-if="kind === 'sessions'">
      <span class="skeleton page-skeleton-table" />
    </template>
    <template v-else-if="kind === 'sessionDetail'">
      <span class="skeleton page-skeleton-banner" />
      <span class="skeleton page-skeleton-settings-card" />
    </template>
    <template v-else>
      <span class="skeleton page-skeleton-banner" />
      <span class="skeleton page-skeleton-settings-card" />
      <span class="skeleton page-skeleton-settings-card" />
    </template>
  </div>
</template>
