/**
 * «Configurar o Linphone» (R278, item 3.8): o servidor emite um bilhete de
 * uso único e o diálogo mostra-o num QR, com o tempo que falta.
 *
 * Ninguém digita a password SIP: o Linphone lê o QR, descarrega a
 * configuração e fica com uma password NOVA. Por isso o diálogo avisa ANTES
 * de emitir — ler o QR troca a password, e o aparelho antigo deixa de
 * registar. O QR reutiliza o gerador que o MFA já usa (`qrcode`).
 *
 * O URL do bilhete é a credencial e o resgate é um GET com efeito: abri-lo num
 * browser, numa pré-visualização de link ou num leitor de QR genérico gasta o
 * bilhete e troca a password. Por isso NÃO se mostra em texto copiável — só
 * quando o QR não se conseguiu desenhar, que é o único caso em que faz falta.
 */
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { TFunction } from 'i18next'
import { apiErrorCode, type ProvisioningTicket } from '../api'
import { Alert, Button, Dialog } from '../ui/kit'

/** A mensagem de uma recusa da emissão, pelo código estável do servidor. */
export function qrErrorMessage(e: unknown, t: TFunction): string {
  switch (apiErrorCode(e)) {
    case 'ramais.sip_server_missing':
      return t('consola.ramais.qr.erro.semServidor')
    case 'ramais.public_url_missing':
      return t('consola.ramais.qr.erro.semEndereco')
    case 'ramais.extension_inactive':
      return t('consola.ramais.qr.erro.inactivo')
    default:
      return t('consola.ramais.qr.erro.generico')
  }
}

/** `m:ss` do que falta; nunca negativo. */
export function remainingText(ms: number): string {
  const s = Math.max(0, Math.ceil(ms / 1000))
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`
}

export type QrStep =
  | { k: 'confirm' }
  | { k: 'busy' }
  | { k: 'shown'; ticket: ProvisioningTicket; qr: string | null }
  | { k: 'error'; text: string }

/** O conteúdo do diálogo, sem pedidos: o que se desenha a partir do estado. */
export function LinphoneQrBody({
  extension,
  step,
  now,
  onIssue,
}: {
  extension: string
  step: QrStep
  now: number
  onIssue: () => void
}) {
  const { t } = useTranslation()
  if (step.k === 'shown') {
    const left = new Date(step.ticket.expires_at).getTime() - now
    const expired = left <= 0
    return (
      <div className="linphone-qr" data-testid="linphone-qr">
        {expired ? (
          <Alert tone="warning">{t('consola.ramais.qr.expirou')}</Alert>
        ) : (
          <>
            <p className="dx-muted" style={{ margin: 0 }}>
              {t('consola.ramais.qr.instrucoes')}
            </p>
            {step.qr ? (
              <div
                className="linphone-qr__code"
                role="img"
                aria-label={t('consola.ramais.qr.alt', { extensao: extension })}
                dangerouslySetInnerHTML={{ __html: step.qr }}
              />
            ) : (
              <>
                <Alert tone="warning">{t('consola.ramais.qr.semImagem')}</Alert>
                <p className="linphone-qr__url dx-muted">{step.ticket.provisioning_url}</p>
              </>
            )}
            <p className="linphone-qr__left" role="timer" aria-live="off">
              {t('consola.ramais.qr.validade', { tempo: remainingText(left) })}
            </p>
          </>
        )}
        <Alert tone="warning">{t('consola.ramais.qr.trocaPassword')}</Alert>
        {!expired && <p className="dx-muted" style={{ margin: 0 }}>{t('consola.ramais.qr.soLinphone')}</p>}
        {expired && (
          <Button variant="secondary" onClick={onIssue}>
            {t('consola.ramais.qr.outro')}
          </Button>
        )}
      </div>
    )
  }
  return (
    <div className="linphone-qr" data-testid="linphone-qr">
      <p style={{ margin: 0 }}>{t('consola.ramais.qr.explica', { extensao: extension })}</p>
      <Alert tone="warning">{t('consola.ramais.qr.trocaPassword')}</Alert>
      {step.k === 'error' && <Alert tone="danger">{step.text}</Alert>}
      <Button variant="primary" busy={step.k === 'busy'} onClick={onIssue}>
        {t('consola.ramais.qr.gerar')}
      </Button>
    </div>
  )
}

export default function LinphoneQrDialog({
  extension,
  issue,
  onClose,
}: {
  extension: string
  issue: () => Promise<ProvisioningTicket>
  onClose: () => void
}) {
  const { t } = useTranslation()
  const [step, setStep] = useState<QrStep>({ k: 'confirm' })
  const [now, setNow] = useState(() => Date.now())

  useEffect(() => {
    if (step.k !== 'shown') return
    const id = window.setInterval(() => setNow(Date.now()), 1000)
    return () => window.clearInterval(id)
  }, [step.k])

  async function run() {
    setStep({ k: 'busy' })
    try {
      const ticket = await issue()
      let qr: string | null = null
      try {
        const { toString } = await import('qrcode')
        qr = await toString(ticket.provisioning_url, { type: 'svg', margin: 1, width: 220 })
      } catch {
        /* sem imagem, o URL em texto continua a servir */
      }
      setNow(Date.now())
      setStep({ k: 'shown', ticket, qr })
    } catch (e) {
      setStep({ k: 'error', text: qrErrorMessage(e, t) })
    }
  }

  return (
    <Dialog title={t('consola.ramais.qr.titulo', { extensao: extension })} onClose={onClose}>
      <LinphoneQrBody extension={extension} step={step} now={now} onIssue={() => void run()} />
      <Button variant="secondary" onClick={onClose}>
        {t('consola.ramais.concluido')}
      </Button>
    </Dialog>
  )
}
