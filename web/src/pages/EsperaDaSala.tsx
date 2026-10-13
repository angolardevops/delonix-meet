/**
 * O ecrã de espera de uma sala agendada: «esta reunião começa às 15:00».
 *
 * Existe por um buraco reportado a 2026-10-13. Desde a migração 0114 a sala
 * de uma reunião nasce COM a reunião — é isso que dá um link para partilhar
 * logo ao agendar. Mas uma sala que existe é uma sala em que se entra, e sem
 * este portão quem abrisse o link de manhã caía dentro de uma sala vazia de
 * uma reunião da tarde, sem nada que lhe dissesse porquê.
 *
 * É um PORTÃO, não uma página: envolve o que já existe dos dois lados — a
 * sala, para quem tem sessão, e a `PortaDeConvidado`, para quem não tem — e
 * não duplica nenhuma das duas. Não toca em câmara nem microfone; a media
 * continua a nascer dentro da sala, depois do `Prejoin` (R2).
 *
 * Três coisas atravessam-no sempre, de propósito:
 * - uma REENTRADA recente (a sala guarda `dx_rejoin_<código>` quando a rede
 *   cai): recuperar de uma quebra não pode cair num ecrã de espera;
 * - uma FALHA a ler a porta: se o servidor não responde, entra-se à mesma —
 *   um portão avariado não pode ser o que impede uma reunião de acontecer;
 * - quem carrega em «entrar mesmo assim», que é quase sempre o anfitrião a
 *   preparar a sala antes da hora.
 */
import { ReactNode, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { roomEntrance, RoomEntrance } from '../api'
import { Icon } from '../ui/icons'
import { Button, Spinner } from '../ui/kit'
import { fmtDayMonth, fmtTime, localeOf, sameDay } from './calendar/dates'
import '../ui/espera.css'

/** Reentrada depois de uma quebra de rede: a mesma janela que a sala já usa. */
export function reentradaRecente(code: string, agora = Date.now()): boolean {
  try {
    return agora - Number(sessionStorage.getItem(`dx_rejoin_${code}`) || 0) < 60_000
  } catch {
    return false
  }
}

/**
 * A porta está aberta AGORA? O servidor já respondeu a isto quando leu a
 * reunião, mas o relógio anda: recalcular localmente é o que faz o ecrã abrir
 * sozinho à hora marcada, sem ninguém carregar em nada.
 */
export function portaAberta(e: RoomEntrance, agora = Date.now()): boolean {
  if (e.open) return true
  return e.starts_at ? agora >= new Date(e.starts_at).getTime() : true
}

/** O que falta, em partes inteiras — dias, horas, minutos. */
export function faltam(ms: number): { dias: number; horas: number; minutos: number } {
  const total = Math.max(0, Math.ceil(ms / 60_000))
  return { dias: Math.floor(total / 1440), horas: Math.floor((total % 1440) / 60), minutos: total % 60 }
}

type Estado = { s: 'a-ler' } | { s: 'porta'; d: RoomEntrance } | { s: 'passa' }

export default function EsperaDaSala({ code, children }: { code: string; children: ReactNode }) {
  const { t, i18n } = useTranslation()
  const locale = localeOf(i18n.language)
  const [estado, setEstado] = useState<Estado>(() => (reentradaRecente(code) ? { s: 'passa' } : { s: 'a-ler' }))
  const [agora, setAgora] = useState(() => Date.now())

  useEffect(() => {
    if (estado.s === 'passa') return
    const ctrl = new AbortController()
    roomEntrance(code, ctrl.signal)
      .then((d) => {
        if (!ctrl.signal.aborted) setEstado(portaAberta(d) ? { s: 'passa' } : { s: 'porta', d })
      })
      // Falha a ler a porta (rede, 404, 5xx): entra-se. O que decide mesmo
      // quem entra é o `join`, não este ecrã.
      .catch(() => {
        if (!ctrl.signal.aborted) setEstado({ s: 'passa' })
      })
    return () => ctrl.abort()
    // Só do código: o resto do estado é desta peça e remontá-la seria voltar
    // a bater no servidor por nada.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [code])

  // O relógio só anda enquanto houver contagem para mostrar.
  useEffect(() => {
    if (estado.s !== 'porta') return
    const id = setInterval(() => setAgora(Date.now()), 15_000)
    return () => clearInterval(id)
  }, [estado.s])

  // A hora chegou com o ecrã aberto: a porta abre sozinha.
  useEffect(() => {
    if (estado.s === 'porta' && portaAberta(estado.d, agora)) setEstado({ s: 'passa' })
  }, [estado, agora])

  if (estado.s === 'passa') return <>{children}</>

  if (estado.s === 'a-ler') {
    return (
      <div className="espera" role="status">
        <Spinner />
      </div>
    )
  }

  const inicio = estado.d.starts_at ? new Date(estado.d.starts_at) : null
  const f = inicio ? faltam(inicio.getTime() - agora) : null

  return (
    <div className="espera">
      <main className="espera__card" data-testid="espera-sala">
        <span className="espera__icone" aria-hidden="true">
          <Icon name="calendar" />
        </span>
        <h1 className="espera__titulo">{estado.d.name}</h1>
        {inicio && (
          <p className="espera__quando dx-num">
            {sameDay(inicio, new Date(agora)) ? t('room.agendada.hoje') : fmtDayMonth(inicio, locale)} · {fmtTime(inicio, locale)}
          </p>
        )}
        {f && (
          <p className="espera__contagem" role="status" data-testid="espera-contagem">
            {f.dias > 0
              ? t('room.agendada.faltamDias', { count: f.dias })
              : f.horas > 0
                ? t('room.agendada.faltamHoras', { horas: f.horas, minutos: f.minutos })
                : t('room.agendada.faltamMinutos', { count: f.minutos })}
          </p>
        )}
        <p className="dx-muted espera__nota">{t('room.agendada.abreSozinho')}</p>
        <Button variant="secondary" onClick={() => setEstado({ s: 'passa' })}>
          {t('room.agendada.entrarMesmoAssim')}
        </Button>
      </main>
    </div>
  )
}
