import { FormEvent, useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { login, registerOrg, ssoCheck, ssoRedirect, User } from '../api'
import { LanguageToggle } from '../components/Shell'
import PasswordInput from '../components/PasswordInput'
import { appNameParts, getLoginBg } from '../branding'

export default function Login({ onLogin }: { onLogin: (u: User) => void }) {
  const { t } = useTranslation()
  const [mode, setMode] = useState<'login' | 'register'>('login')
  const [orgName, setOrgName] = useState('')
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  const [busy, setBusy] = useState(false)

  // SSO state
  const [ssoEnabled, setSsoEnabled] = useState(false)
  const [ssoEnforced, setSsoEnforced] = useState(false)
  const [ssoChecking, setSsoChecking] = useState(false)

  // Verificar SSO quando o email muda (debounce 500ms).
  const checkSso = useCallback(async (emailValue: string) => {
    const domain = emailValue.split('@')[1]
    if (!domain || !domain.includes('.')) {
      setSsoEnabled(false)
      setSsoEnforced(false)
      return
    }
    setSsoChecking(true)
    try {
      const result = await ssoCheck(domain)
      setSsoEnabled(result.sso_enabled)
      setSsoEnforced(result.enforce_sso)
    } catch {
      setSsoEnabled(false)
      setSsoEnforced(false)
    } finally {
      setSsoChecking(false)
    }
  }, [])

  useEffect(() => {
    if (mode !== 'login') return
    const timer = setTimeout(() => checkSso(email), 500)
    return () => clearTimeout(timer)
  }, [email, mode, checkSso])

  async function submit(e: FormEvent) {
    e.preventDefault()
    setError('')
    setBusy(true)
    try {
      const user =
        mode === 'login' ? await login(email, password) : await registerOrg(orgName, email, password)
      onLogin(user)
    } catch (err) {
      setError((err as Error).message)
    } finally {
      setBusy(false)
    }
  }

  function handleSsoClick() {
    const domain = email.split('@')[1]
    if (domain && domain.includes('.')) {
      ssoRedirect(domain)
    } else {
      setNotice(t('login.ssoNeedEmail'))
    }
  }

  const [brand0, brand1] = appNameParts()
  const loginBg = getLoginBg()

  return (
    <div className="auth-page">
      {loginBg && <div className="auth-bg" style={{ backgroundImage: `url(${loginBg})` }} />}
      <div className="auth-lang">
        <LanguageToggle />
      </div>
      <div className="auth-card">
        <img src="/logo.svg" alt="" className="brand-logo auth-logo" />
        <h1 className="logo">
          {brand0} <span>{brand1}</span>
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

        {mode === 'register' && <p className="auth-hint">{t('login.orgHint')}</p>}

        <form onSubmit={submit}>
          {mode === 'register' && (
            <input
              placeholder={t('login.orgName')}
              value={orgName}
              onChange={(e) => setOrgName(e.target.value)}
              required
              minLength={2}
            />
          )}
          <input
            type="email"
            placeholder={mode === 'register' ? t('login.emailCorp') : t('login.email')}
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            required
          />

          {/* SSO enforced: ocultar password e mostrar aviso */}
          {mode === 'login' && ssoEnforced ? (
            <div className="sso-enforced-notice">
              <p>{t('login.ssoEnforced')}</p>
              <button
                type="button"
                className="primary"
                onClick={handleSsoClick}
                disabled={ssoChecking}
              >
                {t('login.ssoRedirect')}
              </button>
            </div>
          ) : (
            <>
              <PasswordInput
                placeholder={t('login.pass')}
                value={password}
                onChange={setPassword}
                required
                minLength={8}
                autoComplete={mode === 'register' ? 'new-password' : 'current-password'}
              />
              {error && <div className="error">{error}</div>}
              {notice && <div className="notice">{notice}</div>}
              <button className="primary" disabled={busy}>
                {busy ? '…' : mode === 'login' ? t('login.submit') : t('login.submitReg')}
              </button>
            </>
          )}
        </form>

        {mode === 'login' && !ssoEnforced && (
          <button className="link small-link" onClick={() => setNotice(t('login.forgotSoon'))}>
            {t('login.forgot')}
          </button>
        )}

        <div className="auth-divider">
          <span>{t('common.or')}</span>
        </div>
        {/* SSO: funcional se o domínio tem OIDC configurado; info se não. */}
        <button
          className="sso-btn"
          onClick={handleSsoClick}
          disabled={ssoChecking}
        >
          {ssoEnabled ? t('login.ssoGo') : t('login.sso')}
        </button>
        {ssoEnabled && !ssoEnforced && (
          <p className="sso-hint">{t('login.ssoAvailable')}</p>
        )}
      </div>
    </div>
  )
}
