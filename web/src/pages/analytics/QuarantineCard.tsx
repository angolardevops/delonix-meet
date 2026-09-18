/**
 * Quarentena de reuniões: quem mais fica em quarentena por não responder
 * (aceitar ou recusar) a convites, por período. O âmbito é a organização
 * activa — o servidor filtra, nunca vê organizações de outros.
 *
 * LACUNA CONHECIDA (renomeação de 2026-09-16): o antigo
 * `/api/quarantine/analytics?org_id=` (GET) aceitava o `org_id` como query
 * OPCIONAL, e sem ele agregava todas as organizações que a pessoa administra.
 * O novo `/api/orgs/{org_id}/analytics/quarantine` (GET) tem o `org_id` como
 * segmento OBRIGATÓRIO e não tem forma cruzada. Enquanto não houver endpoint
 * para o âmbito «todas», esse âmbito não faz pedido nenhum — em vez de chamar
 * um caminho que daria 404 garantido.
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
type Scope = 'org' | 'all'

export function QuarantineCard({ orgId }: { orgId: string | null }) {
  const { t } = useTranslation()
  const [period, setPeriod] = useState<Period>('month')
  const [scope, setScope] = useState<Scope>('org')
  const effectiveOrg = scope === 'org' && orgId ? orgId : null
  const { state, reload } = useAsync(
    () =>
      effectiveOrg
        ? quarantineAnalytics(period, effectiveOrg).catch(forbiddenAsMessage(t('analytics.semPermissao')))
        : Promise.resolve([] as QuarantineRow[]),
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
                searchNs="quar."
              />
            )
          }
        </AsyncSection>
      </div>
    </Card>
  )
}
