/**
 * Coluna direita da Início (336 px, como no DelonixHome) — só com dados que o
 * servidor tem.
 *
 * O template mostra aqui «Canais de emissão», «A minha sala» (ligação pessoal
 * e dial-in) e «Armazenamento» com quota total.
 *  - Canais de emissão: os destinos vão por sessão e não se guardam.
 *  - A minha sala: não há sala pessoal (cada reunião tem código aleatório).
 *    O dial-in PSTN tem plano de controlo (voice.rs: número + PIN por sala)
 *    mas falta a ponte FreeSWITCH↔SFU — um número aqui levava a pessoa a uma
 *    conferência só de voz, fora da reunião. Não se mostra.
 *  - Armazenamento: o cartão fica com a forma do template (título, linha
 *    mono com o volume e o destino, linha da retenção). A barra não se
 *    desenha: o servidor sabe quanto ocupam as gravações, não o tecto — e
 *    uma barra sem tecto é um número inventado.
 * Os «Quadros recentes» da Início antiga saíram: o template não os tem e os
 * quadros estão a um clique no rail.
 */
import { useTranslation } from 'react-i18next'
import { ApiError, getPlatformStorage, orgStats } from '../../api'
import { AsyncSection, useAsync } from '../../components/AsyncSection'
import { useShell } from '../../components/shellContext'
import { fmtBytes, localeOf } from '../calendar/dates'

function Storage() {
  const { t, i18n } = useTranslation()
  const { org } = useShell()
  const locale = localeOf(i18n.language)
  const orgId = org?.id
  const { state, reload } = useAsync(
    async (signal) => {
      if (!orgId) return null
      try {
        void signal
        return await orgStats(orgId)
      } catch (e) {
        // Os números da organização são só para admins: o servidor decide, e
        // um 403 esconde o cartão em vez de o mostrar como avaria.
        if (e instanceof ApiError && (e.status === 401 || e.status === 403)) return null
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
    <section className="home-card" aria-labelledby="home-armazenamento">
      <h2 id="home-armazenamento" className="home-card__title">
        {t('home.armazenamento.titulo')}
      </h2>
      <AsyncSection state={state} onRetry={reload}>
        {(s) => {
          if (!s) return null
          const b = fmtBytes(s.recordings_bytes, locale)
          return (
            <div className="home-storage">
              <div className="home-storage__line dx-num">
                <span>
                  {b.value} {b.unit} · {t('home.armazenamento.gravacoes', { count: s.recordings_total })}
                </span>
                {backendLabel && <span data-testid="home-storage-backend">{backendLabel}</span>}
              </div>
              <div className="dx-muted home-storage__note">
                {org?.retention_days
                  ? t('home.armazenamento.retencao', { count: org.retention_days })
                  : t('consola.inicio.semRetencao')}
              </div>
            </div>
          )
        }}
      </AsyncSection>
    </section>
  )
}

export default function SideColumn() {
  return (
    <aside className="home-side">
      <Storage />
    </aside>
  )
}
