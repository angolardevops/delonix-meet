import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { currentUser, saveWhiteboard } from '../api'
import type { WbStroke } from '../signaling'
import { comObjecto, comTexto, movido, semObjecto } from './wbState'
import type { RoomCore } from './useRoomCore'

/**
 * Cores dos traços. São DADOS da reunião (viajam no `wb-stroke` e têm de ser
 * iguais em todos os ecrãs), não tema — por isso são valores e não variáveis
 * CSS. Tirados da paleta do produto: texto, acento, âmbar, sucesso, azul.
 */
export const WB_COLORS = ['#0b0b0c', '#ad1017', '#a85b00', '#1e7a4a', '#3c5a7a']

/** Cursor de outra pessoa no quadro (efémero: some sem movimento). */
export interface WbCursor {
  x: number
  y: number
  laser: boolean
  input?: 'mouse' | 'pen' | 'touch'
  at: number
}

/** O cursor envia-se no máximo a ~15/s: o servidor corta acima de ~20/s por emissor. */
export const CURSOR_INTERVALO_MS = 66
/** Sem movimento durante isto, o cursor de outra pessoa desaparece. */
export const CURSOR_VIDA_MS = 4000

export function useWhiteboard(core: RoomCore) {
  const { t } = useTranslation()
  const { signal, code, setStatus } = core
  const [open, setOpen] = useState(false)
  const [strokes, setStrokes] = useState<WbStroke[]>([])
  /** Quem abriu o quadro para todos (`wb-open { by }`). */
  const [openedBy, setOpenedBy] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)
  /** Quantos objectos havia no último «guardar» — fechar só volta a guardar se mudou. */
  const [savedCount, setSavedCount] = useState(0)
  /** Caneta detectada neste dispositivo, e se ela reporta pressão (para o cabeçalho). */
  const [pen, setPen] = useState({ on: false, pressao: false })
  /** Páginas (`wb-pages`): quantas e qual está à vista de todos. */
  const [pages, setPages] = useState({ count: 1, current: 0 })
  /** Quem pode escrever (`wb-writers`). Sem restrição, toda a gente. */
  const [writers, setWriters] = useState<{ restricted: boolean; writers: string[] }>({ restricted: false, writers: [] })
  const [cursors, setCursors] = useState<Record<string, WbCursor>>({})
  /** Última actividade por NOME (autor de um objecto, cursor a mexer). */
  const [actividade, setActividade] = useState<Record<string, number>>({})
  /** Último dispositivo de entrada de cada pessoa (para «CANETA/RATO»). */
  const [entradas, setEntradas] = useState<Record<string, 'mouse' | 'pen' | 'touch'>>({})
  /** A vista regista aqui como tirar o PNG (o canvas é dela). */
  const snapshotRef = useRef<(() => string | null) | null>(null)
  const registerSnapshot = useCallback((fn: (() => string | null) | null) => {
    snapshotRef.current = fn
  }, [])

  const tocar = useCallback((nome: string | undefined) => {
    if (!nome) return
    setActividade((a) => ({ ...a, [nome]: Date.now() }))
  }, [])

  useEffect(() => {
    const offs = [
      // O estado ao entrar só carrega o conteúdo — NÃO abre o quadro. JUNTA-SE
      // ao que já houver: um traço ao vivo pode ter chegado antes do snapshot,
      // e substituir a lista apagava-o.
      signal.onB1('wb-state', (m) => setStrokes((st) => (m.strokes ?? []).reduce(comObjecto, st))),
      // Alguém desenhou: o quadro aparece a todos.
      signal.on('wb-stroke', (m) => {
        setStrokes((st) => comObjecto(st, m.stroke))
        tocar(m.stroke.by)
        setOpen(true)
      }),
      signal.on('wb-clear', () => setStrokes([])),
      signal.onB1('wb-erase', (m) => setStrokes((st) => semObjecto(st, m.id))),
      signal.onB1('wb-transform', (m) => setStrokes((st) => movido(st, m.id, m.dx, m.dy))),
      signal.onB1('wb-update', (m) => setStrokes((st) => comTexto(st, m.id, m.text))),
      signal.onB1('wb-pages', (m) => setPages({ count: Math.max(1, m.count), current: m.current })),
      signal.onB1('wb-writers', (m) => setWriters({ restricted: m.restricted, writers: m.writers })),
      signal.onB1('wb-cursor', (m) => {
        const agora = Date.now()
        setCursors((c) => ({ ...c, [m.from]: { x: m.x, y: m.y, laser: m.laser, input: m.input, at: agora } }))
        const nome = core.peersRef.current.find((p) => p.peerId === m.from)?.username
        if (nome) {
          tocar(nome)
          if (m.input) setEntradas((e) => (e[nome] === m.input ? e : { ...e, [nome]: m.input! }))
        }
      }),
      signal.on('peer-left', (m) =>
        setCursors((c) => {
          if (!(m.peer_id in c)) return c
          const n = { ...c }
          delete n[m.peer_id]
          return n
        }),
      ),
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

  // Cursores parados desaparecem. O tique só existe enquanto houver cursores.
  const temCursores = Object.keys(cursors).length > 0
  useEffect(() => {
    if (!temCursores) return
    const id = setInterval(() => {
      const agora = Date.now()
      setCursors((c) => {
        const vivos = Object.entries(c).filter(([, v]) => agora - v.at < CURSOR_VIDA_MS)
        return vivos.length === Object.keys(c).length ? c : Object.fromEntries(vivos)
      })
    }, 1000)
    return () => clearInterval(id)
  }, [temCursores])

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

  const meuNome = currentUser()?.username
  const canWrite = !writers.restricted || core.isHost || writers.writers.includes(core.meuPeerIdRef.current)

  /** Um objecto novo (traço, texto, nota, forma) na página à vista. O servidor não o devolve a quem enviou. */
  function addObject(o: WbStroke) {
    const obj: WbStroke = { ...o, id: o.id ?? crypto.randomUUID(), page: o.page ?? pages.current, by: meuNome }
    setStrokes((st) => comObjecto(st, obj))
    tocar(meuNome)
    signal.send({ type: 'wb-stroke', stroke: obj })
    return obj
  }

  function clear() {
    setStrokes([])
    signal.send({ type: 'wb-clear' })
  }

  // Apagar e editar texto são idempotentes: aplicam-se já. Mover NÃO — o
  // servidor devolve o `wb-transform` a toda a gente, incluindo a quem moveu,
  // e aplicá-lo duas vezes deslocava o dobro.
  function erase(id: string) {
    setStrokes((st) => semObjecto(st, id))
    signal.sendB1({ type: 'wb-erase', id })
  }
  function move(id: string, dx: number, dy: number) {
    if (Math.abs(dx) < 1e-4 && Math.abs(dy) < 1e-4) return
    signal.sendB1({ type: 'wb-transform', id, dx: Math.max(-1, Math.min(1, dx)), dy: Math.max(-1, Math.min(1, dy)) })
    tocar(meuNome)
  }
  function updateText(id: string, text: string) {
    if (!text.trim()) return
    setStrokes((st) => comTexto(st, id, text))
    signal.sendB1({ type: 'wb-update', id, text })
  }

  const ultimoCursor = useRef(0)
  function cursor(x: number, y: number, laser: boolean, input: 'mouse' | 'pen' | 'touch') {
    const agora = Date.now()
    if (agora - ultimoCursor.current < CURSOR_INTERVALO_MS) return
    ultimoCursor.current = agora
    signal.sendB1({ type: 'wb-cursor', x, y, laser, input })
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

  return {
    open,
    strokes,
    unsaved: strokes.length > 0 && strokes.length !== savedCount,
    openedBy,
    saving,
    pen,
    setPen,
    pages,
    writers,
    canWrite,
    cursors,
    actividade,
    entradas,
    registerSnapshot,
    toggle,
    close,
    addObject,
    clear,
    erase,
    move,
    updateText,
    cursor,
    addPage: () => signal.sendB1({ type: 'wb-add-page' }),
    /** Anfitrião ou quem apresenta; a página muda para toda a gente. */
    setPage: (page: number) => signal.sendB1({ type: 'wb-page', page }),
    setLocked: (on: boolean) => signal.sendB1({ type: 'wb-lock', on }),
    grant: (peerId: string, allowed: boolean) => signal.sendB1({ type: 'wb-grant', to: peerId, allowed }),
    save,
    /** Marca actividade minha (traço em curso) — para «A editar». */
    tocar,
  }
}

export type WhiteboardState = ReturnType<typeof useWhiteboard>
