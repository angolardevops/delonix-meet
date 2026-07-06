import { useEffect, useState } from 'react'
import { currentUser, logout, User } from './api'
import Login from './pages/Login'
import Home from './pages/Home'
import Recordings from './pages/Recordings'
import Calendar from './pages/Calendar'
import Directory from './pages/Directory'
import Analytics from './pages/Analytics'
import Room from './pages/Room'
import Shell, { NavKey } from './components/Shell'
import PresenceProvider from './components/PresenceProvider'

type Route =
  | { kind: 'home' }
  | { kind: 'directory' }
  | { kind: 'recordings' }
  | { kind: 'calendar' }
  | { kind: 'analytics' }
  | { kind: 'room'; code: string; voice: boolean }

function parseHash(): Route {
  const h = location.hash
  const roomVoice = h.match(/^#\/r\/([a-z-]+)\?voice$/)
  if (roomVoice) return { kind: 'room', code: roomVoice[1], voice: true }
  const room = h.match(/^#\/r\/([a-z-]+)$/)
  if (room) return { kind: 'room', code: room[1], voice: false }
  if (h.startsWith('#/directory')) return { kind: 'directory' }
  if (h.startsWith('#/recordings')) return { kind: 'recordings' }
  if (h.startsWith('#/calendar')) return { kind: 'calendar' }
  if (h.startsWith('#/analytics')) return { kind: 'analytics' }
  return { kind: 'home' }
}

export default function App() {
  const [user, setUser] = useState<User | null>(currentUser())
  const [route, setRoute] = useState<Route>(parseHash())

  useEffect(() => {
    const onHash = () => setRoute(parseHash())
    window.addEventListener('hashchange', onHash)
    return () => window.removeEventListener('hashchange', onHash)
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

  if (!user) return <Login onLogin={setUser} />

  const nav: NavKey =
    route.kind === 'directory'
      ? 'directory'
      : route.kind === 'recordings'
        ? 'recordings'
        : route.kind === 'calendar'
          ? 'calendar'
          : route.kind === 'analytics'
            ? 'analytics'
            : 'home'

  // A presença vive acima do router: as chamadas tocam em qualquer página,
  // incluindo dentro de uma sala.
  return (
    <PresenceProvider onEnterRoom={enterRoom}>
      {route.kind === 'room' ? (
        <Room code={route.code} voiceOnly={route.voice} onLeave={leaveRoom} onSwitch={(c) => enterRoom(c)} />
      ) : (
        <Shell
          user={user}
          active={nav}
          onNavigate={navigate}
          onLogout={() => {
            logout()
            setUser(null)
            location.hash = '/'
          }}
        >
          {route.kind === 'home' && <Home user={user} onEnterRoom={enterRoom} />}
          {route.kind === 'directory' && <Directory />}
          {route.kind === 'recordings' && <Recordings />}
          {route.kind === 'calendar' && <Calendar onEnterRoom={enterRoom} />}
          {route.kind === 'analytics' && <Analytics />}
        </Shell>
      )}
    </PresenceProvider>
  )
}
