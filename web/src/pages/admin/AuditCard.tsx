/**
 * Registo de auditoria da organização (`/audit`, só admin).
 *
 * O selo «imutável» não é um adjectivo: cada registo leva o hash do anterior
 * (migração 0037) e `/audit/verify` recalcula a cadeia inteira no servidor. O
 * selo diz o que essa verificação devolveu — intacta, ou em que registo partiu.
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { listAudit, verifyAudit } from '../../api'
import { AsyncSection, useAsync } from '../../components/AsyncSection'
import { Button, Card, IconButton, Select, StatusBadge } from '../../ui/kit'
import { formatDateTime, refusalAware, useLocaleTag } from './orgShared'

const LIMITS = [50, 100, 500] as const

export default function AuditCard({ orgId }: { orgId: string }) {
  const { t } = useTranslation()
  const locale = useLocaleTag()
  const [limit, setLimit] = useState<number>(100)
  const audit = useAsync(() => refusalAware(listAudit(orgId, limit), t), [orgId, limit])
  const chain = useAsync((signal) => refusalAware(verifyAudit(orgId, signal), t), [orgId])

  return (
    <Card
      className="org-audit"
      title={t('org.auditoria.titulo')}
      eyebrow={undefined}
      flush
      actions={
        <span className="org-row-actions">
          <span className="dx-num dx-muted org-meta">
            {[
              audit.state.s === 'ready' ? t('org.auditoria.eventos', { count: audit.state.d.length }) : null,
              chain.state.s === 'ready' && chain.state.d.intact ? t('consola.auditoria.imutavelMeta') : null,
            ]
              .filter(Boolean)
              .join(' · ')}
          </span>
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
      <div className="org-audit__chain" data-testid="audit-chain" aria-live="polite">
        {chain.state.s === 'loading' ? (
          <StatusBadge tone="neutral">{t('consola.auditoria.aVerificar')}</StatusBadge>
        ) : chain.state.s === 'error' ? (
          <StatusBadge tone="warning">{t('consola.auditoria.naoVerificada')}</StatusBadge>
        ) : chain.state.d.intact ? (
          <StatusBadge tone="success" icon="shieldCheck">
            {t('consola.auditoria.imutavel')}
          </StatusBadge>
        ) : (
          <StatusBadge tone="record" icon="alert">
            {t('consola.auditoria.partida')}
          </StatusBadge>
        )}
        <span className="dx-muted">
          {chain.state.s === 'ready'
            ? chain.state.d.intact
              ? t('consola.auditoria.intacta', { count: chain.state.d.entries })
              : t('consola.auditoria.partidaEm', { seq: chain.state.d.broken_at_seq ?? '?' })
            : chain.state.s === 'error'
              ? chain.state.msg
              : t('consola.auditoria.cadeiaExplica')}
        </span>
        <span className="dx-spacer" />
        <Button size="sm" variant="ghost" icon="shield" busy={chain.state.s === 'loading'} onClick={chain.reload}>
          {t('consola.auditoria.verificar')}
        </Button>
      </div>
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
