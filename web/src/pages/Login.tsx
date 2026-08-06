import { FormEvent, useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { login, registerOrg, ssoCheck, ssoRedirect, getPlatformSettings, User } from '../api'
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

  // Flags da plataforma (integração Odoo pode ocultar registo e SSO)
  const [hideOrgCreation, setHideOrgCreation] = useState(false)
  const [hideSsoButton, setHideSsoButton] = useState(false)

  useEffect(() => {
    getPlatformSettings()
      .then((s) => {
        setHideOrgCreation(s.hide_org_creation)
        setHideSsoButton(s.hide_sso_button)
        if (s.hide_org_creation && mode === 'register') setMode('login')
      })
      .catch(() => {/* sem settings públicas — manter defaults */})
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

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
    // Cartão único centrado sobre um radial-gradient de acento (handoff §1).
    // O painel de marca lateral saiu: o template põe o foco na entrada, e a
    // marca vive no cabeçalho do próprio cartão.
    <div className="auth-page">
      {loginBg && <div className="auth-bg" style={{ backgroundImage: `url(${loginBg})` }} />}
      <div className="auth-lang">
        <LanguageToggle />
      </div>

      <div className="auth-card">
        <div className="auth-brand">
          <span className="auth-mark" aria-hidden>{brand0.charAt(0)}</span>
          <span className="auth-brand-name">{brand0} <span>{brand1}</span></span>
        </div>
        <p className="tagline">{t('login.tagline')}</p>

        <div className="auth-tabs">
          <button
            className={mode === 'login' ? 'auth-tab active' : 'auth-tab'}
            onClick={() => setMode('login')}
            type="button"
          >
            {t('login.tabLogin')}
          </button>
          {!hideOrgCreation && (
            <button
              className={mode === 'register' ? 'auth-tab active' : 'auth-tab'}
              onClick={() => setMode('register')}
              type="button"
            >
              {t('login.tabRegister')}
            </button>
          )}
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

        {!hideSsoButton && (
          <>
            <div className="auth-divider">
              <span>{t('common.or')}</span>
            </div>
            {/* SSO: activo se o domínio tem OIDC configurado. */}
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
          </>
        )}
        {/* Rodapé de confiança (handoff §1): diz o que a plataforma garante,
            em mono discreto, sem competir com a acção de entrar. */}
        <p className="auth-assurance">{t('login.assurance')}</p>
        <p className="auth-terms">
          {t('login.termsPre')} <a href="#/legal">{t('login.termsLink')}</a>.
        </p>
      </div>
    </div>
  )
}
