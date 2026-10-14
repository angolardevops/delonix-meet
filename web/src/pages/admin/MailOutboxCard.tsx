/**
 * A caixa de saída da organização (`GET /api/orgs/{id}/mail-messages`).
 *
 * **Para que serve.** Uma mensagem que falha deixa a razão numa linha; até esta
 * rota existir, quem configurava um relay descobria o problema por um email que
 * nunca chegou. A verificação do relay (no cartão acima) diz se a LIGAÇÃO está
 * boa; isto diz o que aconteceu a CADA mensagem.
 *
 * **O que não aparece aqui, e é de propósito:**
 *
 * - **o corpo** — o de um convite leva o token do convite, e uma listagem não é
 *   sítio para credenciais; a API nem o devolve;
 * - **o correio pessoal** — a reposição de password e a prova de endereço
 *   enfileiram sem organização (são da pessoa, não da empresa), por isso nunca
 *   entram nesta lista.
 *
 * Os filtros têm catálogo FECHADO: um valor fora dele é um erro do servidor e
 * não uma página vazia — por isso são `Select` e não texto livre.
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { listOrgMailMessages, MailMessage, MailStatus } from '../../api'
import { AsyncSection, useAsync } from '../../components/AsyncSection'
import { BadgeTone, Button, Card, Empty, Select, StatusBadge } from '../../ui/kit'
import { formatDateTime, refusalAware, useLocaleTag } from './orgShared'

const ESTADOS: MailStatus[] = ['pending', 'sending', 'sent', 'failed']
const PROPOSITOS = ['invitation', 'notification', 'password_reset', 'email_verification']

/**
 * O tom de cada estado.
 *
 * **O `failed` fica em `warning` e não em vermelho porque o design system NÃO
 * TEM um tom `danger` para badge**: o `BadgeTone` é
 * `record | live | warning | success | neutral`, e o `record` é o ponto
 * vermelho da gravação — reutilizá-lo aqui era dar-lhe um segundo significado.
 * Acrescentar um tom é mexer no kit, e isso não entra de lado numa alteração
 * de funcionalidade. A razão da falha aparece por extenso na linha, que é o
 * que a pessoa precisa de ler.
 */
const TOM: Record<MailStatus, BadgeTone> = {
  sent: 'success',
  failed: 'warning',
  pending: 'neutral',
  sending: 'neutral',
}

export default function MailOutboxCard({ orgId }: { orgId: string }) {
  const { t } = useTranslation()
  const locale = useLocaleTag()
  const [estado, setEstado] = useState<MailStatus | ''>('')
  const [proposito, setProposito] = useState('')
  const lista = useAsync(
    (signal) =>
      refusalAware(
        listOrgMailMessages(
          orgId,
          {
            ...(estado ? { status: estado } : {}),
            ...(proposito ? { purpose: proposito } : {}),
            page_size: 25,
          },
          signal,
        ),
        t,
      ),
    [orgId, estado, proposito],
  )

  return (
    <Card title={t('org.caixaSaida.titulo')} eyebrow={t('org.caixaSaida.eyebrow')} as="section">
      <div className="org-card-pad" style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
        <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
          <Select
            value={estado}
            onChange={(e) => setEstado(e.target.value as MailStatus | '')}
            aria-label={t('org.caixaSaida.filtrarEstado')}
          >
            <option value="">{t('org.caixaSaida.todosEstados')}</option>
            {ESTADOS.map((s) => (
              <option key={s} value={s}>
                {t(`org.caixaSaida.estado.${s}`)}
              </option>
            ))}
          </Select>
          <Select
            value={proposito}
            onChange={(e) => setProposito(e.target.value)}
            aria-label={t('org.caixaSaida.filtrarProposito')}
          >
            <option value="">{t('org.caixaSaida.todosPropositos')}</option>
            {PROPOSITOS.map((p) => (
              <option key={p} value={p}>
                {t(`org.caixaSaida.proposito.${p}`)}
              </option>
            ))}
          </Select>
          <Button size="sm" variant="ghost" icon="refresh" onClick={lista.reload}>
            {t('org.caixaSaida.actualizar')}
          </Button>
        </div>

        <AsyncSection state={lista.state} onRetry={lista.reload}>
          {(pagina) =>
            pagina.items.length === 0 ? (
              <Empty icon="bell" title={t('org.caixaSaida.vazia')} />
            ) : (
              <ul className="org-simple" role="list">
                {pagina.items.map((m: MailMessage) => (
                  <li key={m.id}>
                    <span className="org-simple__main">
                      <strong>{m.subject}</strong>
                      <small className="dx-muted">
                        {m.to_address} · {t(`org.caixaSaida.proposito.${m.purpose}`, { defaultValue: m.purpose })}
                        {' · '}
                        {formatDateTime(m.created_at, locale)}
                      </small>
                      {/* A razão da falha é o motivo de esta lista existir: se
                          houver uma, mostra-se inteira e não truncada. */}
                      {m.error && <small className="dx-muted">{m.error}</small>}
                      {m.retry_at && (
                        <small className="dx-muted">
                          {t('org.caixaSaida.repeteEm', { quando: formatDateTime(m.retry_at, locale) })}
                        </small>
                      )}
                    </span>
                    <StatusBadge tone={TOM[m.status] ?? 'neutral'}>
                      {t(`org.caixaSaida.estado.${m.status}`, { defaultValue: m.status })}
                      {m.attempt > 1 ? ` · ${t('org.caixaSaida.tentativa', { n: m.attempt })}` : ''}
                    </StatusBadge>
                  </li>
                ))}
              </ul>
            )
          }
        </AsyncSection>
        <p className="dx-muted" style={{ margin: 0, fontSize: 11 }}>{t('org.caixaSaida.semCorpos')}</p>
      </div>
    </Card>
  )
}
