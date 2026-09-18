/**
 * Quarentena de reuniões: quem mais fica em quarentena por não responder
 * (aceitar ou recusar) a convites, por período. O âmbito é a organização
 * activa — o servidor filtra, nunca vê organizações de outros.
 *
 * O selector «todas as minhas organizações» saiu com a reorganização de rotas
 * (6a854af): a quarentena passou a ser um sub-recurso da organização
 * (`/api/orgs/{org_id}/analytics/quarantine`) e a agregação cross-org, que era
 * o que a ausência do `org_id` pedia, deixou de ter endpoint. Um controlo que
 * não muda o pedido é pior do que não existir — por isso saiu daqui também.
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
    [period, orgId],
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
