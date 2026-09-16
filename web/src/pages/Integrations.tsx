/**
 * Integrações (`#/integrations`): Odoo, armazenamento da plataforma,
 * webhooks, chaves de API e SSO OIDC. Cada cartão carrega o seu pedido e
 * trata o seu 403 — esconder um cartão não é autorização, o servidor é que
 * decide, e o ecrã mostra o que ele responde.
 */
import { useTranslation } from 'react-i18next'
import PageBar from '../components/PageBar'
import { useShell } from '../components/shellContext'
import { Alert, Empty, Skeleton } from '../ui/kit'
import '../ui/integrations.css'
import { ApiKeysCard } from './integrations/ApiKeysCard'
import { OdooCard } from './integrations/OdooCard'
import { SsoCard } from './integrations/SsoCard'
import { StorageCard } from './integrations/StorageCard'
import { WebhooksCard } from './integrations/WebhooksCard'

export default function Integrations() {
  const { t } = useTranslation()
  const { org, orgs, isAdmin } = useShell()

  return (
    <>
      <PageBar title={t('integrations.titulo')} meta={org?.name} />
      <div className="page integ-page">
        {orgs.s === 'loading' ? (
          <div className="integ-grid" aria-busy="true">
            <Skeleton h={220} />
            <Skeleton h={220} />
          </div>
        ) : orgs.s === 'error' ? (
          <Alert tone="danger">{orgs.msg}</Alert>
        ) : (
          <>
            {!org && (
              <Empty icon="building" title={t('integrations.semOrg')}>
                {t('integrations.semOrgDica')}
              </Empty>
            )}
            {org && !isAdmin && <Alert tone="warning">{t('integrations.naoAdmin', { org: org.name })}</Alert>}
            <div className="integ-grid">
              {org && <OdooCard orgId={org.id} />}
              <StorageCard />
              {org && <WebhooksCard orgId={org.id} />}
              {org && <ApiKeysCard orgId={org.id} />}
              {org && <SsoCard orgId={org.id} />}
            </div>
          </>
        )}
      </div>
    </>
  )
}
