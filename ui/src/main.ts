import { createApp } from 'vue'
import { getSettings } from './api'
import { applyTheme } from './settingsLogic'
import { applyLanguage } from './i18n/index.ts'
import App from './App.vue'
import router from './router'
import './styles/tokens.css'
import './styles/app.css'
import './styles/dashboard.css'
import './styles/usage.css'
import './styles/advice.css'
import './styles/sessions.css'
import './styles/settings.css'
import './styles/onboarding.css'
import './styles/share.css'
import './styles/motion.css'
import './styles/toast.css'
import './styles/help-tip.css'

async function boot() {
  try {
    const settings = await getSettings()
    applyTheme(settings.theme)
    applyLanguage(settings.language)
  } catch {
    // Keep the system theme when settings cannot be read at startup.
    applyLanguage('system')
  }
  createApp(App).use(router).mount('#app')
}
void boot()
