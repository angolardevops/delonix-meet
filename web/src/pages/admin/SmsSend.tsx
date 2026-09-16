/**
 * Enviar um SMS e ver a fila. O contador de segmentos é uma ESTIMATIVA para
 * quem escreve (`smsSegments.ts`); o número cobrado vem na mensagem que o
 * servidor devolve. «Enviada» quer dizer aceite pelo modem ou pelo operador —
 * os recibos de entrega não existem (ADR-0005).
 */
import { FormEvent, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { listSmsMessages, sendSms, SmsMessage, SmsRouteChoice, SmsStatus } from '../../api'
import { AsyncSection } from '../../components/AsyncSection'
import { BadgeTone, Alert, Button, Field, Segmented, StatusBadge, TextArea, TextInput } from '../../ui/kit'
import { estimateSms } from '../../smsSegments'
import { formatDateTime, orgErrorMessage, useLocaleTag } from './orgShared'
import { newIdempotencyKey, SMS_POLL_MS, usePolled } from './smsShared'

const MESSAGES_SHOWN = 50

export function SmsSend({ orgId, onSent }: { orgId: string; onSent: () => void }) {
  const { t } = useTranslation()
  const [to, setTo] = useState('')
  const [body, setBody] = useState('')
  const [route, setRoute] = useState<SmsRouteChoice>('auto')
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')
  const [ok, setOk] = useState<SmsMessage | null>(null)
  const est = estimateSms(body)

  async function submit(e: FormEvent) {
    e.preventDefault()
    if (busy || !to.trim() || !body) return
    setErr('')
    setOk(null)
    setBusy(true)
    try {
      const m = await sendSms(orgId, { to: to.trim(), body, route }, newIdempotencyKey())
      setOk(m)
      setBody('')
      onSent()
    } catch (x) {
      // 422 sem rota / destino inválido: a razão, por extenso, vem do servidor.
      setErr(orgErrorMessage(x, t, 'consola.sms.erroPedido'))
    } finally {
      setBusy(false)
    }
  }

  return (
    <>
      <h3 className="org-voice__sub">{t('consola.sms.enviarTitulo')}</h3>
      <form className="org-form org-card-pad org-sms__form" onSubmit={(e) => void submit(e)} aria-label={t('consola.sms.enviarTitulo')}>
        <Field label={t('consola.sms.destino')} htmlFor="sms-to">
          <TextInput
            id="sms-to"
            type="tel"
            inputMode="tel"
            autoComplete="off"
            className="dx-num"
            placeholder={t('consola.sms.destinoExemplo')}
            value={to}
            onChange={(e) => setTo(e.target.value)}
            required
          />
        </Field>
        <Field label={t('consola.sms.mensagem')} htmlFor="sms-body">
          <TextArea id="sms-body" rows={4} value={body} onChange={(e) => setBody(e.target.value)} aria-describedby="sms-counter" required />
        </Field>
        <div className="org-sms__counter">
          <span id="sms-counter" className="dx-num" aria-live="polite" data-testid="sms-counter">
            {t('consola.sms.contador', {
              chars: est.units,
              segments: est.segments,
              per: est.perSegment,
              encoding: est.encoding === 'gsm7' ? 'GSM-7' : 'UCS-2',
            })}
          </span>
          {est.encoding === 'ucs2' && <span className="org-sms__reason">{t('consola.sms.ucs2')}</span>}
        </div>
        <div className="org-sms__route">
          <span className="dx-field__label">{t('consola.sms.rota')}</span>
          <Segmented
            value={route}
            onChange={setRoute}
            label={t('consola.sms.rota')}
            options={[
              { value: 'auto', label: t('consola.sms.rotaAuto') },
              { value: 'usb', label: t('consola.sms.rotaUsb') },
              { value: 'operator', label: t('consola.sms.rotaOperador') },
            ]}
          />
        </div>
        {err && <Alert tone="danger">{err}</Alert>}
        {ok && <Alert tone="success">{t('consola.sms.naFila', { to: ok.to, segments: ok.segments })}</Alert>}
        <div className="org-form__foot">
          <Button type="submit" variant="primary" icon="send" busy={busy} disabled={!to.trim() || !body}>
            {t('consola.sms.enviar')}
          </Button>
        </div>
      </form>
    </>
  )
}

const STATUS_TONE: Record<SmsStatus, BadgeTone> = {
  queued: 'neutral',
  claimed: 'warning',
  sent: 'success',
  failed: 'record',
}

export function SmsMessages({ orgId, nonce }: { orgId: string; nonce: number }) {
  const { t } = useTranslation()
  const locale = useLocaleTag()
  const [pending, setPending] = useState(false)
  // `nonce` nas dependências: um envio novo pede a lista já.
  const msgs = usePolled(
    (signal) => listSmsMessages(orgId, MESSAGES_SHOWN, signal).then((r) => r.items),
    [orgId, nonce],
    pending ? SMS_POLL_MS : null,
  )

  // Só se repete enquanto houver alguma na fila ou a enviar.
  useEffect(() => {
    const p = msgs.state.s === 'ready' && msgs.state.d.some((m) => m.status === 'queued' || m.status === 'claimed')
    setPending((prev) => (prev === p ? prev : p))
  }, [msgs.state])

  const statusLabel: Record<SmsStatus, string> = {
    queued: t('consola.sms.estadoQueued'),
    claimed: t('consola.sms.estadoClaimed'),
    sent: t('consola.sms.estadoSent'),
    failed: t('consola.sms.estadoFailed'),
  }
  const routeLabel = (r: string) => (r === 'usb' ? t('consola.sms.viaUsb') : r === 'operator' ? t('consola.sms.viaOperador') : r)

  return (
    <>
      <h3 className="org-voice__sub">{t('consola.sms.mensagens')}</h3>
      <p className="dx-muted org-card-note">{t('consola.sms.mensagensNota')}</p>
      <AsyncSection state={msgs.state} onRetry={msgs.reload}>
        {(list) =>
          list.length === 0 ? (
            <p className="dx-muted org-card-note">{t('consola.sms.semMensagens')}</p>
          ) : (
            <ul className="org-simple org-sms__list" data-testid="sms-messages">
              {list.map((m) => (
                <li key={m.id} className="org-sms__row">
                  <span className="org-simple__main">
                    <span>
                      <strong className="dx-num">{m.to}</strong>
                      <span className="dx-muted dx-num"> · {formatDateTime(m.created_at, locale)}</span>
                    </span>
                    <span className="org-sms__body">{m.body}</span>
                    <span className="dx-muted">
                      {[routeLabel(m.route), m.operator, t('consola.sms.segmentos', { n: m.segments, encoding: m.encoding })]
                        .filter(Boolean)
                        .join(' · ')}
                    </span>
                    {m.error && <span className="org-sms__error">{m.error}</span>}
                  </span>
                  <span className="org-sms__side">
                    <StatusBadge tone={STATUS_TONE[m.status] ?? 'neutral'}>{statusLabel[m.status] ?? m.status}</StatusBadge>
                  </span>
                </li>
              ))}
            </ul>
          )
        }
      </AsyncSection>
    </>
  )
}
