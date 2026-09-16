import { useEffect, useState } from 'react'
import type { BreakoutRoom } from '../signaling'
import type { RoomCore } from './useRoomCore'

/**
 * Salas paralelas (só em formação, só anfitrião a criar). O movimento de cada
 * pessoa (`breakout-move`) é tratado pela sessão, que guarda o caminho de
 * volta; aqui fica a gestão dos grupos e o regresso.
 */
export function useBreakouts(core: RoomCore, onSwitch?: (code: string) => void) {
  const { signal, code } = core
  const [rooms, setRooms] = useState<BreakoutRoom[]>([])
  /** Fim em SEGUNDOS epoch (como o servidor o envia). */
  const [endsAt, setEndsAt] = useState<number | null>(() => {
    const v = sessionStorage.getItem(`dx_bo_ends_${code}`)
    return v ? Number(v) : null
  })
  const [minutes, setMinutes] = useState(0)
  /** Como se distribuem as pessoas ao criar: à vez (servidor) ou à mão (começam vazias). */
  const [assign, setAssign] = useState<'auto' | 'manual'>('auto')
  /** Última mensagem do anfitrião a todas as salas (`announcement`). */
  const [announcement, setAnnouncement] = useState<{ from: string; text: string; at: number } | null>(null)
  // Relido a cada render: a sessão escreve-o quando o servidor nos move.
  const returnTo = sessionStorage.getItem(`dx_return_${code}`)

  useEffect(() => {
    const offs = [
      signal.on('breakouts-created', (m) => {
        setRooms(m.rooms)
        setEndsAt(m.ends_at)
      }),
      signal.onB1('announcement', (m) => setAnnouncement({ from: m.from, text: m.text, at: m.at })),
    ]
    return () => offs.forEach((off) => off())
  }, [signal])

  return {
    rooms,
    endsAt,
    minutes,
    setMinutes,
    returnTo,
    assign,
    setAssign,
    announcement,
    dismissAnnouncement: () => setAnnouncement(null),
    create: (count: number) => signal.sendB1({ type: 'breakouts-create', count, minutes: minutes || null, assign }),
    /** Só anfitrião: vai para a sala principal e para todas as salas de grupo. */
    broadcast: (text: string) => signal.sendB1({ type: 'breakouts-broadcast', text }),
    rename: (roomCode: string, label: string) => signal.send({ type: 'breakout-rename', code: roomCode, label }),
    add: () => signal.send({ type: 'breakout-add' }),
    moveUser: (name: string, roomCode: string) => signal.send({ type: 'breakout-move-user', name, code: roomCode }),
    closeAll: () => signal.send({ type: 'breakouts-close' }),
    /** O anfitrião visita um grupo, guardando o caminho de volta. */
    visit: (roomCode: string) => {
      sessionStorage.setItem(`dx_return_${roomCode}`, code)
      onSwitch?.(roomCode)
    },
    returnToMain: () => {
      if (!returnTo) return
      sessionStorage.removeItem(`dx_return_${code}`)
      sessionStorage.removeItem(`dx_bo_ends_${code}`)
      onSwitch?.(returnTo)
    },
  }
}

export type Breakouts = ReturnType<typeof useBreakouts>
