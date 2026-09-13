import { createRoot } from 'react-dom/client'
import App from './App'
import { initTheme } from './theme'
// Pré-carregar os dois pesos que o primeiro pixel usa de verdade (corpo 400,
// títulos/nav 600) — achado 1.5: seis pesos, nenhum pré-carregado. O URL final
// (hash incluído) só o Vite sabe, daí o `?url` em vez de um <link> estático em
// index.html.
import sans400 from '@fontsource/ibm-plex-sans/files/ibm-plex-sans-latin-400-normal.woff2?url'
import sans600 from '@fontsource/ibm-plex-sans/files/ibm-plex-sans-latin-600-normal.woff2?url'
// Fontes do design system (self-hosted — nada sai da rede local).
// Família única IBM Plex (Sans + Mono): corpo, títulos e dados numéricos
// partilham a mesma métrica — é o que dá o look «consola» do template.
import '@fontsource/ibm-plex-sans/400.css'
import '@fontsource/ibm-plex-sans/500.css'
import '@fontsource/ibm-plex-sans/600.css'
import '@fontsource/ibm-plex-sans/700.css'
import '@fontsource/ibm-plex-mono/400.css'
import '@fontsource/ibm-plex-mono/500.css'
import { initLanguage } from './i18n'
import './styles.scss'
import { currentUser } from './api'

for (const href of [sans400, sans600]) {
  const link = document.createElement('link')
  link.rel = 'preload'
  link.as = 'font'
  link.type = 'font/woff2'
  link.crossOrigin = 'anonymous'
  link.href = href
  document.head.appendChild(link)
}

initTheme()

void import('./branding').then((b) => (document.title = b.getAppName())).catch(() => {})

// O idioma resolve-se ANTES do primeiro render. Em PT (o caso comum) isto
// resolve no mesmo tick — o dicionário já está no bundle. Em EN/FR espera pelo
// `import()` do dicionário, o que troca ~1 ida ao servidor por não haver flash
// de português. O locale da BD ganha ao localStorage (sincroniza dispositivos).
void initLanguage(currentUser()?.locale).finally(() => {
  createRoot(document.getElementById('root')!).render(<App />)
})

// PWA: instalável, e o Estúdio funciona sem rede depois do primeiro arranque.
//
// A condição era `https: && !localhost`, o que excluía DOIS casos legítimos:
// um `http://localhost` (que é contexto seguro por definição e onde o SW
// funciona) e qualquer instalação self-hosted servida em HTTP numa rede
// interna e aberta por `localhost`. O que é preciso excluir não é o localhost
// — é o servidor de DESENVOLVIMENTO, onde um SW a guardar módulos do Vite dá
// uma app teimosa que serve código velho e ninguém percebe porquê.
if ('serviceWorker' in navigator && window.isSecureContext && !import.meta.env.DEV) {
  window.addEventListener('load', () => {
    navigator.serviceWorker.register('/sw.js').catch(() => {
      /* cert não confiável / offline — a app funciona na mesma sem PWA */
    })
  })
}
