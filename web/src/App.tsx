import { useEffect, useState } from 'react'
import { completeSsoLogin, currentUser, logout, User } from './api'
import Login from './pages/Login'
import Landing from './pages/Landing'
import Home from './pages/Home'
import Recordings from './pages/Recordings'
import Whiteboards from './pages/Whiteboards'
import Calendar from './pages/Calendar'
import Directory from './pages/Directory'
import Analytics from './pages/Analytics'
import Roadmap from './pages/Roadmap'
import Status from './pages/Status'
import ApiDocs from './pages/ApiDocs'
import Legal from './pages/Legal'
import Room from './pages/Room'
import Lobby from './pages/Lobby'
import SharePage from './pages/SharePage'
import Shell, { NavKey } from './components/Shell'
import PresenceProvider from './components/PresenceProvider'

type Route =
  | { kind: 'home' }
  | { kind: 'directory' }
  | { kind: 'recordings' }
  | { kind: 'whiteboards' }
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
  if (h.startsWith('#/calendar')) return { kind: 'calendar' }
  if (h.startsWith('#/analytics')) return { kind: 'analytics' }
  if (h.startsWith('#/roadmap')) return { kind: 'roadmap' }
  return { kind: 'home' }
}

export default function App() {
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
    return <div className="auth-page"><div className="auth-card"><p>A completar o login SSO…</p></div></div>
  }
  if (location.hash.startsWith('#/status')) return <Status />
  if (location.hash.startsWith('#/api-docs')) return <ApiDocs />
  if (location.hash.startsWith('#/legal')) return <Legal />
  // Link público de gravação — sem autenticação necessária.
  if (route.kind === 'share') return <SharePage token={route.token} />
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
        ⚠️ Ligação <strong>insegura (HTTP)</strong> — câmara, microfone e chamadas NÃO funcionam.
        Abre em <strong>https://{location.hostname}</strong> (aceita o aviso do certificado).
      </div>
    )}
    <PresenceProvider onEnterRoom={enterRoom}>
      {route.kind === 'lobby' ? (
        <Lobby code={route.code} />
      ) : route.kind === 'room' ? (
        <Room code={route.code} voiceOnly={route.voice} onLeave={leaveRoom} onSwitch={(c) => enterRoom(c)} />
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
          {route.kind === 'home' && <Home user={user} onEnterRoom={enterRoom} onNavigate={navigate} />}
          {route.kind === 'directory' && <Directory />}
          {route.kind === 'recordings' && <Recordings />}
          {route.kind === 'whiteboards' && <Whiteboards />}
          {route.kind === 'calendar' && <Calendar onEnterRoom={enterRoom} />}
          {route.kind === 'analytics' && <Analytics />}
          {route.kind === 'roadmap' && <Roadmap />}
        </Shell>
      )}
    </PresenceProvider>
    </>
  )
}
