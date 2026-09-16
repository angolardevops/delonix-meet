/**
 * Actividade por semana: reuniões e minutos, cada série à sua escala. Barras
 * em CSS (como o cartão «Capacidade de sessões» do template) em vez do
 * Chart.js: seguem os tokens nos dois temas e não pedem um script a mais. A
 * tabela escondida dá os mesmos números a quem usa leitor de ecrã.
 */
import { useTranslation } from 'react-i18next'
import { OrgStats } from '../../api'
import { Card, Empty } from '../../ui/kit'
import { bytesParts, useNumFmt } from './format'

export function UsageCard({ s }: { s: OrgStats }) {
  const { t } = useTranslation()
  const { n, week } = useNumFmt()
  const weeks = s.meetings_per_week
  const maxCount = Math.max(1, ...weeks.map((w) => w.count))
  const maxMin = Math.max(1, ...weeks.map((w) => w.minutes))
  const bytes = bytesParts(s.recordings_bytes)

  return (
    <Card title={t('analytics.uso.titulo')} eyebrow={t('analytics.uso.semanas', { count: weeks.length })} className="an-card an-card--wide">
      {weeks.length === 0 ? (
        <Empty icon="chart" title={t('analytics.uso.vazio')} />
      ) : (
        <>
          <div className="an-legend" aria-hidden="true">
            <span className="an-legend__item">
              <span className="an-swatch an-swatch--count" />
              {t('analytics.uso.reunioes')}
            </span>
            <span className="an-legend__item">
              <span className="an-swatch an-swatch--min" />
              {t('analytics.uso.minutos')}
            </span>
          </div>
          <div className="an-weeks-scroll">
            <div className="an-weeks" aria-hidden="true">
              {weeks.map((w) => (
                <div key={w.week_start} className="an-week" title={t('analytics.uso.dica', { semana: week(w.week_start), reunioes: n(w.count), minutos: n(w.minutes) })}>
                  <span className="an-week__value dx-num">{n(w.count)}</span>
                  <span className="an-week__bars">
                    <span className="an-bar an-bar--count" style={{ height: `${(w.count / maxCount) * 100}%` }} />
                    <span className="an-bar an-bar--min" style={{ height: `${(w.minutes / maxMin) * 100}%` }} />
                  </span>
                  <span className="an-week__label dx-num">{week(w.week_start)}</span>
                </div>
              ))}
            </div>
          </div>
          <table className="dx-sr-only">
            <caption>{t('analytics.uso.titulo')}</caption>
            <thead>
              <tr>
                <th scope="col">{t('analytics.uso.semana')}</th>
                <th scope="col">{t('analytics.uso.reunioes')}</th>
                <th scope="col">{t('analytics.uso.minutos')}</th>
              </tr>
            </thead>
            <tbody>
              {weeks.map((w) => (
                <tr key={w.week_start}>
                  <th scope="row">{week(w.week_start)}</th>
                  <td>{w.count}</td>
                  <td>{w.minutes}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </>
      )}
      <dl className="an-substats">
        <div>
          <dt>{t('analytics.uso.duracaoMedia')}</dt>
          <dd className="dx-num">{t('analytics.uso.minutosValor', { v: n(s.avg_duration_min) })}</dd>
        </div>
        <div>
          <dt>{t('analytics.uso.videoVoz')}</dt>
          <dd className="dx-num">{t('analytics.uso.videoVozValor', { video: n(s.video_30d), voz: n(s.voice_30d) })}</dd>
        </div>
        <div>
          <dt>{t('analytics.uso.gravacoes')}</dt>
          <dd className="dx-num">
            {t(bytes.unit === 'gb' ? 'analytics.uso.gravacoesGb' : 'analytics.uso.gravacoesMb', {
              count: s.recordings_total,
              n: n(s.recordings_total),
              v: n(bytes.v, bytes.unit === 'gb' ? 1 : 0),
            })}
          </dd>
        </div>
      </dl>
    </Card>
  )
}
