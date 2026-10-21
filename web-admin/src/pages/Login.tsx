/**
 * Entrar — simples de propósito: email/palavra-passe e um botão. É o
 * backoffice de um punhado de operadores, não a porta de entrada da
 * consola de tenant (`web/src/pages/Login.tsx`, com SSO, MFA e registo).
 */
import { FormEvent, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage, login, User } from '../api'
import { Alert, Button, Field, TextInput } from '../ui/kit'

export default function Login({ onLogin }: { onLogin: (u: User) => void }) {
  const { t } = useTranslation()
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')

  async function submit(e: FormEvent) {
    e.preventDefault()
    setBusy(true)
    setErr('')
    try {
      const u = await login(email.trim(), password)
      onLogin(u)
    } catch (x) {
      setErr(apiErrorMessage(x, t('auth.erro')))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div style={{ minHeight: '100dvh', display: 'grid', placeItems: 'center', background: 'var(--surface)', padding: 16 }}>
      <div style={{ width: 360, maxWidth: '100%' }} className="dx-card">
        <div className="dx-card__body" style={{ display: 'grid', gap: 16 }}>
          <div>
            <h1 style={{ margin: 0, fontFamily: 'var(--font-display)', fontSize: 20 }}>{t('auth.titulo')}</h1>
            <p className="dx-muted" style={{ margin: '4px 0 0' }}>
              {t('auth.subtitulo')}
            </p>
          </div>
          <form onSubmit={(e) => void submit(e)} style={{ display: 'grid', gap: 12 }}>
            <Field label={t('auth.email')} htmlFor="login-email">
              <TextInput
                id="login-email"
                type="email"
                autoComplete="username"
                required
                value={email}
                onChange={(e) => setEmail(e.target.value)}
              />
            </Field>
            <Field label={t('auth.password')} htmlFor="login-password">
              <TextInput
                id="login-password"
                type="password"
                autoComplete="current-password"
                required
                value={password}
                onChange={(e) => setPassword(e.target.value)}
              />
            </Field>
            {err && <Alert tone="danger">{err}</Alert>}
            <Button type="submit" variant="primary" busy={busy} block>
              {t('auth.entrar')}
            </Button>
          </form>
        </div>
      </div>
    </div>
  )
}
