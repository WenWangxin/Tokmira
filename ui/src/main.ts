import { createApp } from 'vue'
import App from './App.vue'
import router from './router'
import './styles/tokens.css'
import './styles/app.css'
import './styles/dashboard.css'
import './styles/usage.css'
import './styles/advice.css'

createApp(App).use(router).mount('#app')
