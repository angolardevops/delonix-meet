import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { currentUser, logout, User } from './api'
import Shell from './Shell'
import { Empty } from './ui/kit'
import { NavKey, parseHash, type Route } from './rota'
import Login from './pages/Login'
import Overview from './pages/Overview'
import Tenants from './pages/Tenants'
import Integrations from './pages/Integrations'
import Security from './pages/Security'
import Communications from './pages/Communications'

export default function App() {
  const { t } = useTranslation()
  const [user, setUser] = useState<User | null>(currentUser())
  const [route, setRoute] = useState<Route>(parseHash())

  useEffect(() => {
    const onHash = () => setRoute(parseHash())
    // Sessão expirada (a renovação falhou): volta-se ao ecrã de entrada.
    const onExpired = () => setUser(null)
    window.addEventListener('hashchange', onHash)
    window.addEventListener('dxa-auth-expired', onExpired)
    return () => {
      window.removeEventListener('hashchange', onHash)
      window.removeEventListener('dxa-auth-expired', onExpired)
    }
  }, [])

  function navigate(key: NavKey) {
    location.hash = `/${key}`
  }

  if (!user) {
    return (
      <Login
        onLogin={(u) => {
          setUser(u)
          if (route.kind === 'desconhecida') location.hash = '/overview'
        }}
      />
    )
  }

  const active: NavKey = route.kind === 'desconhecida' ? 'overview' : route.kind

  return (
    <Shell
      user={user}
      active={active}
      onNavigate={navigate}
      onLogout={() => {
        logout()
        setUser(null)
      }}
    >
      {route.kind === 'overview' && <Overview />}
      {route.kind === 'tenants' && <Tenants />}
      {route.kind === 'integrations' && <Integrations />}
      {route.kind === 'security' && <Security />}
      {route.kind === 'communications' && <Communications />}
      {route.kind === 'desconhecida' && (
        <div className="page">
          <Empty
            icon="search"
            title={t('ui.rotaDesconhecida.titulo')}
            action={
              <a className="dx-btn dx-btn--primary" href="#/overview">
                {t('ui.rotaDesconhecida.inicio')}
              </a>
            }
          >
            <p>{t('ui.rotaDesconhecida.texto')}</p>
            <code>{route.endereco}</code>
          </Empty>
        </div>
      )}
    </Shell>
  )
}
