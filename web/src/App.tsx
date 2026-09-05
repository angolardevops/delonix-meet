import { lazy, ReactNode, Suspense, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { AlertIcon } from './icons'
import { completeSsoLogin, currentUser, logout, User } from './api'
import Shell, { NavKey } from './components/Shell'
import PresenceProvider from './components/PresenceProvider'

// ---------------------------------------------------------------------------
//  Corte por rota (achado 1.2 do docs/ux-perf-review.md)
//
//  As 15 páginas eram importadas estaticamente aqui, o que punha a `Room`
//  (185 KB de fonte, mais webrtc/media/e2ee/signaling), o `Calendar` e o
//  `Analytics` no MESMO chunk que o dashboard. Ninguém vê a sala e a landing
//  ao mesmo tempo.
//
//  EAGER ficam só os três ecrãs de entrada — Landing, Login e Home. São
//  pequenos (~30 KB somados) e são o primeiro pixel: pô-los em `lazy` trocava
//  bytes por um spinner à frente de toda a gente, o que não é uma troca boa.
// ---------------------------------------------------------------------------
import Landing from './pages/Landing'
import Login from './pages/Login'
import Home from './pages/Home'

const Room = lazy(() => import('./pages/Room'))
const Lobby = lazy(() => import('./pages/Lobby'))
const Calendar = lazy(() => import('./pages/Calendar'))
const Analytics = lazy(() => import('./pages/Analytics'))
const Recordings = lazy(() => import('./pages/Recordings'))
const Directory = lazy(() => import('./pages/Directory'))
const Whiteboards = lazy(() => import('./pages/Whiteboards'))
const Studio = lazy(() => import('./pages/Studio'))
const Roadmap = lazy(() => import('./pages/Roadmap'))
const Status = lazy(() => import('./pages/Status'))
const ApiDocs = lazy(() => import('./pages/ApiDocs'))
const Legal = lazy(() => import('./pages/Legal'))
const SharePage = lazy(() => import('./pages/SharePage'))

/**
 * Espera de rota. Deliberadamente MUDO: o chunk de uma página chega em dezenas
 * de milissegundos na mesma origem, e um spinner que pisca nesse intervalo lê-se
 * como avaria. Só ocupa o espaço para o conteúdo não saltar quando chegar.
 */
function RouteFallback({ children }: { children: ReactNode }) {
  return <Suspense fallback={<div className="route-loading" aria-hidden="true" />}>{children}</Suspense>
}

type Route =
  | { kind: 'home' }
  | { kind: 'directory' }
  | { kind: 'recordings' }
  | { kind: 'whiteboards' }
  | { kind: 'studio' }
  | { kind: 'calendar' }
  | { kind: 'analytics' }
  | { kind: 'roadmap' }
  | { kind: 'room'; code: string; voice: boolean }
  | { kind: 'lobby'; code: string }
  | { kind: 'share'; token: string }

function parseHash(): Route {
  const h = location.hash
  const roomVoice = h.match(/^#\/r\/([a-z-]+)\?voice$/)
  if (roomVoice) return { kind: 'room', code: roomVoice[1], voice: true }
  const room = h.match(/^#\/r\/([a-z-]+)$/)
  if (room) return { kind: 'room', code: room[1], voice: false }
  const lobby = h.match(/^#\/lobby\/([a-z-]+)$/)
  if (lobby) return { kind: 'lobby', code: lobby[1] }
  const share = h.match(/^#\/share\/([a-f0-9]+)$/)
  if (share) return { kind: 'share', token: share[1] }
  if (h.startsWith('#/directory')) return { kind: 'directory' }
  if (h.startsWith('#/recordings')) return { kind: 'recordings' }
  if (h.startsWith('#/whiteboards')) return { kind: 'whiteboards' }
  if (h.startsWith('#/studio')) return { kind: 'studio' }
  if (h.startsWith('#/calendar')) return { kind: 'calendar' }
  if (h.startsWith('#/analytics')) return { kind: 'analytics' }
  if (h.startsWith('#/roadmap')) return { kind: 'roadmap' }
  return { kind: 'home' }
}

export default function App() {
  const { t } = useTranslation()
  const [user, setUser] = useState<User | null>(currentUser())
  const [route, setRoute] = useState<Route>(parseHash())

  useEffect(() => {
    const onHash = () => setRoute(parseHash())
    // Sessão expirada (refresh falhou): limpa o estado → mostra o login.
    const onExpired = () => {
      setUser(null)
      location.hash = '/login'
    }
    window.addEventListener('hashchange', onHash)
    window.addEventListener('dx-auth-expired', onExpired)
    return () => {
      window.removeEventListener('hashchange', onHash)
      window.removeEventListener('dx-auth-expired', onExpired)
    }
  }, [])

  function enterRoom(code: string, voice = false) {
    location.hash = `/r/${code}${voice ? '?voice' : ''}`
  }
  function leaveRoom() {
    location.hash = '/'
  }
  function navigate(key: NavKey) {
    location.hash = key === 'home' ? '/' : `/${key}`
  }

  // SSO callback: o servidor redireciona para #/sso-complete?token=<jwt>
  // após o IdP. Capturamos o token e completamos o login.
  useEffect(() => {
    if (location.hash.startsWith('#/sso-complete')) {
      completeSsoLogin().then((u) => {
        if (u) setUser(u)
        else location.hash = '/login'
      })
    }
  }, [])

  if (location.hash.startsWith('#/sso-complete')) {
    // Mostrar estado de carregamento enquanto completa o SSO.
    return <div className="auth-page"><div className="auth-card"><p>{t('common.aCompletarSso')}</p></div></div>
  }
  if (location.hash.startsWith('#/status')) return <RouteFallback><Status /></RouteFallback>
  if (location.hash.startsWith('#/api-docs')) return <RouteFallback><ApiDocs /></RouteFallback>
  if (location.hash.startsWith('#/legal')) return <RouteFallback><Legal /></RouteFallback>
  // Link público de gravação — sem autenticação necessária.
  if (route.kind === 'share') return <RouteFallback><SharePage token={route.token} /></RouteFallback>
  if (!user) {
    // Convidados com link de sala vão direto ao login; a raiz mostra a landing.
    if (route.kind === 'room' || location.hash.startsWith('#/login')) {
      return <Login onLogin={setUser} />
    }
    return <Landing onSignIn={() => { location.hash = '/login'; setRoute(parseHash()) }} />
  }

  const nav: NavKey =
    route.kind === 'directory'
      ? 'directory'
      : route.kind === 'recordings'
        ? 'recordings'
        : route.kind === 'whiteboards'
          ? 'whiteboards'
          : route.kind === 'studio'
            ? 'studio'
          : route.kind === 'calendar'
            ? 'calendar'
          : route.kind === 'analytics'
            ? 'analytics'
            : route.kind === 'roadmap'
              ? 'roadmap'
              : 'home'

  // A presença vive acima do router: as chamadas tocam em qualquer página,
  // incluindo dentro de uma sala.
  // HTTP fora de localhost NÃO é contexto seguro → o browser bloqueia câmara,
  // microfone e WebRTC. Avisar de forma clara em vez de falhar em silêncio.
  const insecure = typeof window !== 'undefined' && !window.isSecureContext
  return (
    <>
    {insecure && (
      <div className="insecure-banner">
        <AlertIcon /> Ligação <strong>insegura (HTTP)</strong> — câmara, microfone e chamadas NÃO funcionam.
        Abre em <strong>https://{location.hostname}</strong> (aceita o aviso do certificado).
      </div>
    )}
    <PresenceProvider onEnterRoom={enterRoom}>
      {route.kind === 'lobby' ? (
        <RouteFallback><Lobby code={route.code} /></RouteFallback>
      ) : route.kind === 'room' ? (
        <RouteFallback>
          <Room code={route.code} voiceOnly={route.voice} onLeave={leaveRoom} onSwitch={(c) => enterRoom(c)} />
        </RouteFallback>
      ) : (
        <Shell
          user={user}
          active={nav}
          onNavigate={navigate}
          onEnterRoom={enterRoom}
          onLogout={() => {
            logout()
            setUser(null)
            location.hash = '/'
          }}
        >
          <RouteFallback>
            {route.kind === 'home' && <Home user={user} onEnterRoom={enterRoom} onNavigate={navigate} />}
            {route.kind === 'directory' && <Directory />}
            {route.kind === 'recordings' && <Recordings />}
            {route.kind === 'whiteboards' && <Whiteboards />}
            {route.kind === 'studio' && <Studio />}
            {route.kind === 'calendar' && <Calendar onEnterRoom={enterRoom} />}
            {route.kind === 'analytics' && <Analytics />}
            {route.kind === 'roadmap' && <Roadmap />}
          </RouteFallback>
        </Shell>
      )}
    </PresenceProvider>
    </>
  )
}
