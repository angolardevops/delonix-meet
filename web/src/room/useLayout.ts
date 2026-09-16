import { RefObject, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { chooseLayers, type LocalConditions, type TileSignal } from '../layerPolicy'
import type { RoomCore } from './useRoomCore'

/**
 * Retratos por página. Acima disto o browser descodifica fluxos que ninguém
 * consegue ver; com a paginação o cliente só pede (`video-interest`) o vídeo da
 * página visível e o SFU deixa mesmo de o enviar.
 */
export const TILES_PER_PAGE = 24

export type ViewMode = 'grid' | 'stage'
export type PresLayout = 'bottom' | 'side'

/**
 * Grelha: para N retratos num contentor W×H, escolhe o número de colunas que
 * maximiza o tamanho de cada um (medido como 16:9) e depois ENCHE a área com
 * essas colunas e linhas — como no template (4×3 retratos que ocupam o palco
 * todo, não caixas 16:9 a flutuar). A largura da célula é também o que a
 * política de camada usa para pedir a qualidade certa.
 */
export function melhorGrelha(w: number, h: number, count: number, gap = 10): { cols: number; rows: number; w: number; h: number } {
  if (w <= 0 || h <= 0 || count <= 1) return { cols: 1, rows: 1, w: Math.floor(Math.max(0, w)), h: Math.floor(Math.max(0, h)) }
  const RATIO = 16 / 9
  let best = { cols: 1, rows: count, scale: -1 }
  for (let cols = 1; cols <= count; cols++) {
    const rows = Math.ceil(count / cols)
    const cellW = (w - gap * (cols - 1)) / cols
    const cellH = (h - gap * (rows - 1)) / rows
    const scale = Math.min(cellW / RATIO, cellH)
    if (scale > best.scale) best = { cols, rows, scale }
  }
  return {
    cols: best.cols,
    rows: best.rows,
    w: Math.floor((w - gap * (best.cols - 1)) / best.cols),
    h: Math.floor((h - gap * (best.rows - 1)) / best.rows),
  }
}

function useGridSize(areaRef: RefObject<HTMLDivElement | null>, count: number, active: boolean) {
  const [size, setSize] = useState({ w: 480, h: 270, cols: 1, rows: 1 })
  useEffect(() => {
    const el = areaRef.current
    if (!el || !active) return
    const compute = () => {
      if (el.clientWidth <= 0 || el.clientHeight <= 0 || count === 0) return
      const g = melhorGrelha(el.clientWidth, el.clientHeight, count)
      setSize((s) => (s.w === g.w && s.h === g.h && s.cols === g.cols && s.rows === g.rows ? s : g))
    }
    compute()
    const raf = requestAnimationFrame(compute)
    const ro = new ResizeObserver(compute)
    ro.observe(el)
    return () => {
      cancelAnimationFrame(raf)
      ro.disconnect()
    }
  }, [areaRef, count, active])
  return size
}

export function useLayout(core: RoomCore, conditions: LocalConditions) {
  const { peers, speaking, presentation, signal } = core
  const areaRef = useRef<HTMLDivElement>(null)
  // No telemóvel a sala abre no ORADOR: um retrato grande de quem fala e a
  // plateia em fila, em vez de uma grelha de selos ilegíveis.
  const [viewMode, setViewMode] = useState<ViewMode>(() =>
    typeof window !== 'undefined' && window.matchMedia?.('(max-width: 767px)').matches ? 'stage' : 'grid',
  )
  const [pinnedId, setPinnedId] = useState<string | null>(null)
  const [presLayout, setPresLayout] = useState<PresLayout>('side')
  const [hideSelf, setHideSelf] = useState(false)
  const [hideNoVideo, setHideNoVideo] = useState(false)
  const [gridPage, setGridPage] = useState(0)
  const [fullscreen, setFullscreen] = useState(false)
  const paginatedOnceRef = useRef(false)

  const togglePin = useCallback((id: string) => setPinnedId((cur) => (cur === id ? null : id)), [])

  // Destaque PARA TODOS (`spotlight`): decidido pelo anfitrião e guardado no
  // servidor (quem entra depois recebe-o). O pin local ganha-lhe — é a escolha
  // de quem está a ver —, mas sem pin local o palco segue o destaque.
  const [spotlightPeer, setSpotlightPeer] = useState<string | null>(null)
  useEffect(() => signal.onB1('spotlight', (m) => setSpotlightPeer(m.peer)), [signal])
  const spotlightId = spotlightPeer && spotlightPeer === core.meuPeerIdRef.current ? 'me' : spotlightPeer
  /** Quem escolheu a grelha por cima de um destaque não é arrastado de volta — até o destaque mudar. */
  const [ignorado, setIgnorado] = useState<string | null>(null)
  /** Só anfitrião (o servidor recusa aos outros). `'me'` é o meu peer_id. */
  const setSpotlight = useCallback(
    (id: string | null) => signal.sendB1({ type: 'spotlight', peer: id === 'me' ? core.meuPeerIdRef.current || null : id }),
    [signal, core.meuPeerIdRef],
  )

  useEffect(() => {
    const onFs = () => setFullscreen(!!document.fullscreenElement)
    document.addEventListener('fullscreenchange', onFs)
    return () => document.removeEventListener('fullscreenchange', onFs)
  }, [])

  const toggleFullscreen = useCallback(() => {
    if (document.fullscreenElement) void document.exitFullscreen()
    else void document.documentElement.requestFullscreen?.()
  }, [])

  const filteredPeers = hideNoVideo ? peers.filter((p) => !!p.stream?.getVideoTracks().length && p.camOn) : peers
  // O áudio NÃO é paginado — vive no AudioSink e ouve-se toda a gente.
  const pageCount = Math.max(1, Math.ceil(filteredPeers.length / TILES_PER_PAGE))
  const page = Math.min(gridPage, pageCount - 1)
  const visiblePeers =
    filteredPeers.length > TILES_PER_PAGE
      ? filteredPeers.slice(page * TILES_PER_PAGE, page * TILES_PER_PAGE + TILES_PER_PAGE)
      : filteredPeers
  const showSelf = !hideSelf
  const total = visiblePeers.length + (showSelf ? 1 : 0)
  const tileSize = useGridSize(areaRef, total, core.roomState === 'in')

  // O orador activo vai ao palco — inclui-me a MIM. O pin ganha a tudo; sem
  // pin local, vale o destaque do anfitrião (se a pessoa ainda estiver cá).
  const destaqueValido =
    spotlightId !== ignorado && (spotlightId === 'me' || (!!spotlightId && peers.some((p) => p.peerId === spotlightId)))
  const pinEfectivo = pinnedId ?? (destaqueValido ? spotlightId : null)
  const pinnedPeer = pinEfectivo && pinEfectivo !== 'me' ? peers.find((p) => p.peerId === pinEfectivo) ?? null : null
  const pinnedSelf = pinEfectivo === 'me'
  const remoteSpeaker = pinnedPeer ?? (pinnedSelf ? null : peers.find((p) => speaking.has(p.peerId)) ?? null)
  const stagePeer = pinnedPeer ?? remoteSpeaker ?? peers[0] ?? null
  const stageOnSelf = pinnedSelf || (!pinnedPeer && !remoteSpeaker && (speaking.has('me') || peers.length === 0))
  // Com pin, força-se o palco (é o «não trocar a toda a hora»).
  const effectiveViewMode: ViewMode = pinEfectivo ? 'stage' : viewMode

  // De quem precisamos MESMO de vídeo. Inclui sempre o palco e o fixado.
  const videoInterest = useMemo(() => {
    const source = filteredPeers.length > TILES_PER_PAGE ? visiblePeers : filteredPeers
    const ids = new Set(source.map((p) => p.peerId))
    if (stagePeer) ids.add(stagePeer.peerId)
    if (pinnedPeer) ids.add(pinnedPeer.peerId)
    return [...ids].sort()
  }, [visiblePeers, filteredPeers, stagePeer, pinnedPeer])

  // QUE qualidade pedir de cada um: a decisão vive em `layerPolicy.ts`; aqui
  // só se recolhe o que só o cliente sabe (tamanho real, palco, pin, ecrã).
  const videoQuality = useMemo(() => {
    const stageW = Math.round(window.innerWidth * 0.7)
    const tiles: TileSignal[] = videoInterest.map((peerId) => {
      const emPalco = effectiveViewMode === 'stage' && stagePeer?.peerId === peerId
      return {
        peerId,
        widthPx: emPalco ? stageW : tileSize.w,
        pinned: pinnedPeer?.peerId === peerId,
        onStage: emPalco,
        speaking: speaking.has(peerId),
        presenting: presentation?.peerId === peerId,
      }
    })
    return chooseLayers(tiles, conditions)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [videoInterest.join(','), tileSize.w, effectiveViewMode, stagePeer?.peerId, pinnedPeer?.peerId, presentation?.peerId, speaking, conditions])

  useEffect(() => {
    if (core.roomState !== 'in' || core.topology !== 'sfu') return
    // Uma sala que paginou uma vez informa sempre, senão o servidor ficava
    // preso na última página. A qualidade informa-se desde o início.
    if (filteredPeers.length > TILES_PER_PAGE) paginatedOnceRef.current = true
    signal.send({ type: 'video-interest', peers: videoInterest, quality: videoQuality })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [videoInterest.join(','), JSON.stringify(videoQuality), core.roomState, core.topology, filteredPeers.length])

  return {
    areaRef,
    viewMode,
    setViewMode,
    pinnedId,
    setPinnedId,
    togglePin,
    /** Destaque para todos (`'me'` quando sou eu), válido ou não. */
    spotlightId: destaqueValido ? spotlightId : null,
    setSpotlight,
    ignoreSpotlight: () => setIgnorado(spotlightId),
    presLayout,
    setPresLayout,
    hideSelf,
    setHideSelf,
    hideNoVideo,
    setHideNoVideo,
    fullscreen,
    toggleFullscreen,
    filteredPeers,
    pageCount,
    page,
    setGridPage,
    visiblePeers,
    showSelf,
    total,
    tileSize,
    pinnedPeer,
    stagePeer,
    stageOnSelf,
    effectiveViewMode,
  }
}

export type Layout = ReturnType<typeof useLayout>
