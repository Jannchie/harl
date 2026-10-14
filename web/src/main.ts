import { createApp } from 'vue'
import App from './App.vue'
import { i18n } from './i18n'
import { router } from './router'
import 'virtual:uno.css'
import './style.css'

createApp(App).use(i18n).use(router).mount('#app')
