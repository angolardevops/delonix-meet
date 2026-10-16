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
 * O tom de cada estado. O `failed` é `danger` — o tom que faltava ao badge e
 * que entrou com isto: antes ficava em `warning` (âmbar, que quer dizer
 * «atenção») porque a alternativa era o `record`, que já é o ponto vermelho da
 * gravação.
 */
const TOM: Record<MailStatus, BadgeTone> = {
  sent: 'success',
  failed: 'danger',
  pending: 'neutral',
  sending: 'warning',
}

export default function MailOutboxCard({ orgId }: { orgId: string }) {
  const { t } = useTranslation()
  const locale = useLocaleTag()
  const [estado, setEstado] = useState<MailStatus | ''>('')
  const [proposito, setProposito] = useState('')
  // As páginas JÁ VISTAS, acumuladas. A API é por cursor (não há «página 3»),
  // por isso o padrão honesto é «mostrar mais» e não uma barra de números: só
  // se sabe que há mais quando vem um `next_page_token`.
  const [maisPaginas, setMaisPaginas] = useState<MailMessage[]>([])
  const [cursorSeguinte, setCursorSeguinte] = useState<string | null>(null)
  const [aCarregarMais, setACarregarMais] = useState(false)
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

  // Mudar de filtro recomeça: as páginas acumuladas eram do filtro anterior.
  function filtrar(f: () => void) {
    setMaisPaginas([])
    setCursorSeguinte(null)
    f()
  }

  async function verMais(token: string) {
    setACarregarMais(true)
    try {
      const p = await listOrgMailMessages(orgId, {
        ...(estado ? { status: estado } : {}),
        ...(proposito ? { purpose: proposito } : {}),
        page_size: 25,
        page_token: token,
      })
      setMaisPaginas((x) => [...x, ...p.items])
      setCursorSeguinte(p.next_page_token)
    } catch {
      // Sem alarme: a lista que já está continua a servir, e o botão volta.
      setCursorSeguinte(token)
    } finally {
      setACarregarMais(false)
    }
  }

  return (
    <Card title={t('org.caixaSaida.titulo')} eyebrow={t('org.caixaSaida.eyebrow')} as="section">
      <div className="org-card-pad" style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
        <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
          <Select
            value={estado}
            onChange={(e) => filtrar(() => setEstado(e.target.value as MailStatus | ''))}
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
            onChange={(e) => filtrar(() => setProposito(e.target.value))}
            aria-label={t('org.caixaSaida.filtrarProposito')}
          >
            <option value="">{t('org.caixaSaida.todosPropositos')}</option>
            {PROPOSITOS.map((p) => (
              <option key={p} value={p}>
                {t(`org.caixaSaida.proposito.${p}`)}
              </option>
            ))}
          </Select>
          <Button size="sm" variant="ghost" icon="refresh" onClick={() => filtrar(lista.reload)}>
            {t('org.caixaSaida.actualizar')}
          </Button>
        </div>

        <AsyncSection state={lista.state} onRetry={lista.reload}>
          {(pagina) => {
            const items = [...pagina.items, ...maisPaginas]
            // O cursor é o da ÚLTIMA página pedida: na primeira vez vem da
            // resposta inicial, depois de cada «ver mais» vem da que chegou.
            const proximo = maisPaginas.length === 0 ? pagina.next_page_token : cursorSeguinte
            return items.length === 0 ? (
              <Empty icon="bell" title={t('org.caixaSaida.vazia')} />
            ) : (
              <>
                <ul className="org-simple" role="list">
                  {items.map((m: MailMessage) => (
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
                {proximo && (
                  <div>
                    <Button
                      size="sm"
                      variant="secondary"
                      busy={aCarregarMais}
                      onClick={() => void verMais(proximo)}
                    >
                      {t('org.caixaSaida.verMais')}
                    </Button>
                  </div>
                )}
              </>
            )
          }}
        </AsyncSection>
        <p className="dx-muted" style={{ margin: 0, fontSize: 11 }}>{t('org.caixaSaida.semCorpos')}</p>
      </div>
    </Card>
  )
}
