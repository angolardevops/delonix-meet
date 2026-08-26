/**
 * Service worker do Delonix Meet.
 *
 * O QUE MUDA FACE À VERSÃO ANTERIOR: aquela só guardava um recurso DEPOIS de
 * ele ter sido pedido com rede. Isso dá «funciona offline se já tiveres
 * visitado essa página» — que não é o que se promete a quem instala uma app.
 * Agora o esqueleto é guardado na INSTALAÇÃO, com a lista de ficheiros que o
 * build produziu, e uma navegação sem rede é servida do `index.html` em cache.
 *
 * A FRONTEIRA HONESTA: o Estúdio funciona 100% offline — grava, corta, analisa
 * pausas e guarda, tudo no dispositivo. As REUNIÕES não funcionam nem podem:
 * precisam do servidor de sinalização e do SFU por definição. Uma app que
 * dissesse «offline» e depois deixasse alguém tentar entrar numa sala sem rede
 * estaria a mentir. Por isso `/api`, `/ws` e `/rtc` nunca são servidos de
 * cache — falham, e a interface diz porquê.
 */

// Substituídos no build (ver o plugin `precachePwa` em vite.config.ts). Os
// valores literais aqui servem o `vite dev`, onde não há build para listar.
const VERSAO = '__PRECACHE_VERSAO__'.startsWith('__') ? 'dev' : '__PRECACHE_VERSAO__'
// eslint-disable-next-line no-undef
const FICHEIROS = typeof __PRECACHE_LISTA__ === 'undefined' ? [] : __PRECACHE_LISTA__

const CACHE = `delonix-${VERSAO}`
/** Cache separada para o que é grande e opcional (modelos de IA, wasm). */
const CACHE_PESADOS = `delonix-pesados-${VERSAO}`

self.addEventListener('install', (e) => {
  e.waitUntil(
    caches.open(CACHE).then((c) =>
      // `reload` ignora a cache HTTP: sem isto uma instalação logo a seguir a
      // um deploy podia guardar a versão antiga que o browser ainda tinha.
      c.addAll(FICHEIROS.map((f) => new Request(f, { cache: 'reload' }))).catch((err) => {
        // Um ficheiro em falta não pode impedir a instalação inteira — a app
        // ficaria sem service worker nenhum por causa de um asset.
        console.warn('[sw] precache incompleto', err)
      }),
    ),
  )
  self.skipWaiting()
})

self.addEventListener('activate', (e) => {
  e.waitUntil(
    caches
      .keys()
      .then((ks) => Promise.all(ks.filter((k) => k !== CACHE && k !== CACHE_PESADOS).map((k) => caches.delete(k))))
      .then(() => self.clients.claim()),
  )
})

/** Assets com hash no nome são imutáveis: da cache primeiro, sem revalidar. */
function imutavel(url) {
  return url.pathname.startsWith('/assets/')
}

/** Modelos e runtimes de IA: grandes, opcionais, e nunca mudam de conteúdo. */
function pesado(url) {
  return /^\/(models|mediapipe-wasm|ort|ort-rvm|vendor)\//.test(url.pathname)
}

self.addEventListener('fetch', (e) => {
  const url = new URL(e.request.url)
  if (e.request.method !== 'GET' || url.origin !== self.location.origin) return
  // Ver a nota da fronteira honesta no topo.
  if (url.pathname.startsWith('/api') || url.pathname === '/ws' || url.pathname === '/rtc') return

  // Navegação: rede primeiro (para apanhar deploys), esqueleto em cache se
  // falhar. É isto que faz a app abrir sem rede nenhuma.
  if (e.request.mode === 'navigate') {
    e.respondWith(
      fetch(e.request)
        .then((res) => {
          if (res.ok) {
            const copia = res.clone()
            caches.open(CACHE).then((c) => c.put('/index.html', copia))
          }
          return res
        })
        .catch(() => caches.match('/index.html').then((m) => m ?? Response.error())),
    )
    return
  }

  if (imutavel(url)) {
    e.respondWith(
      caches.match(e.request).then(
        (m) =>
          m ??
          fetch(e.request).then((res) => {
            if (res.ok) {
              const copia = res.clone()
              caches.open(CACHE).then((c) => c.put(e.request, copia))
            }
            return res
          }),
      ),
    )
    return
  }

  if (pesado(url)) {
    e.respondWith(
      caches.match(e.request).then(
        (m) =>
          m ??
          fetch(e.request).then((res) => {
            // Só o que veio INTEIRO: um 206 ou um erro guardado faz o modelo
            // falhar para sempre, e sem sintoma que aponte para aqui.
            if (res.ok && res.status === 200) {
              const copia = res.clone()
              caches.open(CACHE_PESADOS).then((c) => c.put(e.request, copia))
            }
            return res
          }),
      ),
    )
    return
  }

  e.respondWith(
    fetch(e.request)
      .then((res) => {
        if (res.ok && /\.(svg|png|ico|css|js|woff2?|webmanifest)$/.test(url.pathname)) {
          const copia = res.clone()
          caches.open(CACHE).then((c) => c.put(e.request, copia))
        }
        return res
      })
      .catch(() => caches.match(e.request).then((m) => m ?? Response.error())),
  )
})
