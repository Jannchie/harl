import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import UnoCSS from 'unocss/vite'

// `harl serve` 默认监听 127.0.0.1:7461，vite 固定 7462（两个都避开 5173 / 7777 这类常用端口，和别的项目一起开也不撞）；
// harl serve 会把 web/dist 嵌进二进制；
// 开发时 vite 只代理 /api 过去：根目录 `pnpm dev` 会同时起 `harl serve` 和 vite。
// 不想编译 Rust 时可以在这里 `pnpm mock`（mock/server.mjs 监听同一端口）替代后端。
export default defineConfig({
  plugins: [vue(), UnoCSS()],
  server: {
    port: 7462,
    strictPort: true,
    // Windows 上 Git Bash 崩溃会在当前目录写 bash.exe.stackdump，写的时候被锁，watcher 读到 EBUSY 会让 vite 退出
    watch: { ignored: ['**/*.stackdump'] },
    proxy: {
      // HARL_API 可以指到别的 harl serve（比如同时跑两份后端对比）
      '/api': process.env.HARL_API ?? 'http://127.0.0.1:7461',
    },
  },
  build: {
    outDir: 'dist',
  },
})
