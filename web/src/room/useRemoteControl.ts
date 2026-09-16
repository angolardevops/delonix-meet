import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { AGENTE_CONTROLO_REMOTO } from '../capabilities'
import type { RoomCore } from './useRoomCore'

/**
 * Controlo remoto do ecrã partilhado. Enquanto não houver agente nativo que
 * encaminhe input (`capabilities.ts`), a funcionalidade NÃO se anuncia: o
 * botão não aparece e um pedido que chegue é recusado antes de qualquer
 * diálogo de consentimento (R109).
 */
export function useRemoteControl(core: RoomCore) {
  const { t } = useTranslation()
  const { signal, setStatus } = core
  const [ctrlAsk, setCtrlAsk] = useState<{ from: string; username: string } | null>(null)

  useEffect(
    () =>
      signal.on('remote-control', (m) => {
        if (m.action === 'request') {
          // Sem agente, recusa-se JÁ. Abrir o diálogo pediria um consentimento
          // sem efeito — a pessoa dizia que sim e nada acontecia.
          if (!AGENTE_CONTROLO_REMOTO) {
            signal.send({ type: 'remote-control', to: m.from, action: 'deny', payload: null })
            return
          }
          const who = core.peersRef.current.find((p) => p.peerId === m.from)?.username ?? ''
          setCtrlAsk({ from: m.from, username: who })
        } else if (m.action === 'accept') {
          setStatus(t('room.estado.controloAceite'))
        } else if (m.action === 'deny') {
          setStatus(t('room.estado.controloRecusado'))
        }
      }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [signal],
  )

  /** `undefined` esconde o botão: um botão cujo único desfecho é recusa é ruído. */
  const requestControl =
    AGENTE_CONTROLO_REMOTO
      ? () => signal.send({ type: 'remote-control', to: core.presentation?.peerId ?? '', action: 'request', payload: null })
      : undefined

  function answer(accept: boolean) {
    if (!ctrlAsk) return
    signal.send({ type: 'remote-control', to: ctrlAsk.from, action: accept ? 'accept' : 'deny', payload: null })
    setCtrlAsk(null)
  }

  return { ctrlAsk, requestControl, answer }
}
