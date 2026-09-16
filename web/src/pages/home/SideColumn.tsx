/**
 * Coluna direita da Início — só com dados que o servidor tem.
 *
 * O template mostra aqui «Canais de emissão», «A minha sala» (ligação pessoal
 * e dial-in) e um armazenamento com quota total.
 *  - Canais de emissão: os destinos vão por sessão e não se guardam.
 *  - A minha sala: não há sala pessoal (cada reunião tem código aleatório).
 *    O dial-in PSTN tem plano de controlo (`voice.rs`: número + PIN por sala)
 *    mas falta a ponte FreeSWITCH↔SFU — um número aqui levava a pessoa a uma
 *    conferência só de voz, fora da reunião. Não se mostra.
 *  - Quota total: o servidor sabe quanto ocupam as gravações, não o tecto.
 * Ficam os quadros recentes, o volume real das gravações (admins) e o destino
 * do armazenamento da plataforma (só para quem o servidor deixa ler).
 */
import { useTranslation } from 'react-i18next'
import { ApiError, getPlatformStorage, listWhiteboards, orgStats } from '../../api'
import { AsyncSection, useAsync } from '../../components/AsyncSection'
import { useShell } from '../../components/shellContext'
import { Icon } from '../../ui/icons'
import { Card } from '../../ui/kit'
import { fmtBytes, localeOf } from '../calendar/dates'

function RecentBoards() {
  const { t, i18n } = useTranslation()
  const locale = localeOf(i18n.language)
  const { state, reload } = useAsync(async (signal) => (await listWhiteboards(signal)).slice(0, 3), [])
  // Sem quadros, o cartão não ocupa a coluna (como na Início antiga).
  if (state.s === 'ready' && state.d.length === 0) return null
  return (
    <Card
      title={t('home.quadros.titulo')}
      actions={
        <a className="home-link" href="#/whiteboards">
          {t('home.quadros.verTodos')}
        </a>
      }
    >
      <AsyncSection state={state} onRetry={reload}>
        {(ws) => (
          <ul className="home-list" role="list">
            {ws.map((w) => (
              <li key={w.id}>
                <a className="home-list__row" href="#/whiteboards">
                  <Icon name="board" size={16} />
                  <span className="home-list__text">
                    <strong>{w.title}</strong>
                    <small>
                      {new Date(w.created_at).toLocaleDateString(locale, { day: 'numeric', month: 'short' })}
                      {w.room_code && (
                        <>
                          {' · '}
                          <span className="dx-num">{w.room_code}</span>
                        </>
                      )}
                    </small>
                  </span>
                </a>
              </li>
            ))}
          </ul>
        )}
      </AsyncSection>
    </Card>
  )
}

function Storage() {
  const { t, i18n } = useTranslation()
  const { org } = useShell()
  const locale = localeOf(i18n.language)
  const orgId = org?.id
  const { state, reload } = useAsync(
    async (signal) => {
      if (!orgId) return null
      try {
        // `orgStats` não aceita sinal; o `useAsync` ignora a resposta se já saímos.
        void signal
        return await orgStats(orgId)
      } catch (e) {
        // Os números da organização são só para admins: o servidor decide, e
        // um 403 esconde o cartão em vez de o mostrar como avaria.
        if (e instanceof ApiError && e.status === 403) return null
        throw e
      }
    },
    [orgId],
  )
  // Destino do armazenamento da PLATAFORMA (local/NFS/WebDAV): só quem o
  // servidor declara administrador da plataforma o lê; os outros não vêem rótulo.
  const backend = useAsync(async () => {
    try {
      return (await getPlatformStorage()).storage_type
    } catch {
      return null
    }
  }, [])
  const backendLabel = backend.state.s === 'ready' && backend.state.d ? t(`integrations.storage.tipo.${backend.state.d}`) : null
  if (!orgId || (state.s === 'ready' && state.d === null)) return null
  return (
    <Card title={t('home.armazenamento.titulo')} eyebrow={org?.name}>
      <AsyncSection state={state} onRetry={reload}>
        {(s) => {
          if (!s) return null
          const b = fmtBytes(s.recordings_bytes, locale)
          return (
            <div className="home-storage">
              <div className="home-storage__big">
                <span className="dx-num">{b.value}</span>
                <small className="dx-num">{b.unit}</small>
              </div>
              <div className="dx-muted">{t('home.armazenamento.gravacoes', { count: s.recordings_total })}</div>
              {backendLabel && (
                <div className="dx-muted" data-testid="home-storage-backend">
                  {t('consola.inicio.destino', { destino: backendLabel })}
                </div>
              )}
              {org?.retention_days ? (
                <div className="dx-muted">{t('home.armazenamento.retencao', { count: org.retention_days })}</div>
              ) : null}
            </div>
          )
        }}
      </AsyncSection>
    </Card>
  )
}

export default function SideColumn() {
  return (
    <aside className="home-side">
      <RecentBoards />
      <Storage />
    </aside>
  )
}
