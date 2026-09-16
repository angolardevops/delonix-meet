import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { currentUser } from '../api'
import { Cena, Fonte, RoomCompositor } from './compositor'
import { Destino, Directo, directoSuportado, EstadoDoDirecto } from '../studio/directo'
import type { RoomCore } from './useRoomCore'

export const MAX_MULTICAM_DESTINOS = 4

/**
 * Multicâmara (anfitrião): compõe os participantes num canvas e envia-o ao
 * directo para até quatro destinos RTMP. Os objectos imperativos vivem em refs:
 * pô-los em estado re-renderizava a sala a cada frame.
 */
export function useMulticam(core: RoomCore) {
  const { t } = useTranslation()
  const { code } = core
  const [open, setOpen] = useState(false)
  const [cena, setCena] = useState<Cena>('grelha')
  const [focoIds, setFocoIds] = useState<string[]>([])
  const [destinos, setDestinos] = useState<Destino[]>([{ url: '', chave: '', rotulo: '' }])
  const [estado, setEstado] = useState<EstadoDoDirecto>({ fase: 'parado' })
  const compositorRef = useRef<RoomCompositor | null>(null)
  const directoRef = useRef<Directo | null>(null)
  // O canvas do compositor não é do React — entra no DOM à mão sempre que o
  // painel (re)monta, e sai com ele.
  const previewRef = useCallback((node: HTMLDivElement | null) => {
    const c = compositorRef.current
    if (!node || !c) return
    c.canvas.className = 'rm-multicam__canvas'
    node.appendChild(c.canvas)
  }, [])

  // Segue quem entra e sai sem recriar o AudioContext (emudeceria o directo).
  useEffect(() => {
    if (!open || !compositorRef.current) return
    const fontes: Fonte[] = [
      { id: 'eu', nome: currentUser()?.username ?? '', stream: core.localStreamRef.current },
      ...core.peers.filter((p) => p.stream).map((p): Fonte => ({ id: p.peerId, nome: p.username, stream: p.stream })),
    ]
    compositorRef.current.definirParticipantes(fontes)
  }, [open, core.peers, core.localStreamRef])

  useEffect(() => {
    if (compositorRef.current) compositorRef.current.cena = cena
  }, [cena])
  useEffect(() => {
    if (compositorRef.current) compositorRef.current.focoIds = focoIds
  }, [focoIds])

  // Sair da sala a emitir pára o directo e liberta o compositor.
  useEffect(
    () => () => {
      void directoRef.current?.parar()
      directoRef.current = null
      compositorRef.current?.destruir()
      compositorRef.current = null
    },
    [],
  )

  function openPanel() {
    if (!compositorRef.current) {
      const c = new RoomCompositor()
      c.cena = cena
      c.focoIds = focoIds
      // A pré-visualização desenha JÁ: antes só arrancava com o directo, e o
      // painel mostrava um quadro vazio precisamente quando se escolhe a cena.
      c.iniciarPreVisualizacao()
      compositorRef.current = c
    }
    setOpen(true)
  }

  async function close() {
    const d = directoRef.current
    directoRef.current = null
    await d?.parar()
    compositorRef.current?.destruir()
    compositorRef.current = null
    setOpen(false)
    setEstado({ fase: 'parado' })
  }

  async function goLive() {
    const c = compositorRef.current
    if (!c) return
    try {
      const alvos = destinos.filter((d) => d.chave.trim())
      const token = core.roomTokenRef.current
      if (!token) throw new Error(t('room.multicam.semToken'))
      const fluxo = c.montarFluxo()
      const d = new Directo()
      d.aoMudar = setEstado
      directoRef.current = d
      await d.comecar(fluxo, code, token, alvos)
    } catch (e) {
      setEstado({ fase: 'erro', motivo: (e as Error).message || t('room.multicam.erroDirecto') })
      directoRef.current = null
    }
  }

  async function stopLive() {
    const d = directoRef.current
    directoRef.current = null
    await d?.parar()
    setEstado({ fase: 'parado' })
  }

  function toggleFoco(id: string) {
    setFocoIds((ids) => {
      const max = cena === 'solo' ? 1 : 2
      if (ids.includes(id)) return ids.filter((x) => x !== id)
      return [...ids, id].slice(-max)
    })
  }

  return {
    supported: directoSuportado(),
    open,
    openPanel,
    close,
    previewRef,
    cena,
    setCena,
    focoIds,
    toggleFoco,
    destinos,
    addDestino: () =>
      setDestinos((ds) => (ds.length >= MAX_MULTICAM_DESTINOS ? ds : [...ds, { url: '', chave: '', rotulo: '' }])),
    removeDestino: (i: number) => setDestinos((ds) => ds.filter((_, j) => j !== i)),
    updateDestino: (i: number, patch: Partial<Destino>) =>
      setDestinos((ds) => ds.map((d, j) => (j === i ? { ...d, ...patch } : d))),
    estado,
    goLive,
    stopLive,
  }
}

export type Multicam = ReturnType<typeof useMulticam>
