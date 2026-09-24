/**
 * Quarentena de reuniões: quem mais fica em quarentena por não responder
 * (aceitar ou recusar) a convites, por período. O âmbito é UMA organização: a
 * rota de analítica de quarentena passou a exigir o org_id no caminho, e a
 * variante «todas as que a pessoa administra» deixou de existir no servidor
 * com a reorganização de rotas de 2026-09-16 (6a854af).
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { quarantineAnalytics, QuarantineRow } from '../../api'
import { AsyncSection, useAsync } from '../../components/AsyncSection'
import { Card, Empty, Segmented } from '../../ui/kit'
import { forbiddenAsMessage } from './format'
import { RankList } from './RankList'

type Period = 'week' | 'month' | 'quarter' | 'year'
const PERIODS: Period[] = ['week', 'month', 'quarter', 'year']

export function QuarantineCard({ orgId }: { orgId: string | null }) {
  const { t } = useTranslation()
  const [period, setPeriod] = useState<Period>('month')
  const { state, reload } = useAsync(
    () =>
      orgId
        ? quarantineAnalytics(orgId, period).catch(forbiddenAsMessage(t('analytics.semPermissao')))
        : Promise.resolve([] as QuarantineRow[]),
    [orgId, period],
  )

  return (
    <Card title={t('analytics.quarentena.titulo')} className="an-card an-card--wide">
      <div className="an-stack">
        <p className="an-desc">{t('analytics.quarentena.sub')}</p>
        <div className="an-controls">
          <Segmented
            label={t('analytics.quarentena.periodo')}
            value={period}
            onChange={setPeriod}
            options={PERIODS.map((p) => ({ value: p, label: t(`analytics.quarentena.periodos.${p}`) }))}
          />
        </div>
        <AsyncSection state={state} onRetry={reload}>
          {(rows) =>
            rows.length === 0 ? (
              <Empty icon="clock" title={t('analytics.quarentena.vazio')} />
            ) : (
              <RankList
                label={t('analytics.quarentena.titulo')}
                rows={rows.map((r) => ({ key: r.user_id, name: r.username, count: r.count }))}
                searchNs="quar."
              />
            )
          }
        </AsyncSection>
      </div>
    </Card>
  )
}
