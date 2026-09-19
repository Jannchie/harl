import { createRouter, createWebHistory } from 'vue-router'
import SessionList from './views/SessionList.vue'
import SessionView from './views/SessionView.vue'

export const router = createRouter({
  // 后端对非 /api 路径回退到 index.html
  history: createWebHistory(),
  routes: [
    { path: '/', name: 'sessions', component: SessionList },
    { path: '/s/:id', name: 'session', component: SessionView, props: true },
    { path: '/:pathMatch(.*)*', redirect: '/' },
  ],
  scrollBehavior(to, _from, saved) {
    if (saved) return saved
    if (to.hash) return { el: to.hash, top: 40 }
    return { top: 0 }
  },
})
