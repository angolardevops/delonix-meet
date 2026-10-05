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
 *
 * O «?» vive aqui pela mesma razão: a folha de atalhos (`AtalhosDialog`) tem
 * de abrir na consola, no Estúdio e dentro da reunião, e é aqui que se sabe
 * qual deles está à frente.
 */
import { createContext, ReactNode, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react'
import type { User } from '../api'
import { applyTheme, storedTheme } from '../theme'
import { combina, type EscopoDeAtalho } from '../ui/atalhos'
import { isPaletteShortcut } from '../ui/hotkeys'
import AtalhosDialog from './AtalhosDialog'
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
  /** Abre a folha de atalhos — o mesmo que carregar «?». */
  abrirAtalhos: () => void
  /** O Shell diz quem é admin e como abrir as definições; a sala não diz nada. */
  register: (extras: PaletteExtras | null) => void
}

const PaletteCtx = createContext<PaletteHostApi | null>(null)

export function usePaletteHost(): PaletteHostApi | null {
  return useContext(PaletteCtx)
}

/**
 * Os escopos que valem no ecrã à frente. O endereço é a única pista fiável:
 * a sala e o Estúdio são rotas, não estado deste anfitrião.
 */
function escoposDoEcra(inRoom: boolean): EscopoDeAtalho[] {
  const hash = typeof location === 'undefined' ? '' : location.hash
  const escopos: EscopoDeAtalho[] = ['global']
  if (inRoom) escopos.push('sala')
  if (hash.startsWith('#/studio')) escopos.push('estudio', 'mesa')
  if (inRoom || /^#\/(whiteboards|diagram)/.test(hash)) escopos.push('quadro')
  return escopos
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
  const [atalhos, setAtalhos] = useState(false)
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
      if (combina('?', e)) {
        e.preventDefault()
        setAtalhos((v) => !v)
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
  const abrirAtalhos = useCallback(() => setAtalhos(true), [])
  const api = useMemo<PaletteHostApi>(
    () => ({ open, close, isOpen, abrirAtalhos, register: setExtras }),
    [open, close, isOpen, abrirAtalhos],
  )

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
      {atalhos && <AtalhosDialog escopos={escoposDoEcra(inRoom)} onClose={() => setAtalhos(false)} />}
      {isOpen && (
        <CommandPalette
          onClose={close}
          onNavigate={navigate}
          onOpenHash={openHash}
          onEnterRoom={(code) => (inRoom ? openHash(`/r/${code}`) : onEnterRoom(code))}
          onLogout={onLogout}
          onSettings={inRoom ? undefined : extras?.onSettings}
          onToggleTheme={extras?.onToggleTheme ?? toggleThemeFallback}
          onAtalhos={() => {
            close()
            setAtalhos(true)
          }}
          user={user}
          isAdmin={extras?.isAdmin ?? false}
          inRoom={inRoom}
        />
      )}
    </PaletteCtx.Provider>
  )
}
