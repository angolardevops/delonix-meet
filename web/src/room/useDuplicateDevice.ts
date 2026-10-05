import { useEffect, useRef, useState } from 'react'
import { AvisoDispositivo, decididas, enfileirar, lembrar, ResultadoDispositivo, resolver } from './dispositivos'
import type { RoomCore } from './useRoomCore'

export type EscolhaDispositivo = 'meet' | 'phone' | 'both'

/**
 * A mesma pessoa está na sala pelo browser e por um telefone (o Linphone dela):
 * o servidor avisa-a e ela escolhe onde continuar. «Só no telefone» é este
 * cliente a sair da sala; «só no Meet» e «nos dois» são pedidos ao servidor, que
 * só actua nas pernas desta pessoa. Uma pergunta de cada vez, por perna; o que
 * já se decidiu nesta sala não volta a perguntar (F5, reconexão).
 */
export function useDuplicateDevice(core: RoomCore, code: string, leave: () => void) {
  const { signal } = core
  const [fila, setFila] = useState<AvisoDispositivo[]>([])
  const [resultado, setResultado] = useState<ResultadoDispositivo | null>(null)
  const [aTratar, setATratar] = useState(false)
  const timeout = useRef<ReturnType<typeof setTimeout> | null>(null)

  const limparTimeout = () => {
    if (timeout.current) clearTimeout(timeout.current)
    timeout.current = null
  }

  useEffect(
    () =>
      signal.on('duplicate-device', (m) => {
        setFila((f) => enfileirar(f, { phoneId: m.phone_id, canHangup: m.can_hangup }, decididas(code)))
      }),
    [signal, code],
  )
  useEffect(
    () =>
      signal.on('duplicate-resolved', (m) => {
        limparTimeout()
        setATratar(false)
        lembrar(code, m.phone_id)
        // Só se fecha a pergunta DESSA perna; as outras continuam.
        setFila((f) => resolver(f, m.phone_id))
        setResultado(m.outcome)
      }),
    [signal, code],
  )
  useEffect(() => limparTimeout, [])

  const aviso = fila[0] ?? null

  function escolher(e: EscolhaDispositivo) {
    if (!aviso || aTratar) return
    if (e === 'phone') {
      lembrar(code, aviso.phoneId)
      setFila((f) => resolver(f, aviso.phoneId))
      leave()
      return
    }
    setATratar(true)
    signal.send({ type: 'device-choice', phone_id: aviso.phoneId, keep: e })
    // Sem resposta (socket fechado): o botão não fica morto para sempre.
    limparTimeout()
    timeout.current = setTimeout(() => setATratar(false), 8000)
  }

  return {
    aviso,
    pendentes: fila.length,
    aTratar,
    resultado,
    escolher,
    /** Fecha sem escolher: os dois ficam como estão, e a pessoa fica a saber do eco. */
    dispensar: () => {
      if (!aviso) return
      lembrar(code, aviso.phoneId)
      setFila((f) => resolver(f, aviso.phoneId))
      setResultado('both')
    },
    dispensarResultado: () => setResultado(null),
  }
}

export type DuplicateDevice = ReturnType<typeof useDuplicateDevice>
