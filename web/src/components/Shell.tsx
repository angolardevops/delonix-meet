import { ReactNode, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { updateMe, User } from '../api'
import { setLanguage } from '../i18n'
import { appNameParts, getAppName, getLoginBg, setAppName, setLoginBg } from '../branding'
import PasswordInput from './PasswordInput'
import { CalendarIcon, ClockIcon, CloseIcon, FilmIcon, HomeIcon, MenuIcon, NoteIcon, PeopleIcon, SettingsIcon, StageIcon } from '../icons'

export type NavKey = 'home' | 'directory' | 'recordings' | 'calendar' | 'analytics' | 'roadmap' | 'whiteboards'

const NAV: { key: NavKey; labelKey: string; icon: ReactNode }[] = [
  { key: 'home', labelKey: 'nav.home', icon: <HomeIcon /> },
  { key: 'directory', labelKey: 'nav.org', icon: <PeopleIcon /> },
  { key: 'calendar', labelKey: 'nav.calendar', icon: <CalendarIcon /> },
  { key: 'recordings', labelKey: 'nav.recordings', icon: <FilmIcon /> },
  { key: 'whiteboards', labelKey: 'nav.whiteboards', icon: <NoteIcon /> },
  { key: 'analytics', labelKey: 'nav.analytics', icon: <ClockIcon /> },
  // Roadmap só visível em desenvolvimento local (nunca em stage/prod)
  ...(import.meta.env.DEV ? [{ key: 'roadmap' as NavKey, labelKey: 'road.navLabel', icon: <StageIcon /> }] : []),
]

export function applyTheme(theme: 'default' | 'delonix-light') {
  if (theme === 'default') delete document.documentElement.dataset.theme
  else document.documentElement.dataset.theme = theme
  localStorage.setItem('dx_theme', theme)
}

export function initTheme() {
  const t = localStorage.getItem('dx_theme')
  applyTheme(t === 'delonix-light' ? 'delonix-light' : 'default')
}

export function ThemePicker() {
  const { t } = useTranslation()
  const [theme, setTheme] = useState<'default' | 'delonix-light'>(
    (localStorage.getItem('dx_theme') as 'default' | 'delonix-light') ?? 'default'
  )
  
  function pick(t: 'default' | 'delonix-light') {
    setTheme(t)
    applyTheme(t)
  }
  
  return (
    <div className="theme-picker">
      <label className="theme-label">{t('common.theme', 'Tema')}</label>
      <select value={theme} onChange={(e) => pick(e.target.value as 'default' | 'delonix-light')}>
        <option value="default">Delonix · Escuro (Padrão)</option>
        <option value="delonix-light">Delonix · Claro</option>
      </select>
    </div>
  )
}

/** Toggle global PT/EN/FR — persiste em localStorage E na BD (sincroniza entre dispositivos). */
export function LanguageToggle() {
  const { i18n } = useTranslation()
  const lang = i18n.language.startsWith('en') ? 'en' : i18n.language.startsWith('fr') ? 'fr' : 'pt'

  function pick(l: 'pt' | 'en' | 'fr') {
    setLanguage(l)
    // Persiste à BD em best-effort — falha silenciosa para não bloquear a UI.
    import('../api').then(({ updateMe }) => updateMe({ locale: l })).catch(() => {})
  }

  return (
    <div className="lang-toggle" role="group" aria-label="Idioma / Language">
      <button className={lang === 'pt' ? 'lang-btn active' : 'lang-btn'} onClick={() => pick('pt')}>PT</button>
      <button className={lang === 'en' ? 'lang-btn active' : 'lang-btn'} onClick={() => pick('en')}>EN</button>
      <button className={lang === 'fr' ? 'lang-btn active' : 'lang-btn'} onClick={() => pick('fr')}>FR</button>
    </div>
  )
}

type SettingsTab = 'appearance' | 'account' | 'brand'

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
  const [collapsed, setCollapsed] = useState(() => localStorage.getItem('dx_nav_collapsed') === '1')
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [brand, setBrand] = useState<[string, string]>(appNameParts)
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
  return (
    <div className={collapsed ? 'shell collapsed' : 'shell'}>
      <aside className="shell-nav">
        <div className="shell-brand">
          <button className="nav-burger" onClick={toggleCollapse} aria-label={t('nav.toggle')} title={t('nav.toggle')}>
            <MenuIcon />
          </button>
          <span className="brand-text">
            {brand[0]} <span>{brand[1]}</span>
          </span>
        </div>
        <nav>
          {NAV.map((n) => (
            <button
              key={n.key}
              className={active === n.key ? 'nav-item active' : 'nav-item'}
              onClick={() => onNavigate(n.key)}
              title={collapsed ? t(n.labelKey) : undefined}
            >
              {n.icon}
              <span>{t(n.labelKey)}</span>
            </button>
          ))}
        </nav>
        <div className="shell-nav-foot">
          <button className="nav-item" onClick={() => setSettingsOpen(true)} title={collapsed ? t('settings.title') : undefined}>
            <SettingsIcon />
            <span>{t('settings.title')}</span>
          </button>
          <div className="nav-user">
            <span className="avatar-circle small">{initials}</span>
            <span className="nav-user-name">{user.username}</span>
          </div>
        </div>
      </aside>
      <main className="shell-main">{children}</main>
      {settingsOpen && <SettingsModal user={user} onClose={() => setSettingsOpen(false)} onLogout={onLogout} />}
    </div>
  )
}
