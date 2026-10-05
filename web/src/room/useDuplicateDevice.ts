import { useEffect, useState } from 'react'
import type { RoomCore } from './useRoomCore'

export type EscolhaDispositivo = 'meet' | 'phone' | 'both'
export type ResultadoDispositivo = 'hung_up' | 'muted' | 'both'

/**
 * A mesma pessoa está na sala pelo browser e por um telefone (o Linphone dela):
 * o servidor avisa-a e ela escolhe onde continuar. «Só no telefone» é este
 * cliente a sair da sala; «só no Meet» e «nos dois» são pedidos ao servidor, que
 * só actua nas pernas desta pessoa.
 */
export function useDuplicateDevice(core: RoomCore, leave: () => void) {
  const { signal } = core
  const [aviso, setAviso] = useState<{ phoneId: string; canHangup: boolean } | null>(null)
  const [resultado, setResultado] = useState<ResultadoDispositivo | null>(null)

  useEffect(
    () =>
      signal.on('duplicate-device', (m) => {
        setResultado(null)
        setAviso({ phoneId: m.phone_id, canHangup: m.can_hangup })
      }),
    [signal],
  )
  useEffect(
    () =>
      signal.on('duplicate-resolved', (m) => {
        setAviso(null)
        setResultado(m.outcome)
      }),
    [signal],
  )

  function escolher(e: EscolhaDispositivo) {
    if (!aviso) return
    if (e === 'phone') {
      setAviso(null)
      leave()
      return
    }
    signal.send({ type: 'device-choice', phone_id: aviso.phoneId, keep: e })
  }

  return {
    aviso,
    resultado,
    escolher,
    /** Fecha sem escolher: os dois ficam como estão. */
    dispensar: () => setAviso(null),
    dispensarResultado: () => setResultado(null),
  }
}

export type DuplicateDevice = ReturnType<typeof useDuplicateDevice>
