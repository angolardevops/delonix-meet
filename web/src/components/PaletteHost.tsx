/**
 * Anfitrião da pesquisa global (Ctrl/Cmd+K) — vive ACIMA do Shell e da sala,
 * para o atalho valer em qualquer ecrã: consola, Estúdio e dentro de uma
 * reunião. Antes vivia no Shell e a sala não o tinha.
 *
 * - O atalho respeita quem escreve (`ui/hotkeys.ts`): num campo de texto,
 *   Ctrl+K fica para o campo.
 * - Esc fecha e o foco volta a quem o tinha (a paleta guarda-o ao abrir).
 * - Dentro de uma reunião, abrir um resultado ou um ecrã abre-o NUM SEPARADOR
 *   NOVO: navegar no mesmo separador desmontava a sala e cortava a chamada.
 *
 * O Shell regista aqui o que só ele sabe (papel de admin, definições, tema).
 */
import { createContext, ReactNode, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react'
import type { User } from '../api'
import { applyTheme, storedTheme } from '../theme'
import { isPaletteShortcut } from '../ui/hotkeys'
import CommandPalette from './CommandPalette'
import type { NavKey } from './shellContext'

export interface PaletteExtras {
  isAdmin: boolean
  onSettings?: () => void
  onToggleTheme?: () => void
}

interface PaletteHostApi {
  open: () => void
  close: () => void
  isOpen: boolean
  /** O Shell diz quem é admin e como abrir as definições; a sala não diz nada. */
  register: (extras: PaletteExtras | null) => void
}

const PaletteCtx = createContext<PaletteHostApi | null>(null)

export function usePaletteHost(): PaletteHostApi | null {
  return useContext(PaletteCtx)
}

export default function PaletteHost({
  user,
  inRoom,
  onLogout,
  onEnterRoom,
  children,
}: {
  user: User
  inRoom: boolean
  onLogout: () => void
  onEnterRoom: (code: string) => void
  children: ReactNode
}) {
  const [isOpen, setOpen] = useState(false)
  const [extras, setExtras] = useState<PaletteExtras | null>(null)
  const openRef = useRef(isOpen)
  openRef.current = isOpen

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // Com a paleta aberta o foco está no campo dela: Ctrl+K fecha (o campo
      // é da paleta, não de quem estava a escrever noutro sítio).
      if (openRef.current && (e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k' && !e.altKey && !e.shiftKey) {
        e.preventDefault()
        setOpen(false)
        return
      }
      if (!isPaletteShortcut(e)) return
      e.preventDefault()
      setOpen(true)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  const open = useCallback(() => setOpen(true), [])
  const close = useCallback(() => setOpen(false), [])
  const api = useMemo<PaletteHostApi>(() => ({ open, close, isOpen, register: setExtras }), [open, close, isOpen])

  const openHash = useCallback(
    (hash: string) => {
      const h = hash.startsWith('#') ? hash : `#${hash}`
      if (inRoom) window.open(`${location.pathname}${location.search}${h}`, '_blank', 'noopener')
      else location.hash = h.slice(1)
    },
    [inRoom],
  )
  const navigate = useCallback((k: NavKey) => openHash(k === 'home' ? '/' : `/${k}`), [openHash])

  function toggleThemeFallback() {
    applyTheme(storedTheme() === 'dark' ? 'light' : 'dark')
  }

  return (
    <PaletteCtx.Provider value={api}>
      {children}
      {isOpen && (
        <CommandPalette
          onClose={close}
          onNavigate={navigate}
          onOpenHash={openHash}
          onEnterRoom={(code) => (inRoom ? openHash(`/r/${code}`) : onEnterRoom(code))}
          onLogout={onLogout}
          onSettings={inRoom ? undefined : extras?.onSettings}
          onToggleTheme={extras?.onToggleTheme ?? toggleThemeFallback}
          user={user}
          isAdmin={extras?.isAdmin ?? false}
          inRoom={inRoom}
        />
      )}
    </PaletteCtx.Provider>
  )
}
