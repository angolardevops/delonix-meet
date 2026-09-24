/**
 * Quatro indicadores dos últimos 30 dias, com variação face aos 30 anteriores.
 *
 * A qualidade prefere SEMPRE a pontuação MEDIDA (Delonix Call Quality Score,
 * 0–100). O 0–5 é um proxy DERIVADO da distribuição de perdas, usado só quando
 * nenhum cliente reporta a pontuação — e mostra-se noutra escala de propósito,
 * para um número inferido nunca passar por um número medido.
 */
import { useTranslation } from 'react-i18next'
import { OrgStats } from '../../api'
import { Icon } from '../../ui/icons'
import { cx, Tag } from '../../ui/kit'
import { delta, useNumFmt } from './format'

function Delta({ d }: { d: number | null }) {
  const { t } = useTranslation()
  if (d == null) return null
  const dir = d > 0 ? 'up' : d < 0 ? 'down' : 'flat'
  return (
    <span className={cx('an-delta', `an-delta--${dir}`)} title={t('analytics.kpi.vsAnterior')}>
      {dir !== 'flat' && <Icon name={dir === 'up' ? 'chevronUp' : 'chevronDown'} size={11} />}
      <span className="dx-num">{t('analytics.kpi.delta', { v: d > 0 ? `+${d}` : String(d) })}</span>
      <span className="dx-sr-only">{t('analytics.kpi.vsAnterior')}</span>
    </span>
  )
}

export function KpiRow({ s }: { s: OrgStats }) {
  const { t } = useTranslation()
  const { n } = useNumFmt()
  const mid = Math.max(0, 100 - s.pct_good - s.pct_poor)
  const proxy = s.quality_samples_30d > 0 ? (s.pct_good * 5 + mid * 3.5 + s.pct_poor * 1.5) / 100 : null
  const measured = s.avg_score
  const qualityValue =
    measured != null
      ? t('analytics.kpi.de100', { v: n(measured) })
      : proxy != null
        ? t('analytics.kpi.de5', { v: n(proxy, 1) })
        : '—'

  return (
    <div className="an-kpis">
      <div className="an-kpi">
        <span className="an-kpi__label">{t('analytics.kpi.reunioes')}</span>
        <span className="an-kpi__value dx-num">{n(s.meetings_30d)}</span>
        <Delta d={delta(s.meetings_30d, s.meetings_prev_30d)} />
      </div>
      <div className="an-kpi">
        <span className="an-kpi__label">{t('analytics.kpi.minutos')}</span>
        <span className="an-kpi__value dx-num">{n(s.meeting_minutes_30d)}</span>
        <Delta d={delta(s.meeting_minutes_30d, s.meeting_minutes_prev_30d)} />
      </div>
      <div className="an-kpi">
        <span className="an-kpi__label">{t('analytics.kpi.activos')}</span>
        <span className="an-kpi__value dx-num">
          {n(s.active_users_30d)}
          <span className="an-kpi__of">{t('analytics.kpi.deTotal', { total: n(s.members_total) })}</span>
        </span>
        <Delta d={delta(s.active_users_30d, s.active_users_prev_30d)} />
      </div>
      <div className="an-kpi">
        <span className="an-kpi__label">{t('analytics.kpi.qualidade')}</span>
        <span className="an-kpi__value dx-num">{qualityValue}</span>
        <span className="an-kpi__foot">
          {measured == null && proxy != null && <Tag plain>{t('analytics.kpi.estimada')}</Tag>}
          {(measured != null || proxy != null) &&
            (s.avg_loss_pct < 5 ? (
              <Tag tone="success">{t('analytics.kpi.estavel')}</Tag>
            ) : (
              <Tag tone="live">{t('analytics.kpi.instavel')}</Tag>
            ))}
        </span>
      </div>
    </div>
  )
}
