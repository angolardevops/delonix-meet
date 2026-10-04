/**
 * «Consumo do mês»: o total em kwanzas, os minutos e a repartição por
 * operadora. Sem câmbio o servidor não soma moedas diferentes: o total vem a
 * null com a razão, e é a razão que se mostra.
 */
import { useTranslation } from 'react-i18next'
import type { TelephonyUsage } from '../../api'
import { Async, AsyncSection } from '../../components/AsyncSection'
import { Card, Meter } from '../../ui/kit'
import { useLocaleTag } from '../admin/orgShared'
import { formatMoney, formatNumber, measured } from './format'
import { useTelecomText } from './shared'

export default function UsageCard({ state, reload }: { state: Async<TelephonyUsage>; reload: () => void }) {
  const { t } = useTranslation()
  const locale = useLocaleTag()
  const { reason } = useTelecomText()
  const month = state.s === 'ready' ? `${state.d.month} · ${state.d.timezone}` : undefined
  return (
    <Card title={t('telecom.consumo.titulo')} eyebrow={month} className="tel-card">
      <AsyncSection state={state} onRetry={reload}>
        {(u) => (
          <div className="tel-usage" data-testid="tel-usage">
            <div className="tel-usage__total">
              {u.total_aoa ? (
                <strong className="dx-num tel-usage__big">{formatMoney(u.total_aoa, locale)}</strong>
              ) : (
                <span className="dx-muted">
                  {t('telecom.consumo.semTotal')}
                  {u.total_aoa_reason ? ` · ${reason(u.total_aoa_reason)}` : ''}
                </span>
              )}
              <span className="dx-muted dx-num tel-small">
                {t('telecom.consumo.minutos', { count: u.minutes, n: formatNumber(u.minutes, locale) })}
                {' · '}
                {t('telecom.consumo.chamadas', { count: u.calls, n: formatNumber(u.calls, locale) })}
              </span>
            </div>
            {u.totals.length > 0 && (!u.total_aoa || u.totals.some((m) => m.currency !== 'AOA')) && (
              <p className="dx-muted tel-small">
                {t('telecom.consumo.porMoeda')} <span className="dx-num">{u.totals.map((m) => formatMoney(m, locale)).join(' · ')}</span>
              </p>
            )}
            {u.unpriced_calls > 0 && <p className="dx-muted tel-small">{t('telecom.consumo.semPreco', { count: u.unpriced_calls })}</p>}
            {u.by_trunk.length === 0 ? (
              <p className="dx-muted tel-small">{t('telecom.consumo.vazio')}</p>
            ) : (
              <ul className="tel-usage__list">
                {u.by_trunk.map((b, i) => {
                  const share = measured(b.share_pct)
                  return (
                    <li key={b.trunk_id ?? `sem-${i}`}>
                      <div className="tel-usage__row">
                        <span>{b.trunk_name || t('telecom.consumo.semOperadora')}</span>
                        <span className="dx-num">
                          {share === null ? (
                            <span className="dx-muted">{t('telecom.consumo.semQuota')}</span>
                          ) : (
                            t('telecom.pct', { v: formatNumber(share, locale, 1) })
                          )}
                        </span>
                      </div>
                      {share !== null && <Meter value={share} />}
                      <div className="dx-muted dx-num tel-small">
                        {t('telecom.consumo.minutos', { count: b.minutes, n: formatNumber(b.minutes, locale) })}
                        {' · '}
                        {b.cost_aoa
                          ? formatMoney(b.cost_aoa, locale)
                          : b.cost.length > 0
                            ? b.cost.map((m) => formatMoney(m, locale)).join(' · ')
                            : t('telecom.chamadas.semCusto')}
                      </div>
                    </li>
                  )
                })}
              </ul>
            )}
          </div>
        )}
      </AsyncSection>
    </Card>
  )
}
