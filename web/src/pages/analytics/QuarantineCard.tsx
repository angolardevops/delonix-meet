/**
 * Quarentena de reuniões: quem mais fica em quarentena por não responder
 * (aceitar ou recusar) a convites, por período. O âmbito é a organização
 * activa ou todas as que a pessoa administra — o servidor filtra, nunca vê
 * organizações de outros.
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
type Scope = 'org' | 'all'

export function QuarantineCard({ orgId }: { orgId: string | null }) {
  const { t } = useTranslation()
  const [period, setPeriod] = useState<Period>('month')
  const [scope, setScope] = useState<Scope>('org')
  const effectiveOrg = scope === 'org' && orgId ? orgId : undefined
  const { state, reload } = useAsync(
    () => quarantineAnalytics(period, effectiveOrg).catch(forbiddenAsMessage(t('analytics.semPermissao'))),
    [period, effectiveOrg],
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
          {orgId && (
            <Segmented
              label={t('analytics.quarentena.ambito')}
              value={scope}
              onChange={setScope}
              options={[
                { value: 'org', label: t('analytics.quarentena.estaOrg') },
                { value: 'all', label: t('analytics.quarentena.todasOrgs') },
              ]}
            />
          )}
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
