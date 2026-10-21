/**
 * «Chamadas externas»: o registo de chamadas, das mais recentes para trás, com
 * «carregar mais» enquanto o servidor der um cursor. Uma chamada sem custo diz
 * porquê em vez de mostrar zero.
 */
import { useTranslation } from 'react-i18next'
import type { CallRecord } from '../../api'
import { Async, AsyncSection } from '../../components/AsyncSection'
import { Icon } from '../../ui/icons'
import { Card, StatusBadge } from '../../ui/kit'
import { formatDateTime, useLocaleTag } from '../admin/orgShared'
import { fmtDuration } from '../admin/VoiceCard'
import { formatMoney } from './format'
import { LoadMore, NA, useTelecomText } from './shared'
import type { PagedList } from './usePaged'

export default function CallsCard({
  state,
  reload,
  loadMore,
  busy,
  err,
}: {
  state: Async<PagedList<CallRecord>>
  reload: () => void
  loadMore: () => void
  busy: boolean
  err: string
}) {
  const { t } = useTranslation()
  const locale = useLocaleTag()
  const { reason, label } = useTelecomText()
  return (
    <Card title={t('telecom.chamadas.titulo')} eyebrow={t('telecom.chamadas.eyebrow')} flush className="tel-card">
      <AsyncSection state={state} onRetry={reload}>
        {(d) =>
          d.items.length === 0 ? (
            <p className="dx-muted tel-note">{t('telecom.chamadas.vazio')}</p>
          ) : (
            <>
              <div className="dx-table-wrap">
                <table className="dx-table tel-table" data-testid="tel-calls">
                  <caption className="dx-sr-only">{t('telecom.chamadas.titulo')}</caption>
                  <thead>
                    <tr>
                      <th scope="col">{t('telecom.chamadas.colSentido')}</th>
                      <th scope="col">{t('telecom.chamadas.colNumero')}</th>
                      <th scope="col">{t('telecom.chamadas.colVia')}</th>
                      <th scope="col">{t('telecom.chamadas.colDesfecho')}</th>
                      <th scope="col">{t('telecom.chamadas.colQuando')}</th>
                      <th scope="col">{t('telecom.chamadas.colDuracao')}</th>
                      <th scope="col">{t('telecom.chamadas.colCusto')}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {d.items.map((c) => (
                      <tr key={c.id}>
                        <td className="org-nowrap">
                          <span className="tel-dir">
                            <Icon name={c.direction === 'inbound' ? 'download' : c.direction === 'outbound' ? 'upload' : 'phone'} size={12} />
                            {label('sentido', c.direction)}
                          </span>
                        </td>
                        <td className="dx-num org-nowrap">{(c.direction === 'inbound' ? c.from_number : c.to_number) || NA}</td>
                        <td>
                          <span className="tel-stack">
                            <span>{c.trunk_name || NA}</span>
                            {c.destination_label && <span className="dx-muted tel-small">{c.destination_label}</span>}
                          </span>
                        </td>
                        <td>
                          <span className="tel-stack">
                            <span>{label('desfecho', c.outcome)}</span>
                            {(c.emergency || c.recorded) && (
                              <span className="tel-badges">
                                {c.emergency && (
                                  <StatusBadge tone="warning" icon="alert">
                                    {t('telecom.plano.emergencia')}
                                  </StatusBadge>
                                )}
                                {c.recorded && <StatusBadge tone="record">{t('telecom.chamadas.gravada')}</StatusBadge>}
                              </span>
                            )}
                          </span>
                        </td>
                        <td className="dx-num dx-muted org-nowrap">{formatDateTime(c.started_at, locale)}</td>
                        <td className="dx-num">{fmtDuration(c.billsec)}</td>
                        <td className="dx-num org-nowrap">
                          {c.cost ? (
                            formatMoney(c.cost, locale)
                          ) : (
                            <span className="dx-muted">{c.cost_reason ? reason(c.cost_reason) : t('telecom.chamadas.semCusto')}</span>
                          )}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
              <LoadMore next={d.next} busy={busy} err={err} onMore={loadMore} />
            </>
          )
        }
      </AsyncSection>
    </Card>
  )
}
