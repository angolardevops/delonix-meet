/**
 * O inventário USB que os agentes reportam e os operadores da plataforma.
 *
 * O frontend não decide política: «capaz», «online» e a razão de um aparelho
 * não enviar vêm do servidor. Um aparelho que nunca vai enviar não leva botão
 * — um botão primário desactivado continuava a parecer clicável, e a razão já
 * está à vista na linha.
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { listSmsDevices, setSmsRoute, SmsDevice, SmsRoute } from '../../api'
import { Async, AsyncSection } from '../../components/AsyncSection'
import { Icon } from '../../ui/icons'
import { Alert, Button, StatusBadge } from '../../ui/kit'
import { orgErrorMessage } from './orgShared'
import { SMS_POLL_MS, usePolled } from './smsShared'

export function SmsDevices({ orgId, onRoute }: { orgId: string; onRoute: (r: SmsRoute) => void }) {
  const { t } = useTranslation()
  const devs = usePolled((signal) => listSmsDevices(orgId, signal), [orgId], SMS_POLL_MS)
  const [busyId, setBusyId] = useState<string | null>(null)
  const [err, setErr] = useState('')

  async function choose(deviceId: string | null, busyKey: string) {
    setErr('')
    setBusyId(busyKey)
    try {
      onRoute(await setSmsRoute(orgId, deviceId))
      devs.refresh()
    } catch (x) {
      // 422: o dispositivo não é capaz — a razão vem do servidor.
      setErr(orgErrorMessage(x, t, 'consola.sms.erroPedido'))
    } finally {
      setBusyId(null)
    }
  }

  const kindLabel: Record<SmsDevice['kind'], string> = {
    modem: t('consola.sms.tipoModem'),
    android_adb: t('consola.sms.tipoAndroidAdb'),
    android_mtp: t('consola.sms.tipoAndroidMtp'),
    mass_storage_modem: t('consola.sms.tipoPen'),
    unknown: t('consola.sms.tipoDesconhecido'),
  }
  const transportLabel: Record<SmsDevice['transport'], string> = {
    at_serial: t('consola.sms.transporteAt'),
    modemmanager: t('consola.sms.transporteMm'),
    none: t('consola.sms.transporteNenhum'),
  }

  return (
    <>
      <h3 className="org-voice__sub">{t('consola.sms.dispositivos')}</h3>
      <p className="dx-muted org-card-note">{t('consola.sms.dispositivosNota')}</p>
      {err && (
        <div className="org-card-pad">
          <Alert tone="danger">{err}</Alert>
        </div>
      )}
      <AsyncSection state={devs.state} onRetry={devs.reload}>
        {(list) =>
          list.length === 0 ? (
            <p className="dx-muted org-card-note">{t('consola.sms.semDispositivos')}</p>
          ) : (
            <ul className="org-simple org-sms__list" data-testid="sms-devices">
              {list.map((d) => {
                const usable = d.capable && d.online
                const whyId = `sms-dev-why-${d.id}`
                return (
                  <li key={d.id} className={d.selected ? 'org-sms__row org-sms__row--selected' : 'org-sms__row'}>
                    <span className="org-simple__main">
                      <strong className="org-break">{d.product || d.manufacturer || kindLabel.unknown}</strong>
                      <span className="dx-muted org-break">
                        {[
                          d.product && d.manufacturer,
                          `${d.vendor_id}:${d.product_id}`,
                          kindLabel[d.kind] ?? kindLabel.unknown,
                          transportLabel[d.transport] ?? transportLabel.none,
                          d.gateway_name,
                        ]
                          .filter(Boolean)
                          .join(' · ')}
                      </span>
                      {(d.operator_name || d.signal_percent !== null) && (
                        <span className="dx-muted">
                          {d.operator_name ?? t('consola.sms.operadorDesconhecido')}
                          {d.signal_percent !== null && ` · ${t('consola.sms.sinal', { pct: d.signal_percent })}`}
                        </span>
                      )}
                      {!d.capable && (
                        <span className="org-sms__reason">
                          <Icon name="alert" size={12} />
                          <span>{d.reason || t('consola.sms.naoCapaz')}</span>
                        </span>
                      )}
                      {d.capable && !usable && !d.selected && (
                        <span id={whyId} className="dx-muted">
                          {t('consola.sms.esperaOnline')}
                        </span>
                      )}
                    </span>
                    <span className="org-sms__side">
                      <StatusBadge tone={d.online ? 'success' : 'neutral'}>
                        {d.online ? t('consola.sms.online') : t('consola.sms.offline')}
                      </StatusBadge>
                      {d.selected && <StatusBadge tone="success" icon="check">{t('consola.sms.emUso')}</StatusBadge>}
                      {d.selected ? (
                        <Button size="sm" variant="ghost" busy={busyId === 'none'} disabled={busyId !== null} onClick={() => void choose(null, 'none')}>
                          {t('consola.sms.deixarDeUsar')}
                        </Button>
                      ) : d.capable ? (
                        <Button
                          size="sm"
                          variant="primary"
                          busy={busyId === d.id}
                          disabled={!usable || busyId !== null}
                          aria-describedby={!usable ? whyId : undefined}
                          onClick={() => void choose(d.id, d.id)}
                        >
                          {t('consola.sms.usar')}
                        </Button>
                      ) : (
                        <StatusBadge tone="neutral" icon="ban">
                          {t('consola.sms.naoEnvia')}
                        </StatusBadge>
                      )}
                    </span>
                  </li>
                )
              })}
            </ul>
          )
        }
      </AsyncSection>
    </>
  )
}

export function SmsOperators({ route, onRetry }: { route: Async<SmsRoute>; onRetry: () => void }) {
  const { t } = useTranslation()
  return (
    <>
      <h3 className="org-voice__sub">{t('consola.sms.operadores')}</h3>
      <p className="dx-muted org-card-note">{t('consola.sms.operadoresNota')}</p>
      <AsyncSection state={route} onRetry={onRetry}>
        {(r) =>
          r.operators.length === 0 ? (
            <p className="dx-muted org-card-note">{t('consola.sms.semOperadores')}</p>
          ) : (
            <ul className="org-simple" data-testid="sms-operators">
              {r.operators.map((o) => (
                <li key={o.operator}>
                  <span className="org-simple__main">
                    <strong>{o.label}</strong>
                    <span className="dx-muted dx-num">{t('consola.sms.prefixos', { lista: o.prefixes.join(', ') })}</span>
                  </span>
                  <StatusBadge tone={o.configured ? 'success' : 'neutral'}>
                    {o.configured ? t('consola.sms.configurado') : t('consola.sms.porContratar')}
                  </StatusBadge>
                </li>
              ))}
            </ul>
          )
        }
      </AsyncSection>
      <p className="dx-muted org-card-note">
        <Icon name="alert" size={12} /> {t('consola.sms.prefixosPorConfirmar')}
      </p>
    </>
  )
}
