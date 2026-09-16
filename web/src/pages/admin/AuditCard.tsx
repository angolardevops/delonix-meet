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
import { Icon } from '../../ui/icons'
import { Card, IconButton, Select, StatusBadge } from '../../ui/kit'
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
          <Icon name="shieldCheck" size={13} />
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
        <button type="button" className="org-linkbtn" disabled={chain.state.s === 'loading'} onClick={chain.reload}>
          {t('consola.auditoria.verificar')}
        </button>
      </div>
      <AsyncSection state={audit.state} onRetry={audit.reload}>
        {(rows) =>
          rows.length === 0 ? (
            <p className="dx-muted org-card-note">{t('org.auditoria.vazio')}</p>
          ) : (
            <ul className="org-audit__list org-audit__scroll" role="list" data-testid="audit-rows">
              {rows.map((a) => (
                <li key={a.id} className="org-audit__row">
                  <span className="dx-num dx-muted">{formatDateTime(a.created_at, locale)}</span>
                  <strong>{a.actor}</strong>
                  <span className="org-audit__what">
                    <span className="dx-num">{a.action}</span>
                    {a.target && <span className="dx-muted"> · {a.target}</span>}
                  </span>
                </li>
              ))}
            </ul>
          )
        }
      </AsyncSection>
    </Card>
  )
}
