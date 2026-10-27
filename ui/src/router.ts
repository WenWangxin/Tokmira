import { createRouter, createWebHashHistory } from 'vue-router'
import PlaceholderPage from './views/PlaceholderPage.vue'
import DashboardPage from './views/DashboardPage.vue'
import UsagePage from './views/UsagePage.vue'
import AdvicePage from './views/AdvicePage.vue'
import SessionsPage from './views/SessionsPage.vue'
import SettingsPage from './views/SettingsPage.vue'

export const pages = [
  { path: '/', key: 'dashboard' },
  { path: '/usage', key: 'usage' },
  { path: '/advice', key: 'advice' },
  { path: '/sessions', key: 'sessions' },
  { path: '/settings', key: 'settings' },
] as const

function pageComponent(path: string) {
  if (path === '/') return DashboardPage
  if (path === '/usage') return UsagePage
  if (path === '/advice') return AdvicePage
  if (path === '/sessions') return SessionsPage
  if (path === '/settings') return SettingsPage
  return PlaceholderPage
}

const router = createRouter({
  history: createWebHashHistory(),
  routes: pages.map((page) => ({
    path: page.path,
    component: pageComponent(page.path),
  })),
})

export default router
