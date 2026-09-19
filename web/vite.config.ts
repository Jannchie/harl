import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

// `harl serve` 默认监听 127.0.0.1:7777，并把 web/dist 嵌进二进制；
// 开发时 vite 只代理 /api 过去。后端还没起来时可以先 `pnpm mock`（mock/server.mjs 监听同一端口）。
export default defineConfig({
  plugins: [vue()],
  server: {
    proxy: {
      '/api': 'http://127.0.0.1:7777',
    },
  },
  build: {
    outDir: 'dist',
  },
})
