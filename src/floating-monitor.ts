import { createPinia } from 'pinia';
import { createApp } from 'vue';
import './assets/main.css';
import { i18n } from './i18n';
import FloatingMonitorPage from './pages/floating-monitor/index.vue';
import { useAppStore } from './stores/app-store';

const pinia = createPinia();
const app = createApp(FloatingMonitorPage).use(pinia).use(i18n);
void useAppStore(pinia)
  .loadSettings()
  .finally(() => app.mount('#app'));
