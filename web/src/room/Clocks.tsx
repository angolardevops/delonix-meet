import { useEffect, useState } from 'react'

/**
 * Relógios da sala, como FOLHAS (achado 2.1 do docs/ux-perf-review.md).
 *
 * PORQUÊ: `Room.tsx` tinha três `setInterval` de 1 Hz a chamar `setState` na
 * RAIZ de um componente de 4 254 linhas. Cada tique reconciliava a árvore
 * inteira — incluindo todos os `<RemoteTile>` e os seus elementos `<video>` —
 * para actualizar um contador de segundos num canto do ecrã.
 *
 * Aqui o tique fica dentro do componente que mostra o número. O resto da sala
 * não sabe que horas são, e não volta a renderizar por causa delas.
 */

/** mm:ss (ou h:mm:ss). Veio da sala com os relógios — lá já não é usado. */
function fmt(secs: number): string {
  const s = Math.max(0, Math.floor(secs))
  const h = Math.floor(s / 3600)
  const m = Math.floor((s % 3600) / 60)
  const r = s % 60
  const dois = (n: number) => String(n).padStart(2, '0')
  return h > 0 ? `${h}:${dois(m)}:${dois(r)}` : `${dois(m)}:${dois(r)}`
}

/** Força um render por segundo — NESTE nó e em mais nenhum. */
function useSegundo(): void {
  const [, setN] = useState(0)
  useEffect(() => {
    const t = setInterval(() => setN((n) => n + 1), 1000)
    return () => clearInterval(t)
  }, [])
}

/** Duração da reunião. Era `elapsed` na raiz — um `setState` por segundo. */
export function MeetingElapsed({ startedAt }: { startedAt: number }) {
  useSegundo()
  // Sem hora de início não se mostra número NENHUM. Sem esta guarda, um
  // `startedAt` a 0 dá a distância à época Unix — o contador mostrava
  // `496594:12:29` e lia-se como um relógio a funcionar. Um ecrã sem contador
  // é uma avaria visível; um contador com 56 anos passa despercebido.
  if (!startedAt) return null
  const secs = Math.floor((Date.now() - startedAt) / 1000)
  if (secs <= 0) return null
  return (
    <span className="meeting-elapsed" title="Duração da reunião">
      {fmt(secs)}
    </span>
  )
}

/**
 * Conta para trás até `endsAt` (segundos epoch). `render` decide o invólucro —
 * a mesma folha serve o chip do topo e o painel de grupos.
 */
export function Countdown({
  endsAt,
  render,
}: {
  endsAt: number
  render: (texto: string, restam: number) => React.ReactNode
}) {
  useSegundo()
  const restam = endsAt - Math.floor(Date.now() / 1000)
  return <>{render(fmt(restam), restam)}</>
}

/**
 * Relógio de parede do topo da sala. Era `setClock` na raiz de 30 em 30
 * segundos — 2 880 reconciliações da sala inteira por dia para mostrar as horas.
 */
export function WallClock({ locale = 'pt-PT' }: { locale?: string }) {
  const [agora, setAgora] = useState(() => new Date())
  useEffect(() => {
    const t = setInterval(() => setAgora(new Date()), 30_000)
    return () => clearInterval(t)
  }, [])
  return <span className="rt-clock">{agora.toLocaleTimeString(locale, { hour: '2-digit', minute: '2-digit' })}</span>
}
