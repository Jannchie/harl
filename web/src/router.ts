import { createRouter, createWebHistory } from 'vue-router'
import SessionHome from './views/SessionHome.vue'
import SessionView from './views/SessionView.vue'
import { setTitle } from './lib/title'

export const router = createRouter({
  // 后端对非 /api 路径回退到 index.html
  history: createWebHistory(),
  routes: [
    // 仪表盘带 ECharts，单独切一个 chunk，看会话不用先下它
    { path: '/', name: 'dashboard', component: () => import('./views/Dashboard.vue'), meta: { title: '概览' } },
    { path: '/sessions', name: 'sessions', component: SessionHome, meta: { title: '会话' } },
    // 会话页的标题等会话头加载完由页面自己设（会话标题）
    { path: '/s/:id', name: 'session', component: SessionView, props: true, meta: { title: '会话' } },
    { path: '/:pathMatch(.*)*', redirect: '/' },
  ],
})

// 滚动的是 <main>，不是 window，vue-router 的 scrollBehavior 管不到，自己来：
// 换页回到顶部，带 hash 的滚到那个锚点
router.afterEach((to, from) => {
  // 首次进入时 from 是 START_LOCATION，路径可能恰好相同
  if (to.path !== from.path || !from.matched.length) setTitle(to.meta.title as string | undefined)
  if (to.path === from.path && to.hash === from.hash) return
  requestAnimationFrame(() => {
    const root = document.querySelector<HTMLElement>('[data-scroll-root]')
    const el = to.hash ? document.getElementById(decodeURIComponent(to.hash.slice(1))) : null
    if (el) el.scrollIntoView({ block: 'start' })
    else if (to.path !== from.path) root?.scrollTo({ top: 0 })
  })
})
