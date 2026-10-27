<script setup lang="ts">
import { nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { ChartColumn, Gauge, LayoutDashboard, Lightbulb, List, PanelLeftClose, PanelLeftOpen, Settings2, ShieldCheck } from 'lucide-vue-next'
import { RouterLink, RouterView, useRoute, useRouter } from 'vue-router'
import { listen } from '@tauri-apps/api/event'
import { getSettings, refreshQuotas, type AdviceAlert } from './api'
import OnboardingDialog from './components/OnboardingDialog.vue'
import ToastHost from './components/ToastHost.vue'
import { showToast } from './toast'
import { previewOnboardingForced } from './onboardingLogic'
import { onboardingOpen, openOnboarding } from './onboardingState'
import { settingsRevision, startRealtime } from './realtime'
import { pages } from './router'

const icons = [LayoutDashboard, ChartColumn, Lightbulb, List, Settings2]
// On macOS the window uses an overlay title bar, so the traffic lights sit on
// top of the sidebar. The browser preview on a Mac mimics the same layout.
const macOverlay = /Mac/.test(navigator.userAgent) && ('__TAURI_INTERNALS__' in window || import.meta.env.DEV)
const router = useRouter()
const route = useRoute()
const content = ref<HTMLElement | null>(null)
const activeContentPath = ref(route.path)
const navigation = ref<HTMLElement | null>(null)
const expanded = ref(readExpanded())
const onboardingDuration = window.matchMedia('(prefers-reduced-motion: reduce)').matches
  ? { enter: 80, leave: 80 }
  : { enter: 320, leave: 200 }
const indicatorTop = ref(0)
const indicatorHeight = ref(16)
const indicatorVisible = ref(false)
let currentIndex: number | null = null
let indicatorFrame = 0

function readExpanded() {
  try {
    return localStorage.getItem('token-audit.sidebar-expanded') !== 'false'
  } catch {
    return true
  }
}

function preparePageEnter() {
  activeContentPath.value = route.path
  if (content.value) content.value.scrollTop = 0
}

function toggleSidebar() {
  expanded.value = !expanded.value
  try {
    localStorage.setItem('token-audit.sidebar-expanded', String(expanded.value))
  } catch {
    // The sidebar remains usable when storage is unavailable.
  }
}

function easeOutCubic(value: number) { return 1 - (1 - value) ** 3 }
function easeInOutCubic(value: number) {
  return value < 0.5 ? 4 * value ** 3 : 1 - (-2 * value + 2) ** 3 / 2
}

function indicatorPosition(index: number) {
  const item = navigation.value?.querySelectorAll<HTMLElement>('.nav-item')[index]
  return item ? item.offsetTop + (item.offsetHeight - 16) / 2 : null
}

function placeIndicator(index: number) {
  const top = indicatorPosition(index)
  if (top === null) return
  indicatorTop.value = top
  indicatorHeight.value = 16
  indicatorVisible.value = true
}

watch(() => route.path, async (path, previousPath) => {
  if (path !== previousPath) {
    await nextTick()
  }

  const targetIndex = pages.findIndex(page => page.path === path)
  cancelAnimationFrame(indicatorFrame)
  if (targetIndex < 0) {
    indicatorVisible.value = false
    currentIndex = null
    return
  }
  if (currentIndex === null || currentIndex === targetIndex || window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
    placeIndicator(targetIndex)
    currentIndex = targetIndex
    return
  }

  const start = indicatorPosition(currentIndex)
  const end = indicatorPosition(targetIndex)
  if (start === null || end === null) return
  const downward = end > start
  const startedAt = performance.now()
  currentIndex = targetIndex
  const frame = (now: number) => {
    const progress = Math.min(1, (now - startedAt) / 320)
    const leading = easeOutCubic(Math.min(1, progress / 0.6))
    const trailing = easeInOutCubic(Math.max(0, (progress - 0.35) / 0.65))
    const top = start + (end - start) * (downward ? trailing : leading)
    const bottom = start + (end - start) * (downward ? leading : trailing)
    indicatorTop.value = top
    indicatorHeight.value = bottom - top + 16
    indicatorVisible.value = true
    if (progress < 1) indicatorFrame = requestAnimationFrame(frame)
    else placeIndicator(targetIndex)
  }
  indicatorFrame = requestAnimationFrame(frame)
}, { immediate: true })

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
// Live updates exist only in the desktop app; the browser preview has no backend events.
const stopRealtime = '__TAURI_INTERNALS__' in window
  ? startRealtime({
      listen: handler => listen('usage-changed', handler),
      refreshQuotas: () => refreshQuotas(true),
      visible: () => document.visibilityState === 'visible',
    })
  : undefined
// Background reminders arrive here instead of the system while the window has focus.
const stopAlerts = '__TAURI_INTERNALS__' in window
  ? listen<AdviceAlert>('advice-alert', event => { showToast(`${event.payload.title}：${event.payload.body}`, 'warning', 10_000) })
  : undefined
const stopNavigate = '__TAURI_INTERNALS__' in window
  ? listen<string>('navigate', event => { void router.push(event.payload) })
  : undefined
const stopSettings = '__TAURI_INTERNALS__' in window
  ? listen('settings-changed', () => { settingsRevision.value += 1 })
  : undefined
onUnmounted(() => { cancelAnimationFrame(indicatorFrame); stopRealtime?.(); void stopAlerts?.then(stop => stop()); void stopNavigate?.then(stop => stop()); void stopSettings?.then(stop => stop()) })
</script>

<template>
  <div class="app-shell" :class="{ 'sidebar-expanded': expanded, 'mac-overlay': macOverlay }">
    <div v-if="macOverlay" class="titlebar-drag" data-tauri-drag-region />
    <aside class="sidebar" aria-label="主导航">
      <div v-if="macOverlay" class="sidebar-titlebar" data-tauri-drag-region />
      <div class="brand" data-tauri-drag-region>
        <!-- Figma Logo (20:28): 17px gauge with a 2px stroke, so 2 * 24 / 17 in Lucide's 24-unit box. -->
        <span class="brand-icon" aria-hidden="true"><Gauge :size="17" :stroke-width="2 * 24 / 17" /></span>
        <div class="brand-copy">
          <strong class="brand-name">token-audit</strong>
          <span class="brand-subtitle">本地用量洞察</span>
        </div>
        <button type="button" class="sidebar-toggle" :aria-label="expanded ? '收起侧边栏' : '展开侧边栏'" :aria-expanded="expanded" @click="toggleSidebar">
          <PanelLeftClose v-if="expanded" :size="20" :stroke-width="2" aria-hidden="true" />
          <PanelLeftOpen v-else :size="20" :stroke-width="2" aria-hidden="true" />
        </button>
      </div>
      <nav ref="navigation" class="navigation" aria-label="页面">
        <span v-if="indicatorVisible" class="nav-indicator" :style="{ top: indicatorTop + 'px', height: indicatorHeight + 'px' }" aria-hidden="true" />
        <RouterLink
          v-for="(page, index) in pages"
          :key="page.path"
          :to="page.path"
          class="nav-item"
          :class="{ 'nav-item-active': route.path === page.path }"
          :aria-current="route.path === page.path ? 'page' : undefined"
          :title="expanded ? undefined : page.title"
        >
          <span class="nav-row">
            <span class="nav-icon"><component :is="icons[index]" :size="20" :stroke-width="2" aria-hidden="true" /></span>
            <span class="nav-label">{{ page.title }}</span>
          </span>
        </RouterLink>
      </nav>

      <div class="privacy-card" :title="expanded ? undefined : '数据仅存于本机 · 零网络请求'">
        <span class="privacy-icon"><ShieldCheck :size="24" :stroke-width="2" aria-hidden="true" /></span>
        <span class="privacy-copy">
          <strong>数据仅存于本机</strong>
          <span>零网络请求</span>
        </span>
      </div>
    </aside>

    <main ref="content" class="content" :class="{ 'content-dashboard': activeContentPath === '/', 'content-usage': activeContentPath === '/usage', 'content-sessions': activeContentPath === '/sessions', 'content-settings': activeContentPath === '/settings' }">
      <RouterView v-slot="{ Component, route: viewedRoute }">
        <Transition name="page-fade-through" mode="out-in" @before-enter="preparePageEnter">
          <component :is="Component" :key="viewedRoute.path" />
        </Transition>
      </RouterView>
    </main>
  </div>
  <ToastHost />
  <Transition name="onboarding-dialog" appear :duration="onboardingDuration">
    <OnboardingDialog v-if="onboardingOpen" />
  </Transition>
</template>
