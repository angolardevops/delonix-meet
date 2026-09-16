import { useCallback, useEffect, useRef, useState } from 'react'
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
  /** Quem abriu o quadro para todos (`wb-open { by }`). */
  const [openedBy, setOpenedBy] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)
  /** Quantos traços havia no último «guardar» — fechar só volta a guardar se mudou. */
  const [savedCount, setSavedCount] = useState(0)
  /** Caneta detectada neste dispositivo, e se ela reporta pressão (para o cabeçalho). */
  const [pen, setPen] = useState({ on: false, pressao: false })
  /** A vista regista aqui como tirar o PNG (o canvas é dela). */
  const snapshotRef = useRef<(() => string | null) | null>(null)
  const registerSnapshot = useCallback((fn: (() => string | null) | null) => {
    snapshotRef.current = fn
  }, [])

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
      signal.on('wb-close', () => {
        setOpen(false)
        setOpenedBy(null)
      }),
      signal.on('wb-open', (m) => {
        setOpen(true)
        setOpenedBy(m.by)
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
    if (!next) {
      signal.send({ type: 'wb-close' })
      setOpenedBy(null)
    }
  }

  function close() {
    setOpen(false)
    setOpenedBy(null)
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

  async function save(pngBase64?: string) {
    const png = pngBase64 ?? snapshotRef.current?.() ?? null
    if (!png) return
    setSaving(true)
    try {
      await saveWhiteboard(t('room.quadro.nomeNaBiblioteca', { code }), code, png)
      setSavedCount(strokes.length)
      setStatus(t('room.estado.quadroGuardado'))
    } catch {
      setStatus(t('room.estado.quadroNaoGuardado'))
    } finally {
      setSaving(false)
    }
  }

  return { open, strokes, unsaved: strokes.length > 0 && strokes.length !== savedCount, openedBy, saving, pen, setPen, registerSnapshot, toggle, close, addStroke, clear, save }
}

export type WhiteboardState = ReturnType<typeof useWhiteboard>
