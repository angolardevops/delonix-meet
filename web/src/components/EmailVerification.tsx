/**
 * O estado da prova do endereço de email, nas definições da conta (D7).
 *
 * A reposição de password pela própria pessoa só vai servir endereços provados,
 * por isso isto não é decoração: é o que diz à pessoa se, no dia em que perder a
 * password, o email chega a ela.
 */
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ApiError, apiErrorCode, type EmailVerificationStatus, emailVerificationStatus, requestEmailVerification } from '../api'
import { Alert, Button, StatusBadge } from '../ui/kit'

function chaveDoErro(e: unknown): string {
  const codigo = apiErrorCode(e)
  if (codigo === 'mail.disabled' || codigo === 'mail.public_url_missing') return 'auth.emailProva.semCorreio'
  if (e instanceof ApiError && e.status === 429) return 'auth.emailProva.erroMuitos'
  return 'ui.erroGenerico'
}

export default function EmailVerification() {
  const { t, i18n } = useTranslation()
  const [estado, setEstado] = useState<EmailVerificationStatus | null>(null)
  const [busy, setBusy] = useState(false)
  const [erro, setErro] = useState('')

  useEffect(() => {
    let vivo = true
    emailVerificationStatus()
      .then((s) => vivo && setEstado(s))
      .catch(() => vivo && setEstado(null))
    return () => {
      vivo = false
    }
  }, [])

  async function enviar() {
    setBusy(true)
    setErro('')
    try {
      setEstado(await requestEmailVerification())
    } catch (e) {
      setErro(t(chaveDoErro(e)))
    } finally {
      setBusy(false)
    }
  }

  if (!estado) return null
  if (estado.status === 'verified') {
    return <StatusBadge tone="success">{t('auth.emailProva.provado')}</StatusBadge>
  }
  const ate = estado.expires_at
    ? new Date(estado.expires_at).toLocaleString(i18n.language, { dateStyle: 'short', timeStyle: 'short' })
    : null
  return (
    <div className="email-prova" role="status">
      {estado.status === 'unverified' ? (
        <p className="dx-muted">{t('auth.emailProva.porProvar')}</p>
      ) : (
        <p className="dx-muted">{t('auth.emailProva.enviado', { email: estado.email, ate })}</p>
      )}
      {erro && <Alert tone="danger">{erro}</Alert>}
      <Button type="button" size="sm" busy={busy} onClick={() => void enviar()}>
        {estado.status === 'unverified' ? t('auth.emailProva.enviar') : t('auth.emailProva.reenviar')}
      </Button>
    </div>
  )
}
