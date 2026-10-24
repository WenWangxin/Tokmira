import { ref } from 'vue'

/** Shared by the shell, the dashboard, and the settings “open again” button. */
export const onboardingOpen = ref(false)
export const dashboardRefreshToken = ref(0)
export const pendingDashboardNotice = ref<{ text: string; tone: 'success' | 'warning' } | null>(null)

export function openOnboarding() {
  onboardingOpen.value = true
}

export function finishOnboarding(message: string, tone: 'success' | 'warning') {
  pendingDashboardNotice.value = message ? { text: message, tone } : null
  onboardingOpen.value = false
  dashboardRefreshToken.value += 1
}
