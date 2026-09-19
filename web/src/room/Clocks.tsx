import { ReactNode, useEffect, useState } from 'react'

/**
 * Relógios da sala, como FOLHAS (achado 2.1). O tique fica dentro do nó que
 * mostra o número; o resto da sala não sabe que horas são e não volta a
 * renderizar por causa delas.
 */

/** mm:ss (ou h:mm:ss). */
export function fmtDuracao(secs: number): string {
  const s = Math.max(0, Math.floor(secs))
  const h = Math.floor(s / 3600)
  const m = Math.floor((s % 3600) / 60)
  const r = s % 60
  const dois = (n: number) => String(n).padStart(2, '0')
  return h > 0 ? `${h}:${dois(m)}:${dois(r)}` : `${dois(m)}:${dois(r)}`
}

/** hh:mm:ss sempre (o cronómetro da sessão no template: «00:24:18»). */
export function fmtRelogio(secs: number): string {
  const s = Math.max(0, Math.floor(secs))
  const dois = (n: number) => String(n).padStart(2, '0')
  return `${dois(Math.floor(s / 3600))}:${dois(Math.floor((s % 3600) / 60))}:${dois(s % 60)}`
}

/** Um render por segundo — NESTE nó e em mais nenhum. */
function useSegundo(): void {
  const [, setN] = useState(0)
  useEffect(() => {
    const id = setInterval(() => setN((n) => n + 1), 1000)
    return () => clearInterval(id)
  }, [])
}

/** Duração da reunião desde a entrada (`startedAt` em ms). */
export function MeetingElapsed({ startedAt, className }: { startedAt: number; className?: string }) {
  useSegundo()
  // Sem hora de início não se mostra número NENHUM: um `startedAt` a 0 dava a
  // distância à época Unix e lia-se como um relógio a funcionar.
  if (!startedAt) return null
  const secs = Math.floor((Date.now() - startedAt) / 1000)
  return <span className={className}>{fmtRelogio(Math.max(0, secs))}</span>
}

/**
 * Conta para trás até `endsAt` (SEGUNDOS epoch — temporizador e salas
 * paralelas). `render` decide o invólucro.
 */
export function Countdown({ endsAt, render }: { endsAt: number; render: (texto: string, restam: number) => ReactNode }) {
  useSegundo()
  const restam = endsAt - Math.floor(Date.now() / 1000)
  return <>{render(fmtDuracao(restam), restam)}</>
}

/** Segundos que faltam até `endsAtMs` (MILISSEGUNDOS — sondagens com prazo). */
export function SecondsLeft({ endsAtMs, render }: { endsAtMs: number; render: (restam: number) => ReactNode }) {
  useSegundo()
  return <>{render(Math.max(0, Math.ceil((endsAtMs - Date.now()) / 1000)))}</>
}

/** Tempo desde `desde` (ms) — duração de um directo. */
export function Since({ desde, render }: { desde: number; render: (texto: string) => ReactNode }) {
  useSegundo()
  return <>{render(fmtDuracao((Date.now() - desde) / 1000))}</>
}

/** Relógio de parede: 30 s chegam para mostrar as horas. */
export function WallClock({ locale, className }: { locale: string; className?: string }) {
  const [agora, setAgora] = useState(() => new Date())
  useEffect(() => {
    const id = setInterval(() => setAgora(new Date()), 30_000)
    return () => clearInterval(id)
  }, [])
  return <span className={className}>{agora.toLocaleTimeString(locale, { hour: '2-digit', minute: '2-digit' })}</span>
}
