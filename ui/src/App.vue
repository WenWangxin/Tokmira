<script setup lang="ts">
import { onMounted } from 'vue'
import { ChartColumn, LayoutDashboard, Lightbulb, List, Settings2, ShieldCheck } from 'lucide-vue-next'
import { RouterLink, RouterView, useRouter } from 'vue-router'
import { getSettings } from './api'
import OnboardingDialog from './components/OnboardingDialog.vue'
import { previewOnboardingForced } from './onboardingLogic'
import { onboardingOpen, openOnboarding } from './onboardingState'
import { pages } from './router'

const icons = [LayoutDashboard, ChartColumn, Lightbulb, List, Settings2]
const router = useRouter()

onMounted(async () => {
  const preview = import.meta.env.DEV && !('__TAURI_INTERNALS__' in window)
  const forced = previewOnboardingForced(window.location.search, preview)
  try {
    const settings = await getSettings()
    if (!forced && settings.onboarding_completed) return
  } catch {
    if (!forced) return
  }
  if (router.currentRoute.value.path !== '/') await router.replace('/')
  openOnboarding()
})
</script>

<template>
  <div class="app-shell">
    <aside class="sidebar" aria-label="主导航">
      <div class="brand">
        <img class="brand-icon" src="/brand.svg" alt="" width="28" height="28" />
        <div class="brand-copy">
          <strong class="brand-name">token-audit</strong>
          <span class="brand-subtitle">本地用量洞察</span>
        </div>
      </div>

      <nav class="navigation" aria-label="页面">
        <RouterLink
          v-for="(page, index) in pages"
          :key="page.path"
          :to="page.path"
          class="nav-item"
          active-class="nav-item-active"
          :exact-active-class="page.path === '/' ? 'nav-item-active' : undefined"
        >
          <component :is="icons[index]" :size="20" :stroke-width="1.8" aria-hidden="true" />
          <span>{{ page.title }}</span>
        </RouterLink>
      </nav>

      <div class="privacy-card">
        <div class="privacy-icon"><ShieldCheck :size="18" :stroke-width="2" aria-hidden="true" /></div>
        <div class="privacy-copy">
          <strong>数据仅存于本机</strong>
          <span>零网络请求</span>
        </div>
      </div>
    </aside>

    <main class="content" :class="{ 'content-dashboard': $route.path === '/', 'content-usage': $route.path === '/usage', 'content-sessions': $route.path === '/sessions', 'content-settings': $route.path === '/settings' }">
      <RouterView />
    </main>
  </div>
  <OnboardingDialog v-if="onboardingOpen" />
</template>
