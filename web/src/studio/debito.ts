/**
 * Débito ENVIADO pelo directo, em kbps, a partir de duas leituras dos bytes
 * que o `Directo` conta. É o único débito que o Estúdio mostra: o servidor não
 * devolve débito por destino (ver o cabeçalho de `LivePanel.tsx`).
 */
import { useEffect, useRef, useState } from 'react'
import type { EstadoDoDirecto } from './directo'

export function useDebito(estado: EstadoDoDirecto): number {
  const ant = useRef<{ bytes: number; t: number } | null>(null)
  const [kbps, setKbps] = useState(0)
  const bytes = estado.fase === 'no-ar' ? estado.bytes : -1
  useEffect(() => {
    if (bytes < 0) {
      ant.current = null
      setKbps(0)
      return
    }
    const agora = performance.now()
    const a = ant.current
    if (a && agora - a.t >= 900) {
      setKbps(Math.round(((bytes - a.bytes) * 8) / 1000 / ((agora - a.t) / 1000)))
      ant.current = { bytes, t: agora }
    } else if (!a) {
      ant.current = { bytes, t: agora }
    }
  }, [bytes])
  return kbps
}
