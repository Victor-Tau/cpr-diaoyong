import vue from '@vitejs/plugin-vue'
import { defineConfig } from 'vite'

export default defineConfig({
  base: './',
  plugins: [
    vue(),
    {
      name: 'plugin-classic-script',
      // 宿主隔离页加载经典脚本
      transformIndexHtml: {
        order: 'post',
        handler: html => html.replace('type="module" crossorigin', 'defer'),
      },
    },
  ],
  build: {
    modulePreload: false,
    cssCodeSplit: false,
    rolldownOptions: {
      output: {
        format: 'iife',
        codeSplitting: false,
        entryFileNames: 'app.js',
        assetFileNames: 'app.[ext]',
      },
    },
  },
})
