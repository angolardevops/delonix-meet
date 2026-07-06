import { FormEvent, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { login, register, User } from '../api'
import { LanguageToggle } from '../components/Shell'

export default function Login({ onLogin }: { onLogin: (u: User) => void }) {
  const { t } = useTranslation()
  const [mode, setMode] = useState<'login' | 'register'>('login')
  const [email, setEmail] = useState('')
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  const [busy, setBusy] = useState(false)

  async function submit(e: FormEvent) {
    e.preventDefault()
    setError('')
    setBusy(true)
    try {
      const user =
        mode === 'login' ? await login(email, password) : await register(email, username, password)
      onLogin(user)
    } catch (err) {
      setError((err as Error).message)
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="auth-page">
      <div className="auth-lang">
        <LanguageToggle />
      </div>
      <div className="auth-card">
        <h1 className="logo">
          Delonix <span>Meet</span>
        </h1>
        <p className="tagline">{t('login.tagline')}</p>

        <div className="auth-tabs">
          <button
            className={mode === 'login' ? 'auth-tab active' : 'auth-tab'}
            onClick={() => setMode('login')}
            type="button"
          >
            {t('login.tabLogin')}
          </button>
          <button
            className={mode === 'register' ? 'auth-tab active' : 'auth-tab'}
            onClick={() => setMode('register')}
            type="button"
          >
            {t('login.tabRegister')}
          </button>
        </div>

        <form onSubmit={submit}>
          {mode === 'register' && (
            <input
              placeholder={t('login.username')}
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              required
              minLength={2}
            />
          )}
          <input
            type="email"
            placeholder={t('login.email')}
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            required
          />
          <input
            type="password"
            placeholder={t('login.pass')}
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            required
            minLength={8}
          />
          {error && <div className="error">{error}</div>}
          {notice && <div className="notice">{notice}</div>}
          <button className="primary" disabled={busy}>
            {busy ? '…' : mode === 'login' ? t('login.submit') : t('login.submitReg')}
          </button>
        </form>

        {mode === 'login' && (
          <button className="link small-link" onClick={() => setNotice(t('login.forgotSoon'))}>
            {t('login.forgot')}
          </button>
        )}

        <div className="auth-divider">
          <span>{t('common.or')}</span>
        </div>
        {/* SSO stub — paridade visual; OIDC/SAML reais na fase "Próximo". */}
        <button className="sso-btn" onClick={() => setNotice(t('login.ssoSoon'))}>
          {t('login.sso')}
        </button>
      </div>
    </div>
  )
}
