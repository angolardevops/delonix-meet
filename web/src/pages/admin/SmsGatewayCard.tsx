/**
 * Envio de SMS por gateway USB/telefone (`server/src/sms.rs`): os aparelhos
 * emparelhados, o encaminhamento (qual deles recebe os SMS de saída) e os
 * operadores SMPP contratados, só para contexto — não há nada para editar
 * aí, é o servidor que decide se estão configurados.
 *
 * O token de um gateway novo só aparece UMA vez, no momento da criação — a
 * partir daí só existe o hash. Por isso o diálogo de emparelhamento fica
 * aberto até a pessoa confirmar que o copiou.
 */
import { FormEvent, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  createSmsGateway,
  CreatedSmsGateway,
  getSmsRoute,
  listSmsDevices,
  listSmsGateways,
  revokeSmsGateway,
  putSmsRoute,
} from '../../api'
import { AsyncSection, useAsync } from '../../components/AsyncSection'
import { Alert, Button, Card, Dialog, Select, StatusBadge, TextInput } from '../../ui/kit'
import { formatAgo, orgErrorMessage, refusalAware, useLocaleTag } from './orgShared'

export default function SmsGatewayCard({ orgId }: { orgId: string }) {
  const { t } = useTranslation()
  const locale = useLocaleTag()
  const gateways = useAsync((signal) => refusalAware(listSmsGateways(orgId, signal), t), [orgId])
  const devices = useAsync((signal) => refusalAware(listSmsDevices(orgId, signal), t), [orgId])
  const route = useAsync((signal) => refusalAware(getSmsRoute(orgId, signal), t), [orgId])
  const [pairing, setPairing] = useState(false)
  const [busyRevoke, setBusyRevoke] = useState<string | null>(null)
  const [routeBusy, setRouteBusy] = useState(false)
  const [err, setErr] = useState('')

  function reloadAll() {
    gateways.reload()
    devices.reload()
    route.reload()
  }

  async function revoke(id: string) {
    if (!window.confirm(t('org.sms.gateways.confirmarRevogar'))) return
    setBusyRevoke(id)
    setErr('')
    try {
      await revokeSmsGateway(orgId, id)
      reloadAll()
    } catch (e) {
      setErr(orgErrorMessage(e, t, 'ui.erroGenerico'))
    } finally {
      setBusyRevoke(null)
    }
  }

  async function changeRoute(deviceId: string) {
    setRouteBusy(true)
    setErr('')
    try {
      await putSmsRoute(orgId, deviceId || null)
      route.reload()
      devices.reload()
    } catch (e) {
      setErr(orgErrorMessage(e, t, 'ui.erroGenerico'))
    } finally {
      setRouteBusy(false)
    }
  }

  return (
    <Card title={t('org.sms.gateways.titulo')} eyebrow={t('org.sms.gateways.eyebrow')} as="section">
      <div className="org-card-pad" style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
        {err && <Alert tone="danger">{err}</Alert>}
        <AsyncSection state={gateways.state} onRetry={gateways.reload}>
          {(list) =>
            list.length === 0 ? (
              <p className="dx-muted" style={{ margin: 0 }}>{t('org.sms.gateways.semAparelhos')}</p>
            ) : (
              <ul className="org-simple">
                {list.map((g) => (
                  <li key={g.id}>
                    <span className="org-simple__main">
                      <strong>{g.name}</strong>
                      <span className="dx-muted dx-num">
                        {g.prefix}… · {g.online ? t('org.sms.gateways.online') : t('org.sms.gateways.offline', { quando: formatAgo(g.last_seen_at, locale) ?? t('org.sms.gateways.nunca') })}
                      </span>
                    </span>
                    <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                      <StatusBadge tone={g.online ? 'success' : 'neutral'}>
                        {g.online ? t('org.sms.gateways.activo') : t('org.sms.gateways.porLigar')}
                      </StatusBadge>
                      <Button size="sm" variant="secondary" busy={busyRevoke === g.id} onClick={() => revoke(g.id)}>
                        {t('org.sms.gateways.revogar')}
                      </Button>
                    </div>
                  </li>
                ))}
              </ul>
            )
          }
        </AsyncSection>
        <div>
          <Button variant="primary" size="sm" icon="plus" onClick={() => setPairing(true)}>
            {t('org.sms.gateways.emparelhar')}
          </Button>
        </div>

        <h3 className="org-voice__sub">{t('org.sms.gateways.encaminhamento')}</h3>
        <AsyncSection state={route.state} onRetry={route.reload}>
          {(r) => (
            <AsyncSection state={devices.state} onRetry={devices.reload}>
              {(deviceList) => (
                <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                  <Select
                    value={r.device_id ?? ''}
                    disabled={routeBusy}
                    onChange={(e) => void changeRoute(e.target.value)}
                    aria-label={t('org.sms.gateways.encaminhamento')}
                  >
                    <option value="">{t('org.sms.gateways.semDispositivo')}</option>
                    {deviceList.map((d) => (
                      <option key={d.id} value={d.id} disabled={!d.capable}>
                        {d.gateway_name} — {d.operator_name ?? d.product ?? d.device_key}
                        {!d.capable ? ` (${t('org.sms.gateways.incapaz')})` : ''}
                      </option>
                    ))}
                  </Select>
                  <p className="dx-muted" style={{ margin: 0, fontSize: 11 }}>{t('org.sms.gateways.encaminhamentoDica')}</p>
                  {r.operators.length > 0 && (
                    <ul className="org-simple">
                      {r.operators.map((op) => (
                        <li key={op.operator}>
                          <span className="org-simple__main">
                            <strong>{op.label}</strong>
                            <span className="dx-muted dx-num">{op.prefixes.join(', ')}</span>
                          </span>
                          <StatusBadge tone={op.configured ? 'success' : 'neutral'}>
                            {op.configured ? t('org.sms.gateways.contratado') : t('org.sms.gateways.naoContratado')}
                          </StatusBadge>
                        </li>
                      ))}
                    </ul>
                  )}
                </div>
              )}
            </AsyncSection>
          )}
        </AsyncSection>
      </div>
      {pairing && (
        <PairDialog
          orgId={orgId}
          onClose={() => {
            setPairing(false)
            reloadAll()
          }}
        />
      )}
    </Card>
  )
}

function PairDialog({ orgId, onClose }: { orgId: string; onClose: () => void }) {
  const { t } = useTranslation()
  const [name, setName] = useState('')
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')
  const [created, setCreated] = useState<CreatedSmsGateway | null>(null)
  const [copied, setCopied] = useState(false)

  async function submit(e: FormEvent) {
    e.preventDefault()
    setBusy(true)
    setErr('')
    try {
      const g = await createSmsGateway(orgId, name.trim() || undefined)
      setCreated(g)
    } catch (x) {
      setErr(orgErrorMessage(x, t, 'ui.erroGenerico'))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog title={t('org.sms.gateways.emparelhar')} onClose={onClose}>
      {created ? (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
          <Alert tone="warning">{t('org.sms.gateways.tokenAviso')}</Alert>
          <code className="mfa-secret dx-num" style={{ wordBreak: 'break-all' }}>{created.token}</code>
          {err && <Alert tone="danger">{err}</Alert>}
          <Button
            variant="secondary"
            icon="copy"
            onClick={async () => {
              try {
                await navigator.clipboard.writeText(created.token)
                setCopied(true)
              } catch {
                setErr(t('org.sms.gateways.erroCopiar'))
              }
            }}
          >
            {copied ? t('org.sms.gateways.copiado') : t('org.sms.gateways.copiar')}
          </Button>
          <Button variant="primary" disabled={!copied} onClick={onClose}>
            {t('org.sms.gateways.concluido')}
          </Button>
        </div>
      ) : (
        <form onSubmit={submit} style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
          <p className="dx-muted" style={{ margin: 0 }}>{t('org.sms.gateways.emparelharDica')}</p>
          <TextInput
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder={t('org.sms.gateways.nomePh')}
            maxLength={60}
          />
          {err && <Alert tone="danger">{err}</Alert>}
          <div style={{ display: 'flex', gap: 8 }}>
            <Button type="button" variant="secondary" onClick={onClose}>{t('ui.cancelar')}</Button>
            <Button type="submit" variant="primary" busy={busy}>{t('org.sms.gateways.gerarToken')}</Button>
          </div>
        </form>
      )}
    </Dialog>
  )
}
