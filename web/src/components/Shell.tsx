/**
 * A consola: rail de navegação (228 px, o `DelonixNav` do template) + área
 * de conteúdo. Cada página desenha a sua própria barra (`PageBar`) — no
 * template a barra muda de ecrã para ecrã (saudação na Início, «Cancelar ·
 * Guardar» na Agenda, pesquisa + vista nas Gravações).
 *
 * Abaixo de 900 px o rail sai do ecrã e vira gaveta: estado próprio, fundo
 * que fecha, Esc que fecha, e escolher um destino fecha (lote2 3.1.1). O
 * estado da gaveta NÃO se persiste — abrir a app com a gaveta aberta tapava
 * o conteúdo.
 *
 * Acima de 900 px o rail recolhe para só ícones (64 px) pelo botão do topo,
 * pelo da PageBar ou por Ctrl/Cmd+B. Esse estado SIM persiste, por
 * utilizador neste browser — é uma preferência de espaço, não um overlay.
 */
import { ReactNode, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { isAbort, myOrgs, OrgSummary, User } from '../api'
import { Icon, IconName } from '../ui/icons'
import { Avatar, cx } from '../ui/kit'
import { applyTheme, storedTheme } from '../theme'
import { Async } from './AsyncSection'
import { BrandLockup } from './BrandMark'
import CommandPalette from './CommandPalette'
import { usePresence } from './PresenceProvider'
import SettingsDialog, { SettingsTab } from './SettingsDialog'
import { NavKey, ShellApi, ShellCtx } from './shellContext'

export type { NavKey } from './shellContext'

/** Abaixo disto o rail é gaveta — o mesmo limiar do `@media` em shell.css. */
const NARROW_QUERY = '(max-width: 900px)'

const collapsedKey = (userId: string | number) => `dx_nav_collapsed:${userId}`

function readCollapsed(userId: string | number): boolean {
  try {
    return localStorage.getItem(collapsedKey(userId)) === '1'
  } catch {
    return false
  }
}

function writeCollapsed(userId: string | number, v: boolean) {
  try {
    localStorage.setItem(collapsedKey(userId), v ? '1' : '0')
  } catch {
    /* sem armazenamento: vale para esta sessão */
  }
}

function isNarrow(): boolean {
  return typeof window !== 'undefined' && !!window.matchMedia?.(NARROW_QUERY).matches
}

/** Atalhos de teclado não roubam teclas a quem está a escrever. */
function isEditable(t: EventTarget | null): boolean {
  const el = t as HTMLElement | null
  return !!el && (el.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName))
}

interface NavItem {
  key: NavKey
  label: string
  icon: IconName
}

export default function Shell({
  user,
  active,
  onNavigate,
  onEnterRoom,
  onLogout,
  children,
}: {
  user: User
  active: NavKey
  onNavigate: (k: NavKey) => void
  onEnterRoom: (code: string, voice?: boolean) => void
  onLogout: () => void
  children: ReactNode
}) {
  const { t } = useTranslation()
  const presence = usePresence()
  const [navOpen, setNavOpen] = useState(false)
  const [navCollapsed, setNavCollapsedState] = useState(() => readCollapsed(user.id))
  const [narrow, setNarrow] = useState(isNarrow)
  const [paletteOpen, setPaletteOpen] = useState(false)
  const [settings, setSettings] = useState<SettingsTab | null>(null)
  const [orgs, setOrgs] = useState<Async<OrgSummary[]>>({ s: 'loading' })
  const [theme, setTheme] = useState(storedTheme())
  const [orgsNonce, setOrgsNonce] = useState(0)

  useEffect(() => {
    const ctrl = new AbortController()
    myOrgs(ctrl.signal)
      .then((d) => setOrgs({ s: 'ready', d }))
      .catch((e) => {
        if (isAbort(e)) return
        setOrgs({ s: 'error', msg: e instanceof Error ? e.message : t('ui.erroCarregar') })
      })
    return () => ctrl.abort()
  }, [t, orgsNonce])

  useEffect(() => {
    const mq = window.matchMedia?.(NARROW_QUERY)
    if (!mq) return
    const onChange = () => {
      setNarrow(mq.matches)
      // Passar a largo com a gaveta aberta deixava o backdrop por cima de tudo.
      if (!mq.matches) setNavOpen(false)
    }
    mq.addEventListener('change', onChange)
    return () => mq.removeEventListener('change', onChange)
  }, [])

  const setNavCollapsed = useCallback(
    (v: boolean) => {
      setNavCollapsedState(v)
      writeCollapsed(user.id, v)
    },
    [user.id],
  )

  /** O hambúrguer: gaveta em ecrã estreito, recolher/expandir em ecrã largo. */
  const toggleNav = useCallback(() => {
    if (isNarrow()) setNavOpen((o) => !o)
    else setNavCollapsed(!navCollapsed)
  }, [navCollapsed, setNavCollapsed])
  const toggleNavRef = useRef(toggleNav)
  toggleNavRef.current = toggleNav

  // Esc fecha a gaveta; Ctrl/Cmd+K abre a paleta em qualquer ecrã da consola;
  // Ctrl/Cmd+B recolhe/expande o rail (fora de campos de texto).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && navOpen) setNavOpen(false)
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault()
        setPaletteOpen((o) => !o)
      }
      if ((e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey && e.key.toLowerCase() === 'b' && !isEditable(e.target)) {
        e.preventDefault()
        toggleNavRef.current()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [navOpen])

  function go(k: NavKey) {
    setNavOpen(false)
    onNavigate(k)
  }

  const org = useMemo(() => {
    if (orgs.s !== 'ready' || orgs.d.length === 0) return null
    return orgs.d.find((o) => o.role === 'admin') ?? orgs.d[0]
  }, [orgs])
  const isAdmin = org?.role === 'admin'

  const primary: NavItem[] = [
    { key: 'home', label: t('shell.nav.inicio'), icon: 'home' },
    { key: 'calendar', label: t('shell.nav.agenda'), icon: 'calendar' },
    { key: 'studio', label: t('shell.nav.estudio'), icon: 'live' },
    { key: 'recordings', label: t('shell.nav.gravacoes'), icon: 'film' },
    { key: 'whiteboards', label: t('shell.nav.quadros'), icon: 'board' },
    { key: 'directory', label: t('shell.nav.contactos'), icon: 'people' },
  ]
  const management: NavItem[] = [
    { key: 'integrations', label: t('shell.nav.integracoes'), icon: 'plug' },
    { key: 'analytics', label: t('shell.nav.analise'), icon: 'chart' },
    { key: 'admin', label: t('shell.nav.administracao'), icon: 'building' },
    { key: 'ai', label: t('consola.nav.ia'), icon: 'sparkles' },
  ]

  const openSettings = useCallback((tab: SettingsTab = 'account') => setSettings(tab), [])
  const api: ShellApi = {
    user,
    orgs,
    org,
    isAdmin,
    reloadOrgs: () => setOrgsNonce((n) => n + 1),
    navOpen,
    setNavOpen,
    navCollapsed: navCollapsed && !narrow,
    navExpanded: narrow ? navOpen : !navCollapsed,
    toggleNav,
    navigate: go,
    enterRoom: onEnterRoom,
    openPalette: () => setPaletteOpen(true),
    openSettings,
  }

  function toggleTheme() {
    const next = theme === 'dark' ? 'light' : 'dark'
    applyTheme(next)
    setTheme(next)
  }

  const collapsed = navCollapsed && !narrow
  const railExpanded = narrow ? navOpen : !navCollapsed
  const railToggleLabel = narrow ? t('shell.fecharNavegacao') : navCollapsed ? t('shell.expandirMenu') : t('shell.recolherMenu')

  const renderItem = (n: NavItem) => (
    <li key={n.key}>
      <button
        type="button"
        className={cx('nav-item', n.key === active && 'nav-item--active')}
        aria-current={n.key === active ? 'page' : undefined}
        onClick={() => go(n.key)}
        title={collapsed ? n.label : undefined}
      >
        <Icon name={n.icon} />
        <span className="nav-item__label">{n.label}</span>
      </button>
    </li>
  )

  return (
    <ShellCtx.Provider value={api}>
      <div className={cx('shell', navOpen && 'nav-open', collapsed && 'nav-collapsed')}>
        <a className="skip-link" href="#conteudo">
          {t('shell.saltarParaConteudo')}
        </a>
        <nav id="shell-nav" className="shell-nav" aria-label={t('shell.nav.rotulo')}>
          <div className="shell-nav__brand">
            <button
              type="button"
              className="dx-iconbtn dx-iconbtn--bare shell-nav__toggle"
              aria-label={railToggleLabel}
              title={`${railToggleLabel} (${t('shell.atalhoMenu')})`}
              aria-expanded={railExpanded}
              aria-controls="shell-nav"
              onClick={() => (narrow ? setNavOpen(false) : setNavCollapsed(!navCollapsed))}
            >
              <Icon name="menu" />
            </button>
            <span className="shell-nav__lockup">
              <BrandLockup size={24} />
            </span>
          </div>
          <button
            type="button"
            className="shell-nav__search"
            onClick={() => setPaletteOpen(true)}
            title={collapsed ? t('shell.procurar') : undefined}
          >
            <Icon name="search" />
            <span className="nav-item__label">{t('shell.procurar')}</span>
            <kbd className="dx-num">{t('shell.atalhoPaleta')}</kbd>
          </button>
          <div className="shell-nav__scroll">
            <ul className="nav-list">{primary.map(renderItem)}</ul>
            {isAdmin && (
              <>
                <div className="nav-section dx-eyebrow">
                  <span className="nav-item__label">{t('shell.nav.gestao')}</span>
                </div>
                <ul className="nav-list">{management.map(renderItem)}</ul>
              </>
            )}
          </div>
          {presence.missed.length > 0 && (
            <div className="shell-nav__missed" role="status" title={collapsed ? t('shell.chamada.perdidas', { count: presence.missed.length }) : undefined}>
              <Icon name="phone" />
              <span style={{ flex: 1, minWidth: 0 }}>
                {t('shell.chamada.perdidas', { count: presence.missed.length })}
              </span>
              <button type="button" className="dx-btn dx-btn--ghost dx-btn--sm" onClick={() => presence.callBack(presence.missed[0])}>
                {t('shell.chamada.devolver')}
              </button>
              <button type="button" className="dx-iconbtn dx-iconbtn--bare" aria-label={t('ui.dispensar')} onClick={presence.ackMissed}>
                <Icon name="x" />
              </button>
            </div>
          )}
          <div className="shell-nav__foot">
            <button
              type="button"
              className="shell-user"
              onClick={() => openSettings('account')}
              title={collapsed ? user.username : undefined}
              aria-label={collapsed ? `${user.username} · ${t('shell.definicoes')}` : undefined}
            >
              <Avatar name={user.username || user.email} size={30} />
              <span className="shell-user__id">
                <strong>{user.username}</strong>
                <span className="dx-num">{org ? org.name : user.email}</span>
              </span>
            </button>
            <div className="shell-nav__tools">
              <button
                type="button"
                className="dx-iconbtn dx-iconbtn--bare"
                onClick={toggleTheme}
                aria-label={theme === 'dark' ? t('shell.temaClaro') : t('shell.temaEscuro')}
                title={theme === 'dark' ? t('shell.temaClaro') : t('shell.temaEscuro')}
              >
                <Icon name={theme === 'dark' ? 'sun' : 'moon'} />
              </button>
              <button
                type="button"
                className="dx-iconbtn dx-iconbtn--bare"
                onClick={() => openSettings('account')}
                aria-label={t('shell.definicoes')}
                title={t('shell.definicoes')}
              >
                <Icon name="sliders" />
              </button>
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
          <main id="conteudo" className="shell-body" tabIndex={-1}>
            {children}
          </main>
        </div>
      </div>
      {paletteOpen && (
        <CommandPalette
          onClose={() => setPaletteOpen(false)}
          onNavigate={go}
          onEnterRoom={onEnterRoom}
          onLogout={onLogout}
          onSettings={() => openSettings('account')}
          onToggleTheme={toggleTheme}
          user={user}
          isAdmin={isAdmin}
        />
      )}
      {settings && (
        <SettingsDialog
          user={user}
          initialTab={settings}
          onClose={() => setSettings(null)}
          onThemeChange={setTheme}
        />
      )}
    </ShellCtx.Provider>
  )
}
