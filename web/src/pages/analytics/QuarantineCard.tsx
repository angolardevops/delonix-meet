/**
 * Quarentena de reuniões: quem mais fica em quarentena por não responder
 * (aceitar ou recusar) a convites, por período. O âmbito é a organização
 * activa — o servidor filtra, nunca vê organizações de outros.
 *
 * O selector «esta organização / todas as minhas» saiu com a renomeação de
 * rotas de 2026-09-16: o `org_id` deixou de ser query opcional e passou a ser
 * segmento do caminho (`/api/orgs/{org_id}/analytics/quarantine`), e com ele o
 * servidor perdeu o modo «todas as minhas organizações». Um botão que já não
 * tem endpoint por trás é pior do que botão nenhum.
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { quarantineAnalytics } from '../../api'
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
        : Promise.resolve([]),
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
              />
            )
          }
        </AsyncSection>
      </div>
    </Card>
  )
}
