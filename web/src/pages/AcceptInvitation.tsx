/**
 * Aceitar um convite de organização por link (`#/invite/:token`, ADR-0008 §11).
 * Exige sessão: sem ela a app mostra a entrada e, depois de entrar, volta
 * aqui. O token é a credencial — de uso único, com expiração — e o servidor
 * decide tudo (rota `invitations/accept`); esta página só o entrega.
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { acceptInvitation, apiErrorMessage } from '../api'
import { Alert, Button, Card } from '../ui/kit'
import Moldura from './publico/Moldura'

export default function AcceptInvitation({ token, onAccepted }: { token: string; onAccepted: () => void }) {
  const { t } = useTranslation()
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')

  async function accept() {
    setBusy(true)
    setErr('')
    try {
      await acceptInvitation(token)
      onAccepted()
    } catch (e) {
      setErr(apiErrorMessage(e, t('org.convite.erroAceitar')))
      setBusy(false)
    }
  }

  return (
    <Moldura pagina="invite" estreita>
      <Card className="pub-cartao">
        <div className="org-form">
          <h1>{t('org.convite.titulo')}</h1>
          <p className="dx-muted">{t('rbac.conviteExplica')}</p>
          {err && <Alert tone="danger">{err}</Alert>}
          <Button variant="primary" size="lg" block busy={busy} onClick={() => void accept()}>
            {t('rbac.conviteAceitar')}
          </Button>
        </div>
      </Card>
    </Moldura>
  )
}
