import { createRouter, createWebHashHistory } from 'vue-router'
import PlaceholderPage from './views/PlaceholderPage.vue'
import DashboardPage from './views/DashboardPage.vue'
import UsagePage from './views/UsagePage.vue'
import AdvicePage from './views/AdvicePage.vue'
import SessionsPage from './views/SessionsPage.vue'
import SettingsPage from './views/SettingsPage.vue'

export const pages = [
  { path: '/', title: '仪表盘', subtitle: '查看订阅额度与本地使用概况' },
  { path: '/usage', title: '用量明细', subtitle: '逐一追溯额度去向，精确到会话与单条提示' },
  { path: '/advice', title: '省额度建议', subtitle: '查找异常消耗与节省空间' },
  { path: '/sessions', title: '会话', subtitle: '按会话查看本地使用记录' },
  { path: '/settings', title: '设置', subtitle: '管理数据来源、隐私与通用偏好' },
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
    props: page.path === '/sessions' || page.path === '/settings' ? false : { title: page.title, subtitle: page.subtitle },
  })),
})

export default router
