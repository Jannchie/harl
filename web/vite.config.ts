import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import UnoCSS from 'unocss/vite'

// `harl serve` 默认监听 127.0.0.1:7777，并把 web/dist 嵌进二进制；
// 开发时 vite 只代理 /api 过去：根目录 `pnpm dev` 会同时起 `harl serve` 和 vite。
// 不想编译 Rust 时可以在这里 `pnpm mock`（mock/server.mjs 监听同一端口）替代后端。
export default defineConfig({
  plugins: [vue(), UnoCSS()],
  server: {
    // Windows 上 Git Bash 崩溃会在当前目录写 bash.exe.stackdump，写的时候被锁，watcher 读到 EBUSY 会让 vite 退出
    watch: { ignored: ['**/*.stackdump'] },
    proxy: {
      '/api': 'http://127.0.0.1:7777',
    },
  },
  build: {
    outDir: 'dist',
  },
})
