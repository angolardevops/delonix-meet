import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

/**
 * `web-admin/` — a consola de operador. Sem PWA, sem TLS de desenvolvimento,
 * sem IA: é uma app pequena, só para quem está em `PLATFORM_ADMIN_USER_IDS`.
 *
 * O MESMO porto de API que `web/vite.config.ts` usa (`API_PORT`, por omissão
 * 8180) — as duas apps falam com o mesmo servidor, cada uma do seu lado do
 * proxy em desenvolvimento. Em produção (PR4) esta app serve-se do MESMO
 * origin que a API: ver a nota de CORS no PR3.
 */
export default defineConfig(() => {
  const apiPort = Number(process.env.API_PORT) || 8180
  const apiHost = process.env.API_HOST || '127.0.0.1'
  return {
    plugins: [react()],
    server: {
      host: '0.0.0.0',
      port: Number(process.env.PORT) || 5174,
      proxy: {
        '/api': { target: `http://${apiHost}:${apiPort}`, changeOrigin: true, ws: true },
      },
    },
    test: {
      environment: 'jsdom',
      setupFiles: ['./src/setupTests.ts'],
    },
  }
})
