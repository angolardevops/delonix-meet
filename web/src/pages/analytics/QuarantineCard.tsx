/**
 * Quarentena de reuniões: quem mais fica em quarentena por não responder
 * (aceitar ou recusar) a convites, por período. O âmbito é SEMPRE a
 * organização activa.
 *
 * Havia aqui um selector «esta organização / todas as que administro». A
 * análise passou a ser um sub-recurso da organização
 * (GET `/api/orgs/{org_id}/analytics/quarantine`) e o servidor responde por
 * uma organização de cada vez: o âmbito «todas» deixou de ter endpoint. Um
 * botão que o cliente carrega e o sistema ignora é pior do que botão nenhum —
 * por isso sai daqui em vez de ficar a mentir.
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { QuarantineRow, quarantineAnalytics } from '../../api'
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
        ? quarantineAnalytics(period, orgId).catch(forbiddenAsMessage(t('analytics.semPermissao')))
        : Promise.resolve([] as QuarantineRow[]),
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
