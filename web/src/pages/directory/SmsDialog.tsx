/**
 * SMS a um contacto da organização (ADR-0005 §Contactos). O número nunca passa
 * por aqui: manda-se o `user_id` e o servidor resolve-o, prefixa o nome de
 * quem envia e põe a mensagem na fila (202).
 *
 * O contador espelha o `sms_codec` do servidor sobre o corpo que ele vai
 * enviar (com o prefixo), para quem escreve ver o custo em partes antes de
 * enviar. As recusas do contrato têm mensagem própria; o resto mostra o que o
 * servidor disse.
 */
import { useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ApiError, apiErrorMessage, Employee, sendSmsToContact, SmsMessage, smsErrorCode } from '../../api'
import { Alert, Button, Dialog, Field, TextArea } from '../../ui/kit'
import { contactBody, countSms, SMS_MAX_SEGMENTS } from './smsCount'

/** Resultado de uma recusa que muda o que o diretório sabe da pessoa. */
export type SmsRefusal = 'opted-out' | 'no-phone' | 'not-member'

export default function SmsDialog({
  orgId,
  person,
  senderName,
  onClose,
  onRefused,
}: {
  orgId: string
  person: Employee
  senderName: string
  onClose: () => void
  /** O servidor recusou por causa do destinatário: o diretório deve recarregar. */
  onRefused: (r: SmsRefusal) => void
}) {
  const { t } = useTranslation()
  const [body, setBody] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [sent, setSent] = useState<SmsMessage | null>(null)
  // Uma chave por INTENÇÃO de envio: um duplo clique ou uma repetição depois de
  // um corte de rede não manda dois SMS. Muda quando o texto muda.
  const key = useRef(crypto.randomUUID())

  const count = useMemo(() => countSms(body.trim() ? contactBody(senderName, body) : ''), [body, senderName])
  const tooLong = count.segments > SMS_MAX_SEGMENTS
  const empty = !body.trim()

  async function send() {
    if (empty || tooLong || busy) return
    setBusy(true)
    setError('')
    try {
      const m = await sendSmsToContact(orgId, { user_id: person.user_id, body: body.trim() }, key.current)
      setSent(m)
    } catch (e) {
      const code = e instanceof ApiError ? smsErrorCode(e.message) : null
      const status = e instanceof ApiError ? e.status : 0
      const nome = person.username
      if (code === 'sms.recipient_opted_out') {
        setError(t('org.sms.erro.recusou', { nome }))
        onRefused('opted-out')
      } else if (code === 'sms.recipient_no_phone') {
        setError(t('org.sms.erro.semNumero', { nome }))
        onRefused('no-phone')
      } else if (status === 429) {
        setError(t('org.sms.erro.limite'))
      } else if (status === 403) {
        setError(t('org.sms.erro.semPermissao'))
      } else if (status === 404) {
        setError(t('org.sms.erro.naoMembro', { nome }))
        onRefused('not-member')
      } else {
        setError(apiErrorMessage(e, t('org.sms.erro.falhou')))
      }
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog
      title={t('org.sms.titulo', { nome: person.username })}
      onClose={onClose}
      footer={
        sent ? (
          <Button variant="primary" onClick={onClose}>
            {t('ui.fechar')}
          </Button>
        ) : (
          <>
            <Button variant="ghost" onClick={onClose}>
              {t('org.sms.cancelar')}
            </Button>
            <Button variant="primary" icon="send" busy={busy} disabled={empty || tooLong} onClick={() => void send()} data-testid="sms-send">
              {t('org.sms.enviar')}
            </Button>
          </>
        )
      }
    >
      {sent ? (
        <Alert tone="success">
          <span data-testid="sms-sent">{t('org.sms.enviado', { nome: person.username, count: sent.segments })}</span>
        </Alert>
      ) : (
        <>
          <Field
            label={t('org.sms.mensagem')}
            htmlFor="sms-body"
            hint={t('org.sms.prefixoNota', { prefixo: contactBody(senderName, '') })}
            aside={
              <span className="dx-num dx-muted" data-testid="sms-count" aria-live="polite">
                {t('org.sms.contador', {
                  chars: count.units,
                  count: count.segments,
                  max: SMS_MAX_SEGMENTS,
                  cod: count.encoding === 'gsm7' ? 'GSM-7' : 'UCS-2',
                })}
              </span>
            }
          >
            <TextArea
              id="sms-body"
              rows={5}
              value={body}
              autoFocus
              onChange={(e) => {
                setBody(e.target.value)
                key.current = crypto.randomUUID()
                setError('')
              }}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) void send()
              }}
            />
          </Field>
          {count.encoding === 'ucs2' && !empty && <p className="dx-muted">{t('org.sms.ucs2Nota')}</p>}
          {tooLong && <Alert tone="warning">{t('org.sms.demasiadoLonga', { max: SMS_MAX_SEGMENTS })}</Alert>}
          {error && (
            <Alert tone="danger">
              <span data-testid="sms-error">{error}</span>
            </Alert>
          )}
        </>
      )}
    </Dialog>
  )
}
