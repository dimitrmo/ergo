import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

// The API lives on the ergo backend (make dev). Relative asset paths (`base`)
// let the built app work under Home Assistant's Ingress path prefix.
const backend = process.env.ERGO_BACKEND ?? 'http://127.0.0.1:8100'

export default defineConfig({
  base: './',
  plugins: [react()],
  server: {
    port: 5173,
    proxy: {
      '/api': backend,
      '/health': backend,
      '/ready': backend,
    },
  },
})
