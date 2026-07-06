import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import basicSsl from '@vitejs/plugin-basic-ssl'

// NO_HTTPS=1 desliga o TLS (preview local); na rede é preciso HTTPS
// para o browser permitir câmara/microfone (getUserMedia).
export default defineConfig({
  plugins: [react(), ...(process.env.NO_HTTPS ? [] : [basicSsl()])],
  server: {
    host: '0.0.0.0',
    port: Number(process.env.PORT) || 5173,
    proxy: {
      '/api': 'http://localhost:8180',
      '/ws': { target: 'ws://localhost:8180', ws: true },
      '/rtc': { target: 'ws://localhost:8180', ws: true },
    },
  },
})
