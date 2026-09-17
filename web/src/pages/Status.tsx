/**
 * Estado do serviço — público, sem sessão. Lê GET /api/status
 * (`server/src/main.rs`): API viva, base de dados a responder, tempo em
 * actividade e versão. Actualiza sozinho a cada 15 s.
 *
 * Só há três componentes porque o servidor só mede três. Uma lista maior seria
 * inventada.
 */
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { serverStatus } from '../api'
import { useAsync } from '../components/AsyncSection'
import { BadgeTone, Button, Card, Skeleton, StatusBadge } from '../ui/kit'
import { EstadoServico, partesUptime, Saude, saudeGlobal } from './auth/logica'
import Moldura from './publico/Moldura'

const INTERVALO_S = 15

interface Leitura {
  info: EstadoServico
  em: Date
}

async function lerEstado(signal: AbortSignal): Promise<Leitura> {
  return { info: await serverStatus(signal), em: new Date() }
}

const TOM: Record<Saude, BadgeTone> = { operacional: 'success', degradado: 'warning', indisponivel: 'record' }

export default function Status() {
  const { t, i18n } = useTranslation()
  const { state, reload } = useAsync(lerEstado, [])
  const [ultima, setUltima] = useState<Leitura | null>(null)

  useEffect(() => {
    const id = setInterval(reload, INTERVALO_S * 1000)
    return () => clearInterval(id)
  }, [reload])

  useEffect(() => {
    if (state.s === 'ready') setUltima(state.d)
  }, [state])

  const aCarregar = state.s === 'loading'
  const emFalha = state.s === 'error'
  // Em falha, o estado é o do pedido que falhou — não o da última leitura boa.
  const info = emFalha ? null : (ultima?.info ?? null)
  const saude = saudeGlobal(info)

  const up = (v: boolean | undefined) =>
    v === undefined ? (
      <StatusBadge tone="neutral">{t('publico.estado.desconhecido')}</StatusBadge>
    ) : v ? (
      <StatusBadge tone="success">{t('publico.estado.ok')}</StatusBadge>
    ) : (
      <StatusBadge tone="record">{t('publico.estado.emBaixo')}</StatusBadge>
    )

  function uptime(s: number) {
    const p = partesUptime(s)
    if (p.d > 0) return t('publico.estado.uptimeDias', p)
    if (p.h > 0) return t('publico.estado.uptimeHoras', p)
    return t('publico.estado.uptimeMin', p)
  }

  return (
    <Moldura pagina="status" estreita>
      <header className="pub-cabecalho">
        <h1>{t('publico.estado.titulo')}</h1>
        <p className="dx-muted">{t('publico.estado.actualiza', { s: INTERVALO_S })}</p>
      </header>

      <section className={`pub-saude pub-saude--${aCarregar && !ultima ? 'aVerificar' : saude}`} aria-live="polite">
        {aCarregar && !ultima ? (
          <StatusBadge tone="neutral">{t('publico.estado.aVerificar')}</StatusBadge>
        ) : (
          <StatusBadge tone={TOM[saude]}>{t(`publico.estado.${saude}`)}</StatusBadge>
        )}
        {emFalha && <p className="dx-muted">{t('publico.estado.semResposta')}</p>}
        <Button size="sm" variant="secondary" icon="refresh" onClick={reload} busy={aCarregar}>
          {t('publico.estado.actualizar')}
        </Button>
      </section>

      <Card title={t('publico.estado.componentes')} className="pub-cartao">
        {aCarregar && !ultima ? (
          <div className="pub-esqueleto">
            <Skeleton h={16} />
            <Skeleton h={16} />
            <Skeleton h={16} />
          </div>
        ) : (
          <ul className="pub-componentes">
            <li>
              <span>{t('publico.estado.api')}</span>
              {up(emFalha ? false : info?.api)}
            </li>
            <li>
              <span>{t('publico.estado.bd')}</span>
              {up(emFalha ? undefined : info?.db)}
            </li>
            <li>
              <span>{t('publico.estado.web')}</span>
              {up(true)}
            </li>
          </ul>
        )}
      </Card>

      {ultima && (
        <Card title={t('publico.estado.detalhes')} className="pub-cartao">
          <dl className="dx-kv">
            <dt>{t('publico.estado.uptime')}</dt>
            <dd className="dx-num">{uptime(ultima.info.uptime_secs)}</dd>
            <dt>{t('publico.estado.versao')}</dt>
            <dd className="dx-num">{ultima.info.version}</dd>
            <dt>{t('publico.estado.verificado')}</dt>
            <dd className="dx-num">{ultima.em.toLocaleTimeString(i18n.language)}</dd>
          </dl>
        </Card>
      )}
    </Moldura>
  )
}
