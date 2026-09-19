/**
 * "Papéis e permissões": entrada no painel de administração. O cartão só
 * mostra a contagem; a gestão a sério (matriz, herança, pedidos de
 * aprovação) vive no diálogo largo — ver RolesDialog.tsx.
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { listPermissionRequests, listRoles } from '../../api'
import { AsyncSection, useAsync } from '../../components/AsyncSection'
import { Button, Card, StatusBadge } from '../../ui/kit'
import { refusalAware } from './orgShared'
import RolesDialog from './RolesDialog'

export default function RolesCard({ orgId }: { orgId: string }) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const roles = useAsync((signal) => refusalAware(listRoles(orgId, signal), t), [orgId])
  const requests = useAsync((signal) => refusalAware(listPermissionRequests(orgId, signal), t), [orgId])
  const pending = requests.state.s === 'ready' ? requests.state.d.filter((r) => r.status === 'pending').length : 0

  return (
    <Card title={t('rbac.titulo')} eyebrow={t('rbac.eyebrow')} as="section">
      <AsyncSection state={roles.state} onRetry={roles.reload}>
        {(r) => (
          <div className="org-card-pad" style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
            <p className="dx-muted" style={{ margin: 0 }}>
              {t('rbac.resumo', { papeis: r.roles.length, capacidades: r.catalog.length })}
            </p>
            {pending > 0 && <StatusBadge tone="warning">{t('rbac.pedidosPendentes', { count: pending })}</StatusBadge>}
            <div>
              <Button variant="secondary" icon="lock" onClick={() => setOpen(true)}>
                {t('rbac.gerir')}
              </Button>
            </div>
          </div>
        )}
      </AsyncSection>
      {open && (
        <RolesDialog
          orgId={orgId}
          onClose={() => {
            setOpen(false)
            roles.reload()
            requests.reload()
          }}
        />
      )}
    </Card>
  )
}
