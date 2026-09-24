/**
 * Análise (`#/analytics`): uso e qualidade da organização activa, tal como o
 * servidor os agrega (`/api/orgs/:id/stats`, só administradores), e a
 * quarentena de convites por período.
 *
 * A GESTÃO da organização (membros, filiais, grupos, auditoria, retenção,
 * quotas, criar organização) vive na Administração; a configuração de
 * integrações vive em Integrações. Aqui só se lê.
 */
import { useTranslation } from 'react-i18next'
import { orgStats } from '../api'
import { AsyncSection, useAsync } from '../components/AsyncSection'
import PageBar from '../components/PageBar'
import { useShell } from '../components/shellContext'
import { Alert, Button, Card, Empty, Skeleton } from '../ui/kit'
import '../ui/analytics.css'
import { forbiddenAsMessage } from './analytics/format'
import { KpiRow } from './analytics/KpiRow'
import { QualityCard } from './analytics/QualityCard'
import { QuarantineCard } from './analytics/QuarantineCard'
import { RankList } from './analytics/RankList'
import { UsageCard } from './analytics/UsageCard'

function StatsSkeleton() {
  return (
    <div className="an-grid" aria-busy="true">
      <div className="an-kpis an-card--wide">
        {[0, 1, 2, 3].map((i) => (
          <Skeleton key={i} h={86} />
        ))}
      </div>
      <Skeleton h={260} />
      <Skeleton h={260} />
    </div>
  )
}

function OrgStatsSection({ orgId }: { orgId: string }) {
  const { t } = useTranslation()
  const { state, reload } = useAsync(() => orgStats(orgId).catch(forbiddenAsMessage(t('analytics.semPermissao'))), [orgId])
  return (
    <AsyncSection state={state} onRetry={reload} skeleton={<StatsSkeleton />}>
      {(s) => (
        <div className="an-grid">
          <div className="an-card--wide">
            <KpiRow s={s} />
          </div>
          <UsageCard s={s} />
          <QualityCard s={s} />
          <Card title={t('analytics.organizadores.titulo')} eyebrow={t('analytics.organizadores.periodo')} className="an-card">
            {s.top_organizers.length === 0 ? (
              <Empty icon="people" title={t('analytics.organizadores.vazio')} />
            ) : (
              <RankList
                label={t('analytics.organizadores.titulo')}
                rows={s.top_organizers.map((o) => ({ key: o.username, name: o.username, count: o.count }))}
                searchNs="org."
              />
            )}
          </Card>
        </div>
      )}
    </AsyncSection>
  )
}

export default function Analytics() {
  const { t } = useTranslation()
  const { org, orgs, isAdmin, navigate } = useShell()

  return (
    <>
      <PageBar title={t('analytics.titulo')} meta={org ? t('analytics.meta', { org: org.name }) : undefined} />
      <div className="page an-page">
        {orgs.s === 'loading' ? (
          <StatsSkeleton />
        ) : orgs.s === 'error' ? (
          <Alert tone="danger">{orgs.msg}</Alert>
        ) : !org ? (
          <Empty icon="building" title={t('analytics.semOrg')}>
            {t('analytics.semOrgDica')}
          </Empty>
        ) : (
          <>
            {!isAdmin && <Alert tone="warning">{t('analytics.naoAdmin', { org: org.name })}</Alert>}
            <OrgStatsSection orgId={org.id} />
            <div className="an-grid">
              <QuarantineCard orgId={org.id} />
            </div>
            <div className="an-footer">
              <span className="dx-muted">{t('analytics.gestaoNaAdmin')}</span>
              <Button size="sm" variant="ghost" icon="chevronRight" onClick={() => navigate('admin')}>
                {t('analytics.irAdmin')}
              </Button>
            </div>
          </>
        )}
      </div>
    </>
  )
}
