import { createRouter, createWebHistory } from 'vue-router'
import SessionHome from './views/SessionHome.vue'
import SessionView from './views/SessionView.vue'

export const router = createRouter({
  // 后端对非 /api 路径回退到 index.html
  history: createWebHistory(),
  routes: [
    // 仪表盘带 ECharts，单独切一个 chunk，看会话不用先下它
    { path: '/', name: 'dashboard', component: () => import('./views/Dashboard.vue') },
    { path: '/sessions', name: 'sessions', component: SessionHome },
    { path: '/s/:id', name: 'session', component: SessionView, props: true },
    { path: '/:pathMatch(.*)*', redirect: '/' },
  ],
})

// 滚动的是 <main>，不是 window，vue-router 的 scrollBehavior 管不到，自己来：
// 换页回到顶部，带 hash 的滚到那个锚点
router.afterEach((to, from) => {
  if (to.path === from.path && to.hash === from.hash) return
  requestAnimationFrame(() => {
    const root = document.querySelector<HTMLElement>('[data-scroll-root]')
    const el = to.hash ? document.getElementById(decodeURIComponent(to.hash.slice(1))) : null
    if (el) el.scrollIntoView({ block: 'start' })
    else if (to.path !== from.path) root?.scrollTo({ top: 0 })
  })
})
