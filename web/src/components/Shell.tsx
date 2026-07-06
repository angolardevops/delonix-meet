import { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { User } from '../api'
import { setLanguage } from '../i18n'
import { CalendarIcon, ClockIcon, FilmIcon, HomeIcon, PeopleIcon } from '../icons'

export type NavKey = 'home' | 'directory' | 'recordings' | 'calendar' | 'analytics'

const NAV: { key: NavKey; labelKey: string; icon: ReactNode }[] = [
  { key: 'home', labelKey: 'nav.home', icon: <HomeIcon /> },
  { key: 'directory', labelKey: 'nav.org', icon: <PeopleIcon /> },
  { key: 'calendar', labelKey: 'nav.calendar', icon: <CalendarIcon /> },
  { key: 'recordings', labelKey: 'nav.recordings', icon: <FilmIcon /> },
  { key: 'analytics', labelKey: 'nav.analytics', icon: <ClockIcon /> },
]

/** Toggle global PT/EN — persiste e re-renderiza toda a app. */
export function LanguageToggle() {
  const { i18n } = useTranslation()
  const lang = i18n.language.startsWith('en') ? 'en' : 'pt'
  return (
    <div className="lang-toggle" role="group" aria-label="Idioma / Language">
      <button className={lang === 'pt' ? 'lang-btn active' : 'lang-btn'} onClick={() => setLanguage('pt')}>
        PT
      </button>
      <button className={lang === 'en' ? 'lang-btn active' : 'lang-btn'} onClick={() => setLanguage('en')}>
        EN
      </button>
    </div>
  )
}

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
  const { t } = useTranslation()
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
              <span>{t(n.labelKey)}</span>
            </button>
          ))}
        </nav>
        <div className="shell-nav-foot">
          <LanguageToggle />
          <div className="nav-user">
            <span className="avatar-circle small">{initials}</span>
            <span className="nav-user-name">{user.username}</span>
          </div>
          <button className="link" onClick={onLogout}>
            {t('nav.logout')}
          </button>
        </div>
      </aside>
      <main className="shell-main">{children}</main>
    </div>
  )
}
