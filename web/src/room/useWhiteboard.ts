import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { saveWhiteboard } from '../api'
import type { WbStroke } from '../signaling'
import type { RoomCore } from './useRoomCore'

/**
 * Cores dos traços. São DADOS da reunião (viajam no `wb-stroke` e têm de ser
 * iguais em todos os ecrãs), não tema — por isso são valores e não variáveis
 * CSS. Tirados da paleta do produto: texto, acento, âmbar, sucesso, azul.
 */
export const WB_COLORS = ['#0b0b0c', '#ad1017', '#a85b00', '#1e7a4a', '#3c5a7a']

export function useWhiteboard(core: RoomCore) {
  const { t } = useTranslation()
  const { signal, code, setStatus } = core
  const [open, setOpen] = useState(false)
  const [strokes, setStrokes] = useState<WbStroke[]>([])

  useEffect(() => {
    const offs = [
      // O snapshot ao entrar só carrega o conteúdo — NÃO abre o quadro.
      signal.on('wb-state', (m) => setStrokes(m.strokes)),
      // Alguém desenhou: o quadro aparece a todos.
      signal.on('wb-stroke', (m) => {
        setStrokes((st) => [...st, m.stroke])
        setOpen(true)
      }),
      signal.on('wb-clear', () => setStrokes([])),
      signal.on('wb-close', () => setOpen(false)),
      signal.on('wb-open', (m) => {
        setOpen(true)
        setStatus(t('room.estado.quadroAbertoPor', { nome: m.by }))
      }),
    ]
    return () => offs.forEach((off) => off())
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [signal])

  /** Abrir difunde a todos quando é quem apresenta (ou o anfitrião) a abrir. */
  function toggle() {
    const next = !open
    setOpen(next)
    if (next && (core.sharing || core.isHost)) signal.send({ type: 'wb-open' })
    if (!next) signal.send({ type: 'wb-close' })
  }

  function close() {
    setOpen(false)
    signal.send({ type: 'wb-close' })
  }

  function addStroke(stroke: WbStroke) {
    setStrokes((st) => [...st, stroke])
    signal.send({ type: 'wb-stroke', stroke })
  }

  function clear() {
    setStrokes([])
    signal.send({ type: 'wb-clear' })
  }

  async function save(pngBase64: string) {
    try {
      await saveWhiteboard(t('room.quadro.nomeNaBiblioteca', { code }), code, pngBase64)
      setStatus(t('room.estado.quadroGuardado'))
    } catch {
      setStatus(t('room.estado.quadroNaoGuardado'))
    }
  }

  return { open, strokes, toggle, close, addStroke, clear, save }
}

export type WhiteboardState = ReturnType<typeof useWhiteboard>
