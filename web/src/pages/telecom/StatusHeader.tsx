/**
 * Cabeçalho da Telefonia: o estado do SBC, os canais, as operadoras por
 * estado e a qualidade medida. Uma métrica que o servidor não mediu diz «sem
 * medição» e a razão — nunca um zero.
 */
import { useTranslation } from 'react-i18next'
import type { SipRegistration } from '../../api'
import { StatusBadge } from '../../ui/kit'
import { formatAgo, useLocaleTag } from '../admin/orgShared'
import { formatNumber, measured, sbcTone, trunkCounts } from './format'
import { useTelecomText } from './shared'

export default function StatusHeader({ registration: r }: { registration: SipRegistration }) {
  const { t } = useTranslation()
  const locale = useLocaleTag()
  const { reason, label } = useTelecomText()
  const inUse = measured(r.channels.in_use)
  const jitter = measured(r.quality.jitter_ms)
  const loss = measured(r.quality.loss_pct)
  const mos = measured(r.quality.mos)
  const counts = trunkCounts(r.trunks)
  const semMedicao = <span className="dx-muted">{t('telecom.semMedicao')}</span>
  const ago = formatAgo(r.measured_at, locale)

  return (
    <section className="dx-card tel-status" aria-label={t('telecom.sbc.rotulo')} data-testid="tel-status">
      <div className="tel-status__state">
        <StatusBadge tone={sbcTone(r.state)}>{label('sbc', r.state)}</StatusBadge>
        {r.reasons.length > 0 && (
          <ul className="tel-reasons">
            {r.reasons.map((c) => (
              <li key={c}>{reason(c)}</li>
            ))}
          </ul>
        )}
      </div>
      <dl className="tel-status__facts">
        <div>
          <dt className="dx-muted">{t('telecom.sbc.canais')}</dt>
          <dd className="dx-num">
            {inUse === null ? semMedicao : formatNumber(inUse, locale)}
            <span className="dx-muted"> {t('telecom.sbc.deMax', { max: formatNumber(r.channels.max, locale) })}</span>
          </dd>
        </div>
        <div>
          <dt className="dx-muted">{t('telecom.sbc.operadoras')}</dt>
          <dd className="dx-num">
            {formatNumber(r.trunks.total, locale)}
            {counts.length > 0 && (
              <span className="dx-muted"> · {counts.map((c) => t(`telecom.contagem.${c.state}`, { count: c.n })).join(' · ')}</span>
            )}
          </dd>
        </div>
        <div>
          <dt className="dx-muted">{t('telecom.sbc.jitter')}</dt>
          <dd className="dx-num">{jitter === null ? semMedicao : t('telecom.sbc.ms', { v: formatNumber(jitter, locale, 1) })}</dd>
        </div>
        <div>
          <dt className="dx-muted">{t('telecom.sbc.perda')}</dt>
          <dd className="dx-num">{loss === null ? semMedicao : t('telecom.pct', { v: formatNumber(loss, locale, 1) })}</dd>
        </div>
        <div>
          <dt className="dx-muted">{t('telecom.sbc.mos')}</dt>
          <dd className="dx-num">{mos === null ? semMedicao : formatNumber(mos, locale, 2)}</dd>
        </div>
      </dl>
      <p className="dx-muted tel-status__note">
        {t('telecom.sbc.janela', { horas: r.quality.window_hours, count: r.quality.calls })}
        {r.quality.reason ? ` · ${reason(r.quality.reason)}` : ''}
        {ago ? ` · ${t('telecom.medido', { quando: ago })}` : ''}
      </p>
    </section>
  )
}
