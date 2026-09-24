import { useEffect, useState } from 'react'
import type { LiveDestination } from '../signaling'
import type { RoomCore } from './useRoomCore'

export interface LiveState {
  on: boolean
  destinations: LiveDestination[]
  /** Epoch ms de quando a sala entrou no ar. */
  since: number | null
}

/** Destinos que estão MESMO no ar (o resto está a ligar, parado ou em erro). */
export function destinosNoAr(live: LiveState): LiveDestination[] {
  return live.destinations.filter((d) => d.state === 'live')
}

/**
 * Estado AO VIVO da SALA, anunciado pelo servidor a toda a gente (`live`). Antes
 * só quem emitia sabia que estava no ar — o badge vinha do browser dele.
 */
export function useLive(core: RoomCore): LiveState {
  const [live, setLive] = useState<LiveState>({ on: false, destinations: [], since: null })
  useEffect(
    () => core.signal.onB1('live', (m) => setLive({ on: m.on, destinations: m.destinations ?? [], since: m.since ?? null })),
    [core.signal],
  )
  return live
}
