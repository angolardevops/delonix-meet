import { createRoot } from 'react-dom/client'
// As folhas globais entram ANTES do App: as páginas eager (Entrar, Início)
// importam as suas folhas e têm de vencer a base na cascata, não perder.
import './ui/tokens.css'
import './ui/base.css'
import './ui/shell.css'
import App from './App'
import { initTheme } from './theme'
// Fontes do template, self-hosted — nada sai da rede local.
// Archivo (interface), Archivo Black (display) e DM Mono (horas, códigos,
// débitos): os números alinham em coluna por `tabular-nums`.
import '@fontsource/archivo/400.css'
import '@fontsource/archivo/500.css'
import '@fontsource/archivo/600.css'
import '@fontsource/archivo/700.css'
import '@fontsource/archivo-black/400.css'
import '@fontsource/dm-mono/400.css'
import '@fontsource/dm-mono/500.css'
import { initLanguage } from './i18n'
import { currentUser } from './api'

initTheme()

void import('./branding').then((b) => (document.title = b.getAppName())).catch(() => {})

// O idioma resolve-se ANTES do primeiro render (sem flash de português). O
// locale guardado na conta ganha ao do browser — sincroniza dispositivos.
void initLanguage(currentUser()?.locale).finally(() => {
  createRoot(document.getElementById('root')!).render(<App />)
})

// PWA: instalável, e o Estúdio funciona sem rede depois do primeiro arranque.
// Regista-se em qualquer contexto seguro (incluindo http://localhost), mas
// nunca no servidor de desenvolvimento — um SW a guardar módulos do Vite dá
// uma app que serve código velho sem ninguém perceber porquê.
if ('serviceWorker' in navigator && window.isSecureContext && !import.meta.env.DEV) {
  window.addEventListener('load', () => {
    navigator.serviceWorker.register('/sw.js').catch(() => {
      /* certificado não confiável / offline — a app funciona sem PWA */
    })
  })
}
