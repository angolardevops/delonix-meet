/**
 * Envio avulso de SMS e o histórico da organização (ADR-0005).
 *
 * O `SmsGatewayCard` trata do CAMINHO (aparelhos, encaminhamento, operadores);
 * o `SmsDialog` do directório envia a um CONTACTO sem mostrar o número. Faltava
 * o terceiro modo que o servidor já serve: o admin escrever um número à mão
 * (o modo «to + body» de `sendSmsToNumber`) e ver o que aconteceu à mensagem.
 *
 * O contador é o mesmo espelho do `sms_codec` que o directório usa — aqui sem
 * prefixo, porque no envio avulso o corpo vai tal como foi escrito. «Enviada»
 * quer dizer aceite pelo aparelho ou pelo operador: não há recibo de entrega.
 */
import { FormEvent, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ApiError, listSmsMessages, sendSmsToNumber, SmsMessage, SmsStatus } from '../../api'
import { AsyncSection, useAsync } from '../../components/AsyncSection'
import { Alert, BadgeTone, Button, Card, Field, Segmented, StatusBadge, TextArea, TextInput } from '../../ui/kit'
import { countSms, SMS_MAX_SEGMENTS } from '../directory/smsCount'
import { formatDateTime, orgErrorMessage, refusalAware, useLocaleTag } from './orgShared'

type Route = 'auto' | 'usb' | 'operator'

const SHOWN = 20
const POLL_MS = 5000

const STATUS_TONE: Record<SmsStatus, BadgeTone> = {
  queued: 'neutral',
  claimed: 'warning',
  sent: 'success',
  failed: 'record',
}

/** `randomUUID` só existe em contexto seguro; fora dele a chave continua única o suficiente. */
function newKey(): string {
  return typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function'
    ? crypto.randomUUID()
    : `sms-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 12)}`
}

export default function SmsSendCard({ orgId }: { orgId: string }) {
  const { t } = useTranslation()
  const locale = useLocaleTag()
  const [to, setTo] = useState('')
  const [body, setBody] = useState('')
  const [route, setRoute] = useState<Route>('auto')
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')
  const [sent, setSent] = useState<SmsMessage | null>(null)
  // Uma chave por INTENÇÃO de envio: muda quando o destino, o texto ou a rota mudam.
  const key = useRef(newKey())
  const messages = useAsync(
    (signal) => refusalAware(listSmsMessages(orgId, SHOWN, signal), t).then((r) => r.items),
    [orgId],
  )

  const count = useMemo(() => countSms(body), [body])
  const tooLong = count.segments > SMS_MAX_SEGMENTS
  const incomplete = !to.trim() || !body.trim()

  // Só se repete a leitura enquanto houver alguma em fila ou a enviar, e com o
  // separador à vista. O `useAsync` não volta ao esqueleto num `reload`.
  const pending = messages.state.s === 'ready' && messages.state.d.some((m) => m.status === 'queued' || m.status === 'claimed')
  const { reload } = messages
  useEffect(() => {
    if (!pending) return
    const id = window.setInterval(() => {
      if (!document.hidden) reload()
    }, POLL_MS)
    return () => window.clearInterval(id)
  }, [pending, reload])

  function intentChanged() {
    key.current = newKey()
    setErr('')
    setSent(null)
  }

  async function submit(e: FormEvent) {
    e.preventDefault()
    if (busy || incomplete || tooLong) return
    setBusy(true)
    setErr('')
    setSent(null)
    try {
      const m = await sendSmsToNumber(orgId, { to: to.trim(), body, route }, key.current)
      setSent(m)
      setBody('')
      key.current = newKey()
      reload()
    } catch (x) {
      // Destino inválido ou sem rota: a razão, por extenso, vem do servidor.
      setErr(x instanceof ApiError && x.status === 429 ? t('org.sms.erro.limite') : orgErrorMessage(x, t, 'org.sms.erro.falhou'))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Card title={t('org.sms.avulso.titulo')} eyebrow={t('org.sms.avulso.eyebrow')} as="section">
      <form className="org-form org-card-pad" onSubmit={(e) => void submit(e)} aria-label={t('org.sms.avulso.titulo')}>
        <Field label={t('org.sms.avulso.destino')} htmlFor="sms-avulso-to">
          <TextInput
            id="sms-avulso-to"
            type="tel"
            inputMode="tel"
            autoComplete="off"
            className="dx-num"
            placeholder={t('org.sms.avulso.destinoPh')}
            value={to}
            onChange={(e) => {
              setTo(e.target.value)
              intentChanged()
            }}
            required
          />
        </Field>
        <Field
          label={t('org.sms.mensagem')}
          htmlFor="sms-avulso-body"
          aside={
            <span className="dx-num dx-muted" data-testid="sms-avulso-count" aria-live="polite">
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
            id="sms-avulso-body"
            rows={4}
            value={body}
            onChange={(e) => {
              setBody(e.target.value)
              intentChanged()
            }}
            required
          />
        </Field>
        {count.encoding === 'ucs2' && body !== '' && <p className="dx-muted org-form__hint">{t('org.sms.ucs2Nota')}</p>}
        {tooLong && <Alert tone="warning">{t('org.sms.demasiadoLonga', { max: SMS_MAX_SEGMENTS })}</Alert>}
        <Segmented<Route>
          label={t('org.sms.avulso.rota')}
          value={route}
          onChange={(r) => {
            setRoute(r)
            intentChanged()
          }}
          options={[
            { value: 'auto', label: t('org.sms.avulso.rotaAuto') },
            { value: 'usb', label: t('org.sms.avulso.rotaUsb') },
            { value: 'operator', label: t('org.sms.avulso.rotaOperador') },
          ]}
        />
        {err && (
          <Alert tone="danger">
            <span data-testid="sms-avulso-error">{err}</span>
          </Alert>
        )}
        {sent && (
          <Alert tone="success">
            <span data-testid="sms-avulso-sent">{t('org.sms.avulso.naFila', { to: sent.to, count: sent.segments })}</span>
          </Alert>
        )}
        <div className="org-form__foot">
          <Button type="submit" variant="primary" size="sm" icon="send" busy={busy} disabled={incomplete || tooLong}>
            {t('org.sms.enviar')}
          </Button>
        </div>
      </form>

      <h3 className="org-voice__sub">{t('org.sms.avulso.historico')}</h3>
      <p className="dx-muted org-card-note">{t('org.sms.avulso.historicoNota')}</p>
      <AsyncSection state={messages.state} onRetry={messages.reload}>
        {(list) =>
          list.length === 0 ? (
            <p className="dx-muted org-card-note">{t('org.sms.avulso.semMensagens')}</p>
          ) : (
            <ul className="org-simple" data-testid="sms-avulso-messages">
              {list.map((m) => (
                <li key={m.id}>
                  <span className="org-simple__main">
                    <span>
                      <strong className="dx-num">{m.to}</strong>
                      <span className="dx-muted dx-num"> · {formatDateTime(m.created_at, locale)}</span>
                    </span>
                    <span className="org-break">{m.body}</span>
                    <span className="dx-muted">
                      {[
                        t(`org.sms.avulso.via.${m.route}`),
                        m.operator,
                        t('org.sms.avulso.partes', { count: m.segments, cod: m.encoding === 'gsm7' ? 'GSM-7' : 'UCS-2' }),
                      ]
                        .filter(Boolean)
                        .join(' · ')}
                    </span>
                    {m.error && <span className="org-sms__error org-break">{m.error}</span>}
                  </span>
                  <StatusBadge tone={STATUS_TONE[m.status] ?? 'neutral'}>{t(`org.sms.avulso.estado.${m.status}`)}</StatusBadge>
                </li>
              ))}
            </ul>
          )
        }
      </AsyncSection>
    </Card>
  )
}
