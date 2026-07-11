// Service worker mínimo do Delonix Meet: torna a app instalável e serve os
// assets estáticos de cache quando a rede falha. Nunca intercepta /api, /ws.
const CACHE = 'delonix-v2'
self.addEventListener('install', (e) => self.skipWaiting())
self.addEventListener('activate', (e) => {
  e.waitUntil(caches.keys().then((ks) => Promise.all(ks.filter((k) => k !== CACHE).map((k) => caches.delete(k)))))
  self.clients.claim()
})
self.addEventListener('fetch', (e) => {
  const url = new URL(e.request.url)
  if (e.request.method !== 'GET' || url.pathname.startsWith('/api') || url.pathname === '/ws' || url.pathname === '/rtc') return
  e.respondWith(
    fetch(e.request)
      .then((res) => {
        if (res.ok && (url.pathname.startsWith('/assets') || url.pathname.match(/\.(svg|png|css|js|woff2?)$/))) {
          const copy = res.clone()
          caches.open(CACHE).then((c) => c.put(e.request, copy))
        }
        return res
      })
      .catch(() => caches.match(e.request).then((m) => m ?? Response.error())),
  )
})
