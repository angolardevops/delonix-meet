import { lazy, ReactNode, Suspense, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { completeSsoLogin, currentUser, logout, User } from './api'
import Shell, { NavKey } from './components/Shell'
import PresenceProvider from './components/PresenceProvider'
import { Icon } from './ui/icons'
import { Spinner } from './ui/kit'

// ---------------------------------------------------------------------------
//  Corte por rota. EAGER ficam só os dois ecrãs de entrada — Entrar e Início
//  — que são o primeiro pixel; tudo o resto chega por `lazy`. A sala (e com
//  ela webrtc/media/e2ee) nunca entra no chunk da consola.
// ---------------------------------------------------------------------------
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
const Integrations = lazy(() => import('./pages/Integrations'))
const Admin = lazy(() => import('./pages/Admin'))
const Intelligence = lazy(() => import('./pages/Intelligence'))
const Status = lazy(() => import('./pages/Status'))
const ApiDocs = lazy(() => import('./pages/ApiDocs'))
const Legal = lazy(() => import('./pages/Legal'))
const SharePage = lazy(() => import('./pages/SharePage'))

/**
 * Espera de rota. Deliberadamente MUDA: o chunk chega em dezenas de
 * milissegundos e um spinner que pisca nesse intervalo lê-se como avaria.
 */
function RouteFallback({ children }: { children: ReactNode }) {
  return <Suspense fallback={<div className="dx-route-wait" aria-hidden="true" />}>{children}</Suspense>
}

type Route =
  | { kind: NavKey }
  | { kind: 'room'; code: string; voice: boolean }
  | { kind: 'lobby'; code: string }
  | { kind: 'share'; token: string }

const PAGES: NavKey[] = ['calendar', 'studio', 'recordings', 'whiteboards', 'directory', 'integrations', 'analytics', 'admin', 'ai']

function parseHash(): Route {
  const h = location.hash
  const room = h.match(/^#\/r\/([a-z-]+)(\?voice)?$/)
  if (room) return { kind: 'room', code: room[1], voice: !!room[2] }
  const lobby = h.match(/^#\/lobby\/([a-z-]+)$/)
  if (lobby) return { kind: 'lobby', code: lobby[1] }
  const share = h.match(/^#\/share\/([a-f0-9]+)$/)
  if (share) return { kind: 'share', token: share[1] }
  for (const p of PAGES) if (h.startsWith(`#/${p}`)) return { kind: p }
  return { kind: 'home' }
}

export default function App() {
  const { t } = useTranslation()
  const [user, setUser] = useState<User | null>(currentUser())
  const [route, setRoute] = useState<Route>(parseHash())

  useEffect(() => {
    const onHash = () => setRoute(parseHash())
    // Sessão expirada (a renovação falhou): volta-se ao ecrã de entrada.
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

  // Regresso do IdP: `#/sso-complete?token=…`.
  useEffect(() => {
    if (location.hash.startsWith('#/sso-complete')) {
      void completeSsoLogin().then((u) => {
        if (u) setUser(u)
        else location.hash = '/login'
      })
    }
  }, [])

  function enterRoom(code: string, voice = false) {
    location.hash = `/r/${code}${voice ? '?voice' : ''}`
  }
  function navigate(key: NavKey) {
    location.hash = key === 'home' ? '/' : `/${key}`
  }

  if (location.hash.startsWith('#/sso-complete')) {
    return (
      <div className="wait-screen" role="status">
        <Spinner />
        <span>{t('auth.aCompletarSso')}</span>
      </div>
    )
  }
  if (location.hash.startsWith('#/status')) return <RouteFallback><Status /></RouteFallback>
  if (location.hash.startsWith('#/api-docs')) return <RouteFallback><ApiDocs /></RouteFallback>
  if (location.hash.startsWith('#/legal')) return <RouteFallback><Legal /></RouteFallback>
  if (route.kind === 'share') return <RouteFallback><SharePage token={route.token} /></RouteFallback>

  if (!user) {
    return (
      <Login
        pendingRoom={route.kind === 'room' || route.kind === 'lobby' ? route.code : null}
        onLogin={(u) => {
          setUser(u)
          if (location.hash.startsWith('#/login')) location.hash = '/'
        }}
      />
    )
  }

  // HTTP fora de localhost não é contexto seguro: o browser bloqueia câmara,
  // microfone e WebRTC. Diz-se claramente em vez de falhar em silêncio.
  const insecure = !window.isSecureContext

  return (
    <>
      {insecure && (
        <div className="insecure-banner" role="alert">
          <Icon name="alert" />
          <span>{t('shell.inseguro', { host: location.hostname })}</span>
        </div>
      )}
      <PresenceProvider onEnterRoom={enterRoom}>
        {route.kind === 'lobby' ? (
          <RouteFallback>
            <Lobby code={route.code} />
          </RouteFallback>
        ) : route.kind === 'room' ? (
          <RouteFallback>
            <Room
              key={route.code}
              code={route.code}
              voiceOnly={route.voice}
              onLeave={() => (location.hash = '/')}
              onSwitch={(c) => enterRoom(c)}
            />
          </RouteFallback>
        ) : (
          <Shell
            user={user}
            active={route.kind}
            onNavigate={navigate}
            onEnterRoom={enterRoom}
            onLogout={() => {
              logout()
              setUser(null)
              location.hash = '/'
            }}
          >
            <RouteFallback>
              {route.kind === 'home' && <Home />}
              {route.kind === 'calendar' && <Calendar />}
              {route.kind === 'studio' && <Studio />}
              {route.kind === 'recordings' && <Recordings />}
              {route.kind === 'whiteboards' && <Whiteboards />}
              {route.kind === 'directory' && <Directory />}
              {route.kind === 'integrations' && <Integrations />}
              {route.kind === 'analytics' && <Analytics />}
              {route.kind === 'admin' && <Admin />}
              {route.kind === 'ai' && <Intelligence />}
            </RouteFallback>
          </Shell>
        )}
      </PresenceProvider>
    </>
  )
}
