/**
 * «Plano de marcação»: as regras pela ordem em que casam e os números de
 * emergência, com «Editar plano» (substitui o plano inteiro) e «Testar número»
 * (diz o que o plano faria; não liga a ninguém).
 */
import { ReactNode, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { DialPlan, Trunk } from '../../api'
import { Async, AsyncSection } from '../../components/AsyncSection'
import { Button, Card, StatusBadge, Tag } from '../../ui/kit'
import DialPlanDialog from './DialPlanDialog'
import { NA, useTelecomText } from './shared'
import TestNumberDialog from './TestNumberDialog'

export default function DialPlanCard({ orgId, state, reload, trunks }: { orgId: string; state: Async<DialPlan>; reload: () => void; trunks: Trunk[] }) {
  const { t } = useTranslation()
  const { label } = useTelecomText()
  const [open, setOpen] = useState<'edit' | 'test' | null>(null)
  const names = new Map(trunks.map((k) => [k.id, k.name]))
  /** Sem operadora diz-se que não há; uma que não está na lista mostra o identificador. */
  const trunkName = (id: string | null | undefined): ReactNode =>
    id ? names.get(id) ?? <span className="dx-num dx-muted">{id.slice(0, 8)}</span> : NA

  return (
    <Card title={t('telecom.plano.titulo')} eyebrow={t('telecom.plano.eyebrow')} flush className="tel-card">
      <AsyncSection state={state} onRetry={reload}>
        {(plan) => (
          <>
            <div className="tel-actions tel-actions--bar">
              <Button size="sm" variant="secondary" icon="edit" onClick={() => setOpen('edit')}>
                {t('telecom.plano.editar')}
              </Button>
              <Button size="sm" variant="secondary" icon="phone" onClick={() => setOpen('test')}>
                {t('telecom.plano.testar')}
              </Button>
            </div>
            {plan.rules.length === 0 ? (
              <p className="dx-muted tel-note">{t('telecom.plano.vazio')}</p>
            ) : (
              <div className="dx-table-wrap">
                <table className="dx-table tel-table" data-testid="tel-dialplan">
                  <caption className="dx-sr-only">{t('telecom.plano.titulo')}</caption>
                  <thead>
                    <tr>
                      <th scope="col">{t('telecom.plano.colN')}</th>
                      <th scope="col">{t('telecom.plano.colPadrao')}</th>
                      <th scope="col">{t('telecom.plano.colDestino')}</th>
                      <th scope="col">{t('telecom.plano.colOperadora')}</th>
                      <th scope="col">{t('telecom.plano.colReserva')}</th>
                      <th scope="col">{t('telecom.plano.colGrava')}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {plan.rules.map((r, i) => (
                      <tr key={`${i}-${r.pattern}`}>
                        <td className="dx-num dx-muted">{i + 1}</td>
                        <th scope="row" className="dx-num tel-table__pattern">
                          {r.pattern}
                        </th>
                        <td>
                          <span className="tel-stack">
                            <span>{r.description || label('accao', r.action)}</span>
                            {r.description && <span className="dx-muted tel-small">{label('accao', r.action)}</span>}
                            {r.emergency && (
                              <span className="tel-badges">
                                <StatusBadge tone="warning" icon="alert">
                                  {t('telecom.plano.emergencia')}
                                </StatusBadge>
                              </span>
                            )}
                          </span>
                        </td>
                        <td>{trunkName(r.trunk_id)}</td>
                        <td>{trunkName(r.fallback_trunk_id)}</td>
                        <td>{r.record ? t('ui.sim') : t('ui.nao')}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
            <div className="tel-emergency">
              <span className="dx-eyebrow">{t('telecom.plano.numerosEmergencia')}</span>
              {plan.emergency_numbers.length === 0 ? (
                <span className="dx-muted tel-small">{t('telecom.plano.semEmergencia')}</span>
              ) : (
                <ul className="tel-tags" data-testid="tel-emergency">
                  {plan.emergency_numbers.map((n) => (
                    <li key={n}>
                      <Tag>{n}</Tag>
                    </li>
                  ))}
                </ul>
              )}
              <span className="dx-muted tel-small">{t('telecom.plano.emergenciaNota')}</span>
            </div>
            {open === 'edit' && (
              <DialPlanDialog
                orgId={orgId}
                plan={plan}
                trunks={trunks}
                onClose={() => setOpen(null)}
                onSaved={() => {
                  setOpen(null)
                  reload()
                }}
              />
            )}
            {open === 'test' && <TestNumberDialog orgId={orgId} onClose={() => setOpen(null)} />}
          </>
        )}
      </AsyncSection>
    </Card>
  )
}
