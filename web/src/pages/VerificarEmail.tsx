/**
 * Provar o endereço de email a partir do link do email
 * (`#/verificar-email?token=…`, D7). PÚBLICA: o link abre-se muitas vezes noutro
 * aparelho, sem sessão — o token é a credencial, e o servidor decide tudo.
 *
 * Pede um clique, de propósito, em vez de consumir o token ao abrir: há leitores
 * de correio que abrem os links para os examinar, e gastavam-no antes da pessoa.
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ApiError, acceptEmailVerification, apiErrorCode } from '../api'
import { Alert, Button, Card } from '../ui/kit'
import Moldura from './publico/Moldura'

type Estado = { kind: 'inicio' } | { kind: 'feito'; email: string } | { kind: 'erro'; chave: string }

/** O código do servidor → a frase que a pessoa lê. */
function chaveDoErro(e: unknown): string {
  switch (apiErrorCode(e)) {
    case 'email_verification.expired':
      return 'auth.emailProva.erroExpirado'
    case 'email_verification.email_changed':
      return 'auth.emailProva.erroMudou'
    case 'email_verification.not_found':
      return 'auth.emailProva.erroInvalido'
    default:
      return e instanceof ApiError && e.status === 429 ? 'auth.emailProva.erroMuitos' : 'ui.erroGenerico'
  }
}

export default function VerificarEmail({ token }: { token: string }) {
  const { t } = useTranslation()
  const [busy, setBusy] = useState(false)
  const [estado, setEstado] = useState<Estado>({ kind: 'inicio' })

  async function confirmar() {
    setBusy(true)
    try {
      const r = await acceptEmailVerification(token)
      setEstado({ kind: 'feito', email: r.email })
    } catch (e) {
      setEstado({ kind: 'erro', chave: chaveDoErro(e) })
    } finally {
      setBusy(false)
    }
  }

  return (
    <Moldura pagina="verificar-email" estreita>
      <Card className="pub-cartao">
        <div className="org-form">
          <h1>{t('auth.emailProva.paginaTitulo')}</h1>
          {estado.kind === 'feito' ? (
            <>
              <Alert tone="success">{t('auth.emailProva.feito', { email: estado.email })}</Alert>
              <Button variant="primary" size="lg" block onClick={() => (location.hash = '/')}>
                {t('auth.emailProva.continuar')}
              </Button>
            </>
          ) : (
            <>
              <p className="dx-muted">{t('auth.emailProva.paginaExplica')}</p>
              {estado.kind === 'erro' && <Alert tone="danger">{t(estado.chave)}</Alert>}
              <Button variant="primary" size="lg" block busy={busy} onClick={() => void confirmar()}>
                {t('auth.emailProva.confirmar')}
              </Button>
            </>
          )}
        </div>
      </Card>
    </Moldura>
  )
}
