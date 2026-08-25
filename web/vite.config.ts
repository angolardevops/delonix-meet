import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import basicSsl from '@vitejs/plugin-basic-ssl'

import fs from 'fs'

const KEY = '../deploy/certs/meet.delonix.local.key'
const CRT = '../deploy/certs/meet.delonix.local.crt'

// NO_HTTPS=1 desliga o TLS (preview local); na rede é preciso HTTPS para o
// browser permitir câmara/microfone (getUserMedia).
export default defineConfig(({ command }) => {
  // TLS só no dev server (`serve`). No `build` (produção/contentor) NÃO se leem
  // certos — senão o build falha onde os certos de dev não existem (ex.: imagem
  // Docker). Se estivermos a servir mas sem os certos mkcert, cai no basic-ssl.
  const wantHttps = command === 'serve' && !process.env.NO_HTTPS
  const haveCerts = wantHttps && fs.existsSync(KEY) && fs.existsSync(CRT)
  return {
    plugins: [react(), ...(wantHttps && !haveCerts ? [basicSsl()] : [])],
    server: {
      host: '0.0.0.0',
      port: Number(process.env.PORT) || 5173,
      // COOP+COEP: tornam a página "cross-origin isolated" → ativam
      // SharedArrayBuffer → o WASM multi-thread do ONNX Runtime (RVM) e do
      // MediaPipe arranca. O nginx de produção já os põe
      // (deploy/k8s/nginx.conf); o dev server NÃO punha, e o efeito era este:
      // os fundos e efeitos da SALA e o recorte sem fundo do ESTÚDIO falhavam
      // em silêncio só em desenvolvimento. Medido: `crossOriginIsolated`
      // dava `false` no dev server e `true` em produção.
      // A app é toda da mesma origem, por isso o COEP não bloqueia nada.
      headers: {
        'Cross-Origin-Opener-Policy': 'same-origin',
        'Cross-Origin-Embedder-Policy': 'require-corp',
      },
      https: haveCerts
        ? { key: fs.readFileSync(KEY), cert: fs.readFileSync(CRT) }
        : wantHttps
          ? undefined // o plugin basic-ssl injeta um cert self-signed
          : false,
      proxy: {
        '/api': { target: 'http://127.0.0.1:8180', changeOrigin: true },
        '/ws': { target: 'ws://127.0.0.1:8180', ws: true, changeOrigin: true },
        '/rtc': { target: 'ws://127.0.0.1:8180', ws: true, changeOrigin: true },
      },
    },
  }
})
