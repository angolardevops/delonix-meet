/**
 * Relógio do Estúdio. Vive numa folha própria: é ele que tica uma vez por
 * segundo, não a página — que tem um canvas imperativo e vários painéis que
 * não precisam de voltar a desenhar-se por causa de um número.
 */
import { useEffect, useState } from 'react'
import { hhmmss } from './palco'

export function mmss(s: number): string {
  const total = Math.max(0, Math.floor(s))
  const m = Math.floor(total / 60)
  const r = total % 60
  return `${String(m).padStart(2, '0')}:${String(r).padStart(2, '0')}`
}

export default function Cronometro({
  activo,
  ler,
  className,
  label,
  ...rest
}: {
  activo: boolean
  /** Segundos decorridos, lidos da fonte da verdade (o compositor ou o directo). */
  ler: () => number
  className?: string
  label?: string
  'data-studio'?: string
}) {
  const [s, setS] = useState(() => ler())
  useEffect(() => {
    setS(ler())
    if (!activo) return
    const id = setInterval(() => setS(ler()), 1000)
    return () => clearInterval(id)
  }, [activo, ler])
  return (
    <span className={className} role="timer" aria-live="off" aria-label={label} {...rest}>
      {hhmmss(s)}
    </span>
  )
}
