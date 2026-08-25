import { FormEvent, ReactNode, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { createRoom, getRoom, isAbort, myOrgs, updateMe, User } from '../api'
import CommandPalette from './CommandPalette'
import NotificationCenter from './NotificationCenter'
import ThemePicker from './ThemePicker'
import LanguageToggle from './LanguageToggle'
import { applyTheme, storedTheme, Theme } from '../theme'
import { appNameParts, getAppName, getLoginBg, setAppName, setLoginBg } from '../branding'
import PasswordInput from './PasswordInput'
import MfaPanel from './MfaPanel'
import OnboardingTour from './OnboardingTour'
import { CalendarIcon, ChevronDownIcon, ClockIcon, CloseIcon, FilmIcon, HomeIcon, MenuIcon, NoteIcon, PeopleIcon, SearchIcon, SettingsIcon, StageIcon, ThemeIcon } from '../icons'

export type NavKey = 'home' | 'directory' | 'recordings' | 'calendar' | 'analytics' | 'roadmap' | 'whiteboards'

type NavItem = { key: NavKey; labelKey: string; icon: ReactNode }
type NavSection = { titleKey: string; items: NavItem[] }

// Navegação agrupada por secção (padrão enterprise: Teams/Slack) — comunica
// hierarquia e escala melhor que uma lista plana. Ver docs/ux-review.md.
const NAV_SECTIONS: NavSection[] = [
  {
    titleKey: 'navSection.work',
    items: [
      { key: 'home', labelKey: 'nav.home', icon: <HomeIcon /> },
      { key: 'calendar', labelKey: 'nav.calendar', icon: <CalendarIcon /> },
    ],
  },
  {
    titleKey: 'navSection.library',
    items: [
      { key: 'recordings', labelKey: 'nav.recordings', icon: <FilmIcon /> },
      { key: 'whiteboards', labelKey: 'nav.whiteboards', icon: <NoteIcon /> },
    ],
  },
  {
    titleKey: 'navSection.org',
    items: [{ key: 'directory', labelKey: 'nav.org', icon: <PeopleIcon /> }],
  },
  {
    titleKey: 'navSection.admin',
    items: [
      { key: 'analytics', labelKey: 'nav.analytics', icon: <ClockIcon /> },
      // Roadmap só visível em desenvolvimento local (nunca em stage/prod)
      ...(import.meta.env.DEV ? [{ key: 'roadmap' as NavKey, labelKey: 'road.navLabel', icon: <StageIcon /> }] : []),
    ],
  },
]

type SettingsTab = 'appearance' | 'account' | 'security' | 'brand'

/** Painel de definições em gaveta à direita, com abas por tipo (estilo consola). */
export function SettingsModal({ user, onClose, onLogout }: { user: User; onClose: () => void; onLogout: () => void }) {
  const { t } = useTranslation()
  const [tab, setTab] = useState<SettingsTab>('appearance')
  // Conta
  const [username, setUsername] = useState(user.username)
  const [password, setPassword] = useState('')
  const [saving, setSaving] = useState(false)
  const [msg, setMsg] = useState('')
  const dirty = username.trim() !== user.username || password.length > 0
  // Marca
  const [appName, setAppNameState] = useState(getAppName())
  const [bg, setBg] = useState<string | null>(getLoginBg())

  async function saveAccount() {
    setMsg('')
    setSaving(true)
    try {
      const patch: { username?: string; password?: string } = {}
      if (username.trim() && username.trim() !== user.username) patch.username = username.trim()
      if (password) patch.password = password
      await updateMe(patch)
      setPassword('')
      setMsg('✓ Guardado')
      setTimeout(() => setMsg(''), 3000)
    } catch (e) {
      setMsg((e as Error).message || 'Falha ao guardar')
    } finally {
      setSaving(false)
    }
  }

  function onPickBg(file: File | null) {
    if (!file) return
    const reader = new FileReader()
    reader.onload = () => {
      const img = new Image()
      img.onload = () => {
        // Redimensiona (máx. 1600px) e comprime para não encher o localStorage.
        const scale = Math.min(1, 1600 / Math.max(img.width, img.height))
        const c = document.createElement('canvas')
        c.width = Math.round(img.width * scale)
        c.height = Math.round(img.height * scale)
        c.getContext('2d')!.drawImage(img, 0, 0, c.width, c.height)
        const url = c.toDataURL('image/jpeg', 0.72)
        setLoginBg(url)
        setBg(url)
      }
      img.src = reader.result as string
    }
    reader.readAsDataURL(file)
  }

  const tabs: { key: SettingsTab; label: string }[] = [
    { key: 'appearance', label: t('settings.appearance') },
    { key: 'account', label: t('settings.account') },
    { key: 'security', label: t('settings.security', 'Segurança') },
    { key: 'brand', label: 'Marca' },
  ]

  return (
    <div className="settings-backdrop" onClick={onClose}>
      <div className="settings-drawer" onClick={(e) => e.stopPropagation()} role="dialog" aria-label={t('settings.title')}>
        <header className="settings-head">
          <h2><SettingsIcon /> {t('settings.title')}</h2>
          <button className="icon-btn" onClick={onClose} aria-label={t('common.close')}><CloseIcon /></button>
        </header>

        <nav className="settings-tabs">
          {tabs.map((tb) => (
            <button
              key={tb.key}
              className={tab === tb.key ? 'settings-tab active' : 'settings-tab'}
              onClick={() => setTab(tb.key)}
            >
              {tb.label}
            </button>
          ))}
        </nav>

        <div className="settings-body">
          {tab === 'appearance' && (
            <>
              <section className="settings-group">
                <h3>{t('settings.appearance')}</h3>
                <ThemePicker />
              </section>
              <section className="settings-group">
                <h3>{t('settings.language')}</h3>
                <LanguageToggle />
              </section>
              <section className="settings-group">
                <h3>{t('tour.settingsTitle', 'Introdução')}</h3>
                <small className="muted">{t('tour.settingsHint', 'Faz de novo o tour guiado da plataforma.')}</small>
                <button
                  className="btn-sm"
                  onClick={() => { onClose(); window.dispatchEvent(new Event('dx-start-tour')) }}
                >
                  {t('tour.replay', 'Ver introdução')}
                </button>
              </section>
            </>
          )}

          {tab === 'account' && (
            <section className="settings-group">
              <div className="account-edit">
                <label className="set-label">
                  Nome de utilizador
                  <input value={username} onChange={(e) => setUsername(e.target.value)} maxLength={40} />
                </label>
                <label className="set-label">
                  Nova password <small className="muted">(deixa vazio para manter)</small>
                  <PasswordInput value={password} onChange={setPassword} placeholder="mín. 8 caracteres" autoComplete="new-password" minLength={8} />
                </label>
                <div className="account-actions">
                  <button className="btn-sm primary" disabled={!dirty || saving} onClick={() => void saveAccount()}>
                    {saving ? 'A guardar…' : 'Guardar alterações'}
                  </button>
                  {msg && <span className="account-msg">{msg}</span>}
                </div>
              </div>
              <button className="btn-ghost small" onClick={onLogout}>{t('nav.logout')}</button>
            </section>
          )}

          {tab === 'security' && (
            <section className="settings-group">
              <MfaPanel />
            </section>
          )}

          {tab === 'brand' && (
            <>
              <section className="settings-group">
                <h3>Nome da aplicação</h3>
                <small className="muted">Substitui «Delonix Meet» no cabeçalho e no login.</small>
                <div className="brand-row">
                  <input
                    className="brand-name-input"
                    value={appName}
                    maxLength={30}
                    onChange={(e) => setAppNameState(e.target.value)}
                    placeholder="Delonix Meet"
                  />
                  <button
                    className="btn-sm primary"
                    onClick={() => {
                      setAppName(appName)
                      setMsg('✓ Nome atualizado')
                      setTimeout(() => setMsg(''), 2500)
                    }}
                  >
                    Aplicar
                  </button>
                </div>
              </section>
              <section className="settings-group">
                <h3>Fundo do login</h3>
                <small className="muted">Imagem de fundo do ecrã de entrada (mostrada desfocada).</small>
                {bg && (
                  <div className="brand-bg-preview" style={{ backgroundImage: `url(${bg})` }} />
                )}
                <div className="brand-row">
                  <label className="btn-sm">
                    {bg ? 'Trocar imagem' : 'Escolher imagem'}
                    <input type="file" accept="image/*" style={{ display: 'none' }} onChange={(e) => onPickBg(e.target.files?.[0] ?? null)} />
                  </label>
                  {bg && (
                    <button className="btn-ghost small" onClick={() => { setLoginBg(null); setBg(null) }}>
                      Remover
                    </button>
                  )}
                </div>
              </section>
            </>
          )}
        </div>
      </div>
    </div>
  )
}

/**
 * Criar reunião e entrar por código. Vive em DOIS sítios — na barra de topo em
 * desktop e dentro da gaveta em ecrã estreito — porque abaixo de 860px a barra
 * escondia-as com `display: none` e entrar por código deixava simplesmente de
 * existir no telemóvel (achado 3.1.4). São duas instâncias com estado próprio;
 * só uma está visível de cada vez, por isso não há foco duplicado.
 */
function QuickActions({
  onEnterRoom,
  username,
  onDone,
  variant,
}: {
  onEnterRoom: (code: string, voice?: boolean) => void
  username: string
  onDone?: () => void
  variant: 'bar' | 'drawer'
}) {
  const { t } = useTranslation()
  const [code, setCode] = useState('')
  const [creating, setCreating] = useState(false)
  const [err, setErr] = useState('')

  async function newMeeting() {
    setErr('')
    setCreating(true)
    try {
      const room = await createRoom(`Reunião de ${username}`, 'sfu', false, false, 'normal')
      onDone?.()
      onEnterRoom(room.code)
    } catch (e) {
      setErr((e as Error).message)
    } finally {
      setCreating(false)
    }
  }

  async function join(e: FormEvent) {
    e.preventDefault()
    setErr('')
    // Aceita link completo, código com sufixo ou código puro (mesma regra da Home).
    const raw = code.trim().toLowerCase()
    const parsed = (raw.match(/[a-z]+-[a-z]+-[a-z]+/) ?? [raw.replace(/^.*\/r\//, '')])[0]
    if (!parsed) {
      setErr(t('dash.notFound'))
      return
    }
    try {
      const room = await getRoom(parsed)
      onDone?.()
      onEnterRoom(room.code)
    } catch {
      setErr(t('dash.notFound'))
    }
  }

  return (
    <div className={`quick-actions qa-${variant}`}>
      <button className="app-bar-new" disabled={creating} onClick={() => void newMeeting()}>
        {creating ? t('dash.creating') : t('dash.newMeeting')}
      </button>
      <form className="app-bar-join" onSubmit={join}>
        <input
          placeholder={t('dash.joinPh')}
          value={code}
          onChange={(e) => setCode(e.target.value)}
          aria-label={t('dash.joinPh')}
        />
        <button disabled={!code.trim()}>{t('dash.join')}</button>
      </form>
      {err && <span className="app-bar-err" role="alert">{err}</span>}
    </div>
  )
}

/**
 * Barra de aplicação (topo do conteúdo): abre a gaveta em ecrã estreito, mostra
 * a data, o interruptor de tema e — em desktop — as ações rápidas.
 */
function AppBar({
  onEnterRoom,
  username,
  onOpenNav,
  navOpen,
}: {
  onEnterRoom: (code: string, voice?: boolean) => void
  username: string
  onOpenNav: () => void
  navOpen: boolean
}) {
  const { t, i18n } = useTranslation()
  const [theme, setTheme] = useState<Theme>(storedTheme)
  const locale = i18n.language.startsWith('en') ? 'en-GB' : i18n.language.startsWith('fr') ? 'fr-FR' : 'pt-PT'
  const now = new Date()
  const dateLabel = `${now.toLocaleDateString(locale, { weekday: 'long', day: 'numeric', month: 'long' })} · ${now.toLocaleTimeString(locale, { hour: '2-digit', minute: '2-digit' })}`

  function toggleTheme() {
    const next = theme === 'default' ? 'delonix-light' : 'default'
    setTheme(next)
    applyTheme(next)
  }

  return (
    <header className="app-bar">
      <button
        className="app-bar-burger"
        onClick={onOpenNav}
        aria-label={t('nav.toggle')}
        aria-expanded={navOpen}
        aria-controls="shell-nav"
      >
        <MenuIcon />
      </button>
      <span className="app-bar-date">{dateLabel}</span>
      <div className="app-bar-right">
        <button
          className="app-bar-icon"
          onClick={toggleTheme}
          title={t('common.theme', 'Tema')}
          aria-label={t('common.theme', 'Tema')}
        >
          <ThemeIcon />
        </button>
        <QuickActions variant="bar" onEnterRoom={onEnterRoom} username={username} />
      </div>
    </header>
  )
}

/** Layout com barra lateral de navegação e topo — partilhado por Home/Gravações/Calendário. */
export default function Shell({
  user,
  active,
  onNavigate,
  onLogout,
  onEnterRoom,
  children,
}: {
  user: User
  active: NavKey
  onNavigate: (k: NavKey) => void
  onLogout: () => void
  onEnterRoom: (code: string, voice?: boolean) => void
  children: ReactNode
}) {
  const { t } = useTranslation()
  const [collapsed, setCollapsed] = useState(() => localStorage.getItem('dx_nav_collapsed') === '1')
  // Gaveta em ecrã estreito (achado 3.1.1). O `collapsed` é preferência de
  // DESKTOP e persiste; isto é estado efémero de navegação e nunca persiste.
  const [navOpen, setNavOpen] = useState(false)
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [acctOpen, setAcctOpen] = useState(false)
  const [paletteOpen, setPaletteOpen] = useState(false)
  const [isAdmin, setIsAdmin] = useState(false)
  const acctRef = useRef<HTMLDivElement>(null)
  // Papel: admin de alguma org → mostra a secção Administração + Análises.
  // Padrão do delonix-portal: um AbortController por efeito e `isAbort` no
  // catch. O `catch(() => {})` que aqui estava engolia TUDO — incluindo a
  // resposta que dizia que a pessoa É admin e falhou por rede.
  useEffect(() => {
    const ctrl = new AbortController()
    myOrgs(ctrl.signal)
      .then((orgs) => {
        if (!ctrl.signal.aborted) setIsAdmin(orgs.some((o) => o.role === 'admin'))
      })
      .catch((e: unknown) => {
        if (isAbort(e)) return
        // Não é fatal — a consola funciona sem a secção de administração —,
        // mas deixa rasto: sem isto, um admin sem menu não tinha explicação.
        console.warn('[shell] não foi possível determinar o papel na organização:', e)
      })
    return () => ctrl.abort()
  }, [])
  // Esc fecha a gaveta. Sem isto, num telemóvel só um toque no backdrop a
  // fechava — e com teclado (tablet com teclado, leitor de ecrã) não havia saída.
  useEffect(() => {
    if (!navOpen) return
    const onEsc = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        setNavOpen(false)
        document.querySelector<HTMLButtonElement>('.app-bar-burger')?.focus()
      }
    }
    document.addEventListener('keydown', onEsc)
    return () => document.removeEventListener('keydown', onEsc)
  }, [navOpen])

  // Atalho global do command palette (Cmd/Ctrl-K).
  useEffect(() => {
    const on = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && (e.key === 'k' || e.key === 'K')) {
        e.preventDefault()
        setPaletteOpen((o) => !o)
      }
    }
    window.addEventListener('keydown', on)
    return () => window.removeEventListener('keydown', on)
  }, [])
  const [brand, setBrand] = useState<[string, string]>(appNameParts)
  // Fecha o menu de conta ao clicar fora ou premir Esc.
  useEffect(() => {
    if (!acctOpen) return
    const onDoc = (e: MouseEvent) => {
      if (acctRef.current && !acctRef.current.contains(e.target as Node)) setAcctOpen(false)
    }
    const onEsc = (e: KeyboardEvent) => { if (e.key === 'Escape') setAcctOpen(false) }
    document.addEventListener('mousedown', onDoc)
    document.addEventListener('keydown', onEsc)
    return () => {
      document.removeEventListener('mousedown', onDoc)
      document.removeEventListener('keydown', onEsc)
    }
  }, [acctOpen])
  useEffect(() => {
    const on = () => setBrand(appNameParts())
    window.addEventListener('dx-branding', on)
    return () => window.removeEventListener('dx-branding', on)
  }, [])
  const initials = user.username.slice(0, 2).toUpperCase()
  function toggleCollapse() {
    setCollapsed((c) => {
      const n = !c
      localStorage.setItem('dx_nav_collapsed', n ? '1' : '')
      return n
    })
  }
  // Fecha a gaveta e navega. Numa gaveta sobreposta, escolher um destino e
  // ficar a olhar para a gaveta é o erro clássico deste padrão.
  function go(k: NavKey) {
    setNavOpen(false)
    onNavigate(k)
  }

  return (
    <div className={`shell${collapsed ? ' collapsed' : ''}${navOpen ? ' nav-open' : ''}`}>
      {navOpen && (
        <div className="shell-nav-backdrop" onClick={() => setNavOpen(false)} aria-hidden="true" />
      )}
      <aside className="shell-nav" id="shell-nav">
        <div className="shell-brand">
          <button className="nav-burger" onClick={toggleCollapse} aria-label={t('nav.toggle')} title={t('nav.toggle')}>
            <MenuIcon />
          </button>
          <span className="brand-square" aria-hidden="true">{(brand[0] || 'D').trim().charAt(0).toUpperCase()}</span>
          <span className="brand-text">
            {brand[0]} <span>{brand[1]}</span>
          </span>
        </div>
        <QuickActions
          variant="drawer"
          onEnterRoom={onEnterRoom}
          username={user.username}
          onDone={() => setNavOpen(false)}
        />
        <button
          className="nav-search"
          onClick={() => { setNavOpen(false); setPaletteOpen(true) }}
          title={collapsed ? t('cmd.title', 'Comandos') : undefined}
          aria-label={t('cmd.title', 'Comandos')}
        >
          <span className="nav-search-icon" aria-hidden="true"><SearchIcon /></span>
          <span className="nav-search-label">{t('cmd.searchLabel', 'Procurar')}</span>
          <kbd className="nav-search-kbd">⌘K</kbd>
        </button>
        <nav>
          {NAV_SECTIONS.filter((s) => s.titleKey !== 'navSection.admin' || isAdmin).map((section) => (
            <div className="nav-section" key={section.titleKey}>
              <div className="nav-section-label">{t(section.titleKey)}</div>
              {section.items.map((n) => (
                <button
                  key={n.key}
                  data-tour={`nav-${n.key}`}
                  className={active === n.key ? 'nav-item active' : 'nav-item'}
                  aria-current={active === n.key ? 'page' : undefined}
                  onClick={() => go(n.key)}
                  title={collapsed ? t(n.labelKey) : undefined}
                >
                  {n.icon}
                  <span>{t(n.labelKey)}</span>
                </button>
              ))}
            </div>
          ))}
        </nav>
        <div className="shell-nav-foot">
          <NotificationCenter onNavigate={onNavigate} />
          <button className="nav-item" data-tour="settings" onClick={() => setSettingsOpen(true)} title={collapsed ? t('settings.title') : undefined}>
            <SettingsIcon />
            <span>{t('settings.title')}</span>
          </button>
          <div className="nav-account" ref={acctRef}>
            <button
              className="nav-user"
              data-tour="account"
              aria-haspopup="menu"
              aria-expanded={acctOpen}
              onClick={() => setAcctOpen((o) => !o)}
              title={collapsed ? user.username : undefined}
            >
              <span className="avatar-circle small">{initials}</span>
              <span className="nav-user-name">{user.username}</span>
              <span className="nav-user-caret" aria-hidden="true"><ChevronDownIcon /></span>
            </button>
            {acctOpen && (
              <div className="nav-account-menu" role="menu">
                <div className="acct-head">
                  <span className="avatar-circle small">{initials}</span>
                  <div className="acct-id">
                    <strong>{user.username}</strong>
                    <small>{user.email}</small>
                  </div>
                </div>
                <button role="menuitem" onClick={() => { setAcctOpen(false); setSettingsOpen(true) }}>
                  <SettingsIcon /> {t('settings.title')}
                </button>
                <button role="menuitem" onClick={() => { setAcctOpen(false); window.dispatchEvent(new Event('dx-start-tour')) }}>
                  {t('tour.replay', 'Ver introdução')}
                </button>
                <button role="menuitem" className="acct-danger" onClick={onLogout}>
                  {t('nav.logout')}
                </button>
              </div>
            )}
          </div>
        </div>
      </aside>
      <main className="shell-main">
        <AppBar onEnterRoom={onEnterRoom} username={user.username} onOpenNav={() => setNavOpen(true)} navOpen={navOpen} />
        <div className="shell-body">{children}</div>
      </main>
      {settingsOpen && <SettingsModal user={user} onClose={() => setSettingsOpen(false)} onLogout={onLogout} />}
      <OnboardingTour />
      <CommandPalette
        open={paletteOpen}
        onClose={() => setPaletteOpen(false)}
        onNavigate={onNavigate}
        onEnterRoom={onEnterRoom}
        onLogout={onLogout}
        username={user.username}
        isAdmin={isAdmin}
      />
    </div>
  )
}
