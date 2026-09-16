/**
 * Integrações (`#/integrations`): Odoo, armazenamento da plataforma,
 * webhooks, chaves de API e SSO OIDC. Cada cartão carrega o seu pedido e
 * trata o seu 403 — esconder um cartão não é autorização, o servidor é que
 * decide, e o ecrã mostra o que ele responde.
 *
 * Barra: a instância é o host onde a consola está servida (e o domínio da
 * organização, quando existe); «Registo de eventos» é o registo de auditoria
 * da organização; «Nova integração» não inventa tipos — escolhe um dos cinco
 * cartões que existem e leva lá.
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import PageBar from '../components/PageBar'
import { useShell } from '../components/shellContext'
import { IconName, Icon } from '../ui/icons'
import { Alert, Button, Dialog, Empty, Skeleton } from '../ui/kit'
import '../ui/integrations.css'
import '../ui/org.css'
import AuditCard from './admin/AuditCard'
import { ApiKeysCard } from './integrations/ApiKeysCard'
import { OdooCard } from './integrations/OdooCard'
import { SsoCard } from './integrations/SsoCard'
import { StorageCard } from './integrations/StorageCard'
import { WebhooksCard } from './integrations/WebhooksCard'

type Slot = 'odoo' | 'storage' | 'webhooks' | 'apiKeys' | 'sso'

const SLOTS: { key: Slot; icon: IconName; title: string; sub: string; needsOrg: boolean }[] = [
  { key: 'odoo', icon: 'building', title: 'integrations.odoo.titulo', sub: 'integrations.odoo.sub', needsOrg: true },
  { key: 'storage', icon: 'database', title: 'integrations.storage.titulo', sub: 'integrations.storage.sub', needsOrg: false },
  { key: 'webhooks', icon: 'share', title: 'integrations.webhooks.titulo', sub: 'integrations.webhooks.sub', needsOrg: true },
  { key: 'apiKeys', icon: 'key', title: 'integrations.apiKeys.titulo', sub: 'integrations.apiKeys.sub', needsOrg: true },
  { key: 'sso', icon: 'lock', title: 'integrations.sso.titulo', sub: 'integrations.sso.sub', needsOrg: true },
]

/** Leva ao cartão e põe o foco no primeiro campo dele. */
function goToSlot(key: Slot) {
  const card = document.getElementById(`integ-${key}`)?.firstElementChild as HTMLElement | null
  if (!card) return
  card.scrollIntoView({ behavior: 'smooth', block: 'start' })
  const field = card.querySelector<HTMLElement>('input:not([type=hidden]), select, textarea, button')
  field?.focus({ preventScroll: true })
}

export default function Integrations() {
  const { t } = useTranslation()
  const { org, orgs, isAdmin } = useShell()
  const [auditOpen, setAuditOpen] = useState(false)
  const [pickerOpen, setPickerOpen] = useState(false)

  const instance = [t('consola.integracoes.instancia', { host: location.host }), org?.domain].filter(Boolean).join(' · ')
  const slots = SLOTS.filter((s) => !s.needsOrg || org)

  return (
    <>
      <PageBar title={t('integrations.titulo')} meta={<span data-testid="integ-meta">{[org?.name, instance].filter(Boolean).join(' · ')}</span>}>
        {org && isAdmin && (
          <Button variant="secondary" size="sm" icon="list" onClick={() => setAuditOpen(true)} aria-label={t('consola.integracoes.registo')}>
            <span className="integ-hide-narrow">{t('consola.integracoes.registo')}</span>
          </Button>
        )}
        <Button variant="primary" size="sm" icon="plus" onClick={() => setPickerOpen(true)} aria-label={t('consola.integracoes.nova')}>
          <span className="integ-hide-narrow">{t('consola.integracoes.nova')}</span>
        </Button>
      </PageBar>
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
              {org && (
                <div id="integ-odoo" className="integ-slot">
                  <OdooCard orgId={org.id} />
                </div>
              )}
              <div id="integ-storage" className="integ-slot">
                <StorageCard orgId={org?.id} />
              </div>
              {org && (
                <div id="integ-webhooks" className="integ-slot">
                  <WebhooksCard orgId={org.id} />
                </div>
              )}
              {org && (
                <div id="integ-apiKeys" className="integ-slot">
                  <ApiKeysCard orgId={org.id} />
                </div>
              )}
              {org && (
                <div id="integ-sso" className="integ-slot">
                  <SsoCard orgId={org.id} />
                </div>
              )}
            </div>
          </>
        )}
      </div>

      {auditOpen && org && (
        <Dialog title={t('consola.integracoes.registo')} onClose={() => setAuditOpen(false)} wide>
          <AuditCard orgId={org.id} />
        </Dialog>
      )}

      {pickerOpen && (
        <Dialog title={t('consola.integracoes.nova')} onClose={() => setPickerOpen(false)}>
          <p className="dx-muted integ-picker__intro">{t('consola.integracoes.novaExplica')}</p>
          <ul className="integ-picker" role="list">
            {slots.map((s) => (
              <li key={s.key}>
                <button
                  type="button"
                  className="integ-picker__item"
                  onClick={() => {
                    setPickerOpen(false)
                    // Depois de o diálogo fechar e devolver o foco, leva-o ao cartão.
                    setTimeout(() => goToSlot(s.key), 0)
                  }}
                >
                  <Icon name={s.icon} />
                  <span className="integ-picker__text">
                    <strong>{t(s.title)}</strong>
                    <small className="dx-muted">{t(s.sub)}</small>
                  </span>
                  <Icon name="chevronRight" />
                </button>
              </li>
            ))}
          </ul>
        </Dialog>
      )}
    </>
  )
}
