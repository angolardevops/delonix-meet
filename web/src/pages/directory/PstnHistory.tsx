/**
 * Histórico PSTN: as chamadas telefónicas que entraram pelo dial-in da
 * organização (GET /api/orgs/{org}/voice/call-records, só admins). É o registo real
 * da central — número de quem ligou, número marcado e duração.
 *
 * Chamadas efectuadas não aparecem porque não existem: não há saída PSTN
 * (`voice/README.md`: «Sem outbound»), e as chamadas entre contactos só se
 * guardam quando ficam perdidas.
 */
import { useTranslation } from 'react-i18next'
import { listVoiceCdr } from '../../api'
import { AsyncSection, useAsync } from '../../components/AsyncSection'
import { Icon } from '../../ui/icons'
import { formatAgo, refusalAware, useLocaleTag } from '../admin/orgShared'
import { fmtDuration } from '../admin/VoiceCard'

export default function PstnHistory({ orgId }: { orgId: string }) {
  const { t } = useTranslation()
  const locale = useLocaleTag()
  const cdr = useAsync((signal) => refusalAware(listVoiceCdr(orgId, signal), t), [orgId])
  return (
    <>
      <div className="org-dir__sub">
        <span className="dx-eyebrow">{t('consola.contactos.telefoneTitulo')}</span>
      </div>
      <AsyncSection state={cdr.state} onRetry={cdr.reload}>
        {(rows) =>
          rows.length === 0 ? (
            <p className="org-dir__empty dx-muted">{t('consola.contactos.semTelefone')}</p>
          ) : (
            <ul className="org-rows" data-testid="pstn-history">
              {rows.map((c) => (
                <li key={c.id} className="org-row">
                  <div className="org-row__main org-row__main--static">
                    <span className="org-av org-av--group" aria-hidden="true">
                      <Icon name="phone" />
                    </span>
                    <span className="org-row__text">
                      <strong className="dx-num">{c.caller_number || t('consola.voz.anonimo')}</strong>
                      <span className="org-row__meta dx-num">
                        {[
                          t('consola.contactos.recebidaEm', { numero: c.did_e164 }),
                          formatAgo(c.started_at, locale),
                          fmtDuration(c.duration_secs),
                        ]
                          .filter(Boolean)
                          .join(' · ')}
                      </span>
                    </span>
                  </div>
                </li>
              ))}
            </ul>
          )
        }
      </AsyncSection>
    </>
  )
}
