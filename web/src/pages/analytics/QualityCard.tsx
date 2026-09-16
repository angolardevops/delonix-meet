/**
 * Qualidade das chamadas (amostras QoS dos últimos 30 dias). Os indicadores
 * que dependem de clientes recentes (pontuação, TURN, CPU) só aparecem quando
 * o servidor os devolve: um «0 %» sem medição por trás parece boa notícia e
 * não é notícia nenhuma.
 */
import { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { OrgStats } from '../../api'
import { Card, cx, Empty } from '../../ui/kit'
import { useNumFmt } from './format'

function Stat({ label, value, tone }: { label: ReactNode; value: ReactNode; tone?: 'good' | 'poor' }) {
  return (
    <div className={cx('an-stat', tone && `an-stat--${tone}`)}>
      <span className="an-stat__value dx-num">{value}</span>
      <span className="an-stat__label">{label}</span>
    </div>
  )
}

export function QualityCard({ s }: { s: OrgStats }) {
  const { t } = useTranslation()
  const { n } = useNumFmt()
  const mid = Math.max(0, 100 - s.pct_good - s.pct_poor)
  const pct = (v: number) => t('analytics.qualidade.pct', { v: n(v, 1) })

  return (
    <Card title={t('analytics.qualidade.titulo')} eyebrow={t('analytics.qualidade.amostras', { count: s.quality_samples_30d })} className="an-card">
      {s.quality_samples_30d === 0 ? (
        <Empty icon="signal" title={t('analytics.qualidade.vazio')}>
          {t('analytics.qualidade.vazioDica')}
        </Empty>
      ) : (
        <div className="an-stack">
          <div className="an-dist">
            {[
              { k: 'good', label: t('analytics.qualidade.boa'), v: s.pct_good },
              { k: 'mid', label: t('analytics.qualidade.media'), v: mid },
              { k: 'poor', label: t('analytics.qualidade.fraca'), v: s.pct_poor },
            ].map((b) => (
              <div key={b.k} className="an-dist__row">
                <span className="an-dist__label">{b.label}</span>
                <span
                  className="an-track"
                  role="meter"
                  aria-label={b.label}
                  aria-valuemin={0}
                  aria-valuemax={100}
                  aria-valuenow={b.v}
                >
                  <span className={cx('an-track__fill', `an-track__fill--${b.k}`)} style={{ width: `${Math.min(100, b.v)}%` }} />
                </span>
                <span className="an-dist__pct dx-num">{pct(b.v)}</span>
              </div>
            ))}
          </div>
          <div className="an-stats">
            <Stat label={t('analytics.qualidade.rtt')} value={s.avg_rtt_ms != null ? t('analytics.qualidade.ms', { v: n(s.avg_rtt_ms) }) : '—'} />
            <Stat label={t('analytics.qualidade.perda')} value={pct(s.avg_loss_pct)} />
            {s.avg_score != null && <Stat label={t('analytics.qualidade.pontuacao')} value={t('analytics.kpi.de100', { v: n(s.avg_score) })} />}
            {s.pct_low_score != null && <Stat label={t('analytics.qualidade.pontuacaoBaixa')} value={pct(s.pct_low_score)} tone="poor" />}
            {s.pct_turn_relay != null && <Stat label={t('analytics.qualidade.turn')} value={pct(s.pct_turn_relay)} />}
            {s.pct_cpu_limited != null && <Stat label={t('analytics.qualidade.cpu')} value={pct(s.pct_cpu_limited)} />}
          </div>
        </div>
      )}
    </Card>
  )
}
