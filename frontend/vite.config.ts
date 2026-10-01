import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'
import pkg from './package.json' with { type: 'json' }

// The API lives on the ergo backend (make dev). Relative asset paths (`base`)
// let the built app work under Home Assistant's Ingress path prefix.
const backend = process.env.ERGO_BACKEND ?? 'http://127.0.0.1:8100'

export default defineConfig({
  base: './',
  plugins: [react()],
  // CI's release commit keeps package.json at the add-on's version.
  define: { __ERGO_VERSION__: JSON.stringify(pkg.version) },
  server: {
    port: 5173,
    proxy: {
      '/api': backend,
      '/health': backend,
      '/ready': backend,
    },
  },
})
