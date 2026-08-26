import { defineConfig, type Plugin } from 'vite'
import react from '@vitejs/plugin-react'
import basicSsl from '@vitejs/plugin-basic-ssl'

import fs from 'fs'
import { createHash } from 'node:crypto'
import { resolve } from 'node:path'


/**
 * Escreve no `sw.js` construído a lista REAL de ficheiros a pré-carregar.
 *
 * Sem isto o service worker não pode saber o que guardar: os nomes têm hash e
 * mudam a cada build. A alternativa — guardar só o que já foi pedido — dá
 * «funciona offline se já lá tiveres estado», que não é offline.
 *
 * O que entra: o esqueleto (index.html, ícones, manifesto) e os assets do
 * arranque. O que NÃO entra: os chunks das rotas pesadas e os modelos de IA —
 * juntos passam de 30 MB, e obrigar toda a gente a descarregá-los na
 * instalação para o caso de virem a usar o Estúdio seria pagar por todos o que
 * só alguns querem. Esses ficam em cache quando forem usados pela primeira vez.
 */
function precachePwa(): Plugin {
  return {
    name: 'delonix-precache-pwa',
    apply: 'build',
    enforce: 'post',
    // `writeBundle`, não `generateBundle`: o `public/` é copiado para o `dist`
    // DEPOIS do bundle e nunca passa pelo Rollup, por isso o `sw.js` não existe
    // no objecto `bundle`. A primeira versão disto usava o hook errado e o
    // plugin avisou em vez de falhar em silêncio — que é a única razão de o
    // aviso existir.
    writeBundle(opcoes, bundle) {
      // O ESQUELETO mais o ESTÚDIO. A primeira versão levava só o esqueleto, e
      // com a rede cortada a app abria mas o Estúdio não — o chunk da rota não
      // estava em cache. A promessa «o Estúdio funciona offline» era falsa, e
      // foi um teste com a rede realmente cortada que o mostrou.
      //
      // O Estúdio entra porque é O que se promete offline e custa ~58 KB. Não
      // entram: a `Room` (precisa do servidor por definição), o `whisperWorker`
      // (816 KB), o `matte` (416 KB) e os modelos — juntos passam dos 30 MB, e
      // obrigar toda a gente a descarregá-los na instalação seria pagar por
      // todos o que só alguns usam. Esses ficam em cache no primeiro uso.
      const nomes = Object.keys(bundle)

      // O FECHO das importações estáticas, não uma lista escrita à mão.
      //
      // A primeira versão escolhia ficheiros por padrão de NOME (`Studio-*`) e
      // deixou de fora o `media-*.js`, que o Estúdio importa. Com a rede
      // cortada a rota rebentava e o React ficava com a raiz vazia. Uma lista
      // de precache tem de vir do grafo de dependências, que o Rollup conhece —
      // adivinhá-la pelo nome falha exactamente no ficheiro em que ninguém
      // pensou.
      const fecho = (raizes: string[]): string[] => {
        const vistos = new Set<string>()
        const pilha = [...raizes]
        while (pilha.length) {
          const f = pilha.pop()!
          if (vistos.has(f)) continue
          vistos.add(f)
          const parte = bundle[f]
          if (parte && parte.type === 'chunk') pilha.push(...parte.imports)
        }
        return [...vistos]
      }

      const arranque = nomes.filter((f) => /^assets\/index-[^/]+\.js$/.test(f))
      const estilos = nomes.filter((f) => /^assets\/index-[^/]+\.css$/.test(f))
      // O Estúdio é o que se promete offline; a `Room` fica de fora porque
      // precisa do servidor por definição, e os modelos de IA porque juntos
      // passam dos 30 MB — esses ficam em cache no primeiro uso.
      const estudio = nomes.filter((f) => /^assets\/Studio-[^/]+\.js$/.test(f))
      const lista = [
        '/',
        '/index.html',
        '/logo.svg',
        '/manifest.webmanifest',
        ...estilos.map((f) => '/' + f),
        ...fecho([...arranque, ...estudio]).map((f) => '/' + f),
      ]
      // A versão vem do CONTEÚDO: dois builds iguais dão a mesma cache, e um
      // build diferente invalida-a sozinho. Uma data ou um contador fariam
      // toda a gente descarregar tudo outra vez a cada deploy sem mudanças.
      const versao = createHash('sha256').update(lista.join('|')).digest('hex').slice(0, 12)
      const destino = resolve(opcoes.dir ?? 'dist', 'sw.js')
      if (!fs.existsSync(destino)) {
        this.warn('sw.js não está no dist — o precache do PWA não foi injectado')
        return
      }
      const texto = fs
        .readFileSync(destino, 'utf8')
        .replace(/'__PRECACHE_VERSAO__'\.startsWith\('__'\) \? 'dev' : '__PRECACHE_VERSAO__'/, JSON.stringify(versao))
        .replace(/typeof __PRECACHE_LISTA__ === 'undefined' \? \[\] : __PRECACHE_LISTA__/, JSON.stringify(lista))
      fs.writeFileSync(destino, texto)
      this.info(`precache do PWA: ${lista.length} ficheiros, versão ${versao}`)
    },
  }
}

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
    plugins: [react(), precachePwa(), ...(wantHttps && !haveCerts ? [basicSsl()] : [])],
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
        // `ws: true` no /api: a rota do DIRECTO
        // (`/api/rooms/{code}/broadcast`) é um WebSocket debaixo do prefixo
        // /api. Sem isto o vite responde ao upgrade com HTTP e o pedido nunca
        // chega ao servidor — sem erro em lado nenhum, nem no browser nem no
        // log do backend. Foi assim que o directo pareceu recusado quando na
        // verdade nunca foi tentado.
        '/api': { target: 'http://127.0.0.1:8180', changeOrigin: true, ws: true },
        '/ws': { target: 'ws://127.0.0.1:8180', ws: true, changeOrigin: true },
        '/rtc': { target: 'ws://127.0.0.1:8180', ws: true, changeOrigin: true },
      },
    },
  }
})
