import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  server: {
    host: 'localhost',
    port: 1420,
    strictPort: true,
  },
})
