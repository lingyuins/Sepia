import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { pwaShell } from './vite-pwa-shell'

export default defineConfig({
  plugins: [react(), pwaShell()],
  server: { host: '0.0.0.0', port: 5173, watch: { ignored: ['**/backend/target/**', '**/backend/data/**'] }, proxy: Object.fromEntries(['/api', '/v1'].map((path) => [path, { target: process.env.SEPIA_GATEWAY_URL || 'http://127.0.0.1:5000', changeOrigin: false }])) },
  build: {
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (/node_modules\/(react|react-dom|scheduler)\//.test(id)) return 'react'
          if (id.includes('node_modules/recharts/') || id.includes('node_modules/d3-')) return 'charts'
        },
      },
    },
  },
})
