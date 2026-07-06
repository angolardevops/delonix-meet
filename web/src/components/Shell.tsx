import { ReactNode } from 'react'
import { User } from '../api'
import { CalendarIcon, ClockIcon, FilmIcon, HomeIcon, PeopleIcon } from '../icons'

export type NavKey = 'home' | 'directory' | 'recordings' | 'calendar' | 'analytics'

const NAV: { key: NavKey; label: string; icon: ReactNode }[] = [
  { key: 'home', label: 'Início', icon: <HomeIcon /> },
  { key: 'directory', label: 'Organização', icon: <PeopleIcon /> },
  { key: 'calendar', label: 'Calendário', icon: <CalendarIcon /> },
  { key: 'recordings', label: 'Gravações', icon: <FilmIcon /> },
  { key: 'analytics', label: 'Análises', icon: <ClockIcon /> },
]

/** Layout com barra lateral de navegação e topo — partilhado por Home/Gravações/Calendário. */
export default function Shell({
  user,
  active,
  onNavigate,
  onLogout,
  children,
}: {
  user: User
  active: NavKey
  onNavigate: (k: NavKey) => void
  onLogout: () => void
  children: ReactNode
}) {
  const initials = user.username.slice(0, 2).toUpperCase()
  return (
    <div className="shell">
      <aside className="shell-nav">
        <div className="shell-brand">
          <span className="brand-mark">◆</span>
          <span className="brand-text">
            Delonix <span>Meet</span>
          </span>
        </div>
        <nav>
          {NAV.map((n) => (
            <button
              key={n.key}
              className={active === n.key ? 'nav-item active' : 'nav-item'}
              onClick={() => onNavigate(n.key)}
            >
              {n.icon}
              <span>{n.label}</span>
            </button>
          ))}
        </nav>
        <div className="shell-nav-foot">
          <div className="nav-user">
            <span className="avatar-circle small">{initials}</span>
            <span className="nav-user-name">{user.username}</span>
          </div>
          <button className="link" onClick={onLogout}>
            Terminar sessão
          </button>
        </div>
      </aside>
      <main className="shell-main">{children}</main>
    </div>
  )
}
