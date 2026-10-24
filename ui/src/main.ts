import { createApp } from 'vue'
import { getSettings } from './api'
import { applyTheme } from './settingsLogic'
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

async function boot() {
  try {
    applyTheme((await getSettings()).theme)
  } catch {
    // Keep the system theme when settings cannot be read at startup.
  }
  createApp(App).use(router).mount('#app')
}
void boot()
