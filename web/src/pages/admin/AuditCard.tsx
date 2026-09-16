/** Registo de auditoria da organização (`/audit`, só admin). */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { listAudit } from '../../api'
import { AsyncSection, useAsync } from '../../components/AsyncSection'
import { Card, IconButton, Select } from '../../ui/kit'
import { formatDateTime, refusalAware, useLocaleTag } from './orgShared'

const LIMITS = [50, 100, 500] as const

export default function AuditCard({ orgId }: { orgId: string }) {
  const { t } = useTranslation()
  const locale = useLocaleTag()
  const [limit, setLimit] = useState<number>(100)
  const audit = useAsync(() => refusalAware(listAudit(orgId, limit), t), [orgId, limit])

  return (
    <Card
      className="org-audit"
      title={t('org.auditoria.titulo')}
      eyebrow={audit.state.s === 'ready' ? t('org.auditoria.eventos', { count: audit.state.d.length }) : undefined}
      flush
      actions={
        <span className="org-row-actions">
          <Select
            value={String(limit)}
            onChange={(e) => setLimit(Number(e.target.value))}
            aria-label={t('org.auditoria.quantos')}
            className="org-select-sm"
          >
            {LIMITS.map((n) => (
              <option key={n} value={n}>
                {t('org.auditoria.ultimos', { count: n })}
              </option>
            ))}
          </Select>
          <IconButton icon="refresh" bare label={t('org.auditoria.actualizar')} onClick={audit.reload} />
        </span>
      }
    >
      <AsyncSection state={audit.state} onRetry={audit.reload}>
        {(rows) =>
          rows.length === 0 ? (
            <p className="dx-muted org-card-note">{t('org.auditoria.vazio')}</p>
          ) : (
            <div className="dx-table-wrap org-table-wrap org-audit__scroll">
              <table className="dx-table org-table">
                <thead>
                  <tr>
                    <th scope="col">{t('org.coluna.quando')}</th>
                    <th scope="col">{t('org.coluna.quem')}</th>
                    <th scope="col">{t('org.coluna.accao')}</th>
                    <th scope="col">{t('org.coluna.alvo')}</th>
                  </tr>
                </thead>
                <tbody>
                  {rows.map((a) => (
                    <tr key={a.id}>
                      <td className="dx-num dx-muted org-nowrap">{formatDateTime(a.created_at, locale)}</td>
                      <td>
                        <strong>{a.actor}</strong>
                      </td>
                      <td className="dx-num">{a.action}</td>
                      <td className="dx-muted org-break">{a.target || '—'}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )
        }
      </AsyncSection>
    </Card>
  )
}
