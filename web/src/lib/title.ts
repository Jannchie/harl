// 页面标题：标签页、历史记录和读屏的换页播报都靠 document.title。
// 路由给默认标题（router.afterEach），页面可以再覆盖（会话页用会话标题）
export function setTitle(title: string | undefined) {
  document.title = title ? `${title} · harl` : 'harl'
}
