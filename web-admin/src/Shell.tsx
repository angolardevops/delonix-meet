/**
 * A casca do backoffice: barra lateral com as cinco secções + barra de
 * topo com o título da secção activa. Muito mais pequena que o `Shell` da
 * consola de tenant (`web/src/components/Shell.tsx`) — sem palete de
 * comandos, sem definições, sem presença, sem recolher o rail: só a
 * gaveta em ecrã estreito, que o `shell.css` (copiado tal como está) já
 * sabe desenhar.
 */
import { ReactNode, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { User } from './api'
import { NAV_I18N, NavKey, PAGES } from './rota'
import { Icon, IconName } from './ui/icons'
import { Avatar, cx } from './ui/kit'

const SECTION_ICON: Record<NavKey, IconName> = {
  overview: 'server',
  tenants: 'building',
  integrations: 'database',
  security: 'shield',
  communications: 'phone',
}

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
  const [navOpen, setNavOpen] = useState(false)

  function go(k: NavKey) {
    setNavOpen(false)
    onNavigate(k)
  }

  return (
    <div className={cx('shell', navOpen && 'nav-open')}>
      <button type="button" className="skip-link" onClick={() => document.getElementById('conteudo')?.focus()}>
        {t('shell.saltarParaConteudo')}
      </button>
      <nav id="shell-nav" className="shell-nav" aria-label={t('shell.marca')}>
        <div className="shell-nav__brand">
          <span className="shell-nav__lockup">
            <strong style={{ fontFamily: 'var(--font-display)', fontSize: 14 }}>Delonix Meet</strong>
            <span className="dx-muted" style={{ fontSize: 10, marginLeft: 6 }}>
              {t('shell.marca')}
            </span>
          </span>
        </div>
        <div className="shell-nav__scroll">
          <ul className="nav-list">
            {PAGES.map((k) => (
              <li key={k}>
                <button
                  type="button"
                  className={cx('nav-item', k === active && 'nav-item--active')}
                  aria-current={k === active ? 'page' : undefined}
                  onClick={() => go(k)}
                >
                  <Icon name={SECTION_ICON[k]} />
                  <span className="nav-item__label">{t(NAV_I18N[k])}</span>
                </button>
              </li>
            ))}
          </ul>
        </div>
        <div className="shell-nav__foot">
          <div className="shell-user">
            <Avatar name={user.username || user.email} size={30} />
            <span className="shell-user__id">
              <strong>{user.username}</strong>
              <span className="dx-num">{user.email}</span>
            </span>
          </div>
          <div className="shell-nav__tools">
            <button
              type="button"
              className="dx-iconbtn dx-iconbtn--bare"
              onClick={onLogout}
              aria-label={t('shell.terminarSessao')}
              title={t('shell.terminarSessao')}
            >
              <Icon name="logout" />
            </button>
          </div>
        </div>
      </nav>
      {navOpen && <div className="shell-nav-backdrop" onClick={() => setNavOpen(false)} aria-hidden="true" />}
      <div className="shell-main">
        <header className="page-bar">
          <button
            type="button"
            className="dx-iconbtn page-bar__burger"
            aria-label={t('shell.abrirNavegacao')}
            aria-expanded={navOpen}
            aria-controls="shell-nav"
            onClick={() => setNavOpen((o) => !o)}
          >
            <Icon name="menu" />
          </button>
          <h1 className="page-bar__title">{t(NAV_I18N[active])}</h1>
        </header>
        <main id="conteudo" className="shell-body" tabIndex={-1}>
          {children}
        </main>
      </div>
    </div>
  )
}
