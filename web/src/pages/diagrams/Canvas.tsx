/**
 * Área de desenho: SVG com vista (deslocar e ampliar), arrastar elementos,
 * redimensionar, ligar pelo puxador, desenhar à mão e apagar traços.
 *
 * O documento é do pai. Durante um gesto o pai recebe `onLive` (sem
 * histórico); no fim recebe `onCommit(novo, antes)` — é assim que um arrasto
 * inteiro se desfaz com um só Ctrl+Z.
 */
import { KeyboardEvent as ReactKeyboardEvent, PointerEvent as ReactPointerEvent, RefObject, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { cx } from '../../ui/kit'
import { Box, center, clampZoom, contains, edgeSegment, lassoPick, moveSelection, nodeBox, Pt } from './geometry'
import { GROUP_PAD } from './exporters'
import { expandPick, groupOfItem, groupsOf, pickBox, pickSize, strokeBox, togglePick } from './groups'
import { CONTAINERS, DiagramDoc, DNode, EdgeType, FIXED_SIZE, QuickShape, Stroke, uid } from './model'
import { EdgeShape, NodeShape, strokeD, strokePath } from './shapes'
import { FONT, INK, MARKER } from './paint'

export type Sel = { kind: 'node' | 'edge' | 'stroke'; id: string }
export type Tool =
  | { kind: 'select' }
  | { kind: 'edge'; edge: EdgeType; key?: string }
  | { kind: 'pen' }
  | { kind: 'marker' }
  | { kind: 'eraser' }
  | { kind: 'lasso' }
  | { kind: 'shape'; shape: QuickShape }
  /** v5 — mini-barra: mover a selecção (M) e mão (H). Seleccionar (V) é `select`. */
  | { kind: 'move' }
  | { kind: 'hand' }

/** Como se chegou à selecção múltipla (o inspector di-lo, como o template). */
export type MultiVia = 'laco' | 'caixa' | 'clique'

/** Selecção múltipla (laço, caixa, ⇧clique, grupo): elementos e traços. */
export interface Multi {
  nodes: string[]
  strokes: string[]
}
export interface View {
  x: number
  y: number
  k: number
}

export const PALETTE_MIME = 'application/x-delonix-diagram'

const NO_RESIZE = new Set<DNode['type']>(['startEvent', 'intermediateEvent', 'endEvent', 'gateway', 'dataObject', ...FIXED_SIZE])

/** Mensagens que se arrastam na vertical ao longo das linhas de vida. */
const SEQ_MESSAGES = new Set<EdgeType>(['message', 'reply', 'lostMessage', 'foundMessage'])

/**
 * Elementos que viajam com outro sem ele ser contentor: portos na borda de um
 * componente, barras de activação na linha de vida, eventos de fronteira na
 * actividade.
 */
function carries(host: DNode, m: DNode): boolean {
  const c = center(nodeBox(m))
  const b = nodeBox(host)
  const grown = { x: b.x - 10, y: b.y - 10, w: b.w + 20, h: b.h + 20 }
  if (host.type === 'component') return m.type === 'port' && contains(grown, c)
  if (host.type === 'lifeline') return m.type === 'activation' && contains(grown, c)
  if (host.type === 'task' || host.type === 'subProcess') {
    const r = m.w / 2
    return m.type === 'intermediateEvent' && !!m.props.boundary && contains({ x: b.x - r, y: b.y - r, w: b.w + 2 * r, h: b.h + 2 * r }, c)
  }
  return false
}
const WIDTH_ONLY = new Set<DNode['type']>(['class', 'interface', 'enum'])

type Gesture =
  | { t: 'drag'; start: Pt; orig: Map<string, Pt>; before: DiagramDoc; moved: boolean }
  | { t: 'resize'; id: string; start: Pt; orig: { w: number; h: number; len: number; lastLane: number }; before: DiagramDoc; moved: boolean }
  | { t: 'offset'; id: string; start: Pt; orig: number; before: DiagramDoc; moved: boolean }
  | { t: 'connect'; from: string; start: Pt }
  | { t: 'pan'; sx: number; sy: number; orig: View }
  | { t: 'pinch'; d0: number; mid0: Pt; orig: View }
  | { t: 'pen'; points: number[] }
  | { t: 'erase' }
  | { t: 'shape'; a: Pt; b: Pt }
  | { t: 'lasso'; points: number[] }
  | { t: 'moveMulti'; start: Pt; pick: Multi; before: DiagramDoc; moved: boolean; dx: number; dy: number }
  /** `click`: um contentor por baixo — um clique sem arrastar escolhe-o. */
  | { t: 'box'; a: Pt; b: Pt; add: Multi | null; click?: string }

const snap = (v: number) => Math.round(v / 4) * 4

export default function Canvas({
  doc,
  selection,
  tool,
  view,
  svgRef,
  penColor,
  penWidth,
  penOpacity,
  multi,
  typeLabel,
  subLabel,
  onView,
  onSelect,
  onLive,
  onCommit,
  onConnect,
  onDropPalette,
  onMulti,
}: {
  doc: DiagramDoc
  selection: Sel | null
  tool: Tool
  view: View
  svgRef: RefObject<SVGSVGElement | null>
  penColor: string
  penWidth: number
  penOpacity: number
  multi: Multi | null
  typeLabel: (n: DNode) => string
  subLabel: (n: DNode) => string | undefined
  onView: (v: View) => void
  onSelect: (s: Sel | null) => void
  onLive: (d: DiagramDoc) => void
  onCommit: (next: DiagramDoc, before: DiagramDoc) => void
  onConnect: (from: DNode, to: DNode, offset: number) => void
  onDropPalette: (key: string, at: Pt) => void
  onMulti: (m: Multi | null, via?: MultiVia) => void
}) {
  const { t } = useTranslation()
  const wrapRef = useRef<HTMLDivElement>(null)
  const gesture = useRef<Gesture | null>(null)
  const pointers = useRef(new Map<number, Pt>())
  const [ghost, setGhost] = useState<{ a: Pt; b: Pt } | null>(null)
  const [ink, setInk] = useState<number[] | null>(null)
  const [draft, setDraft] = useState<Stroke | null>(null)
  const [lasso, setLasso] = useState<number[] | null>(null)
  const [marquee, setMarquee] = useState<{ a: Pt; b: Pt } | null>(null)
  const lassoAdd = useRef<Multi | null>(null)
  const docRef = useRef(doc)
  docRef.current = doc
  const viewRef = useRef(view)
  viewRef.current = view

  const toWorld = (clientX: number, clientY: number): Pt => {
    const r = svgRef.current!.getBoundingClientRect()
    const v = viewRef.current
    return { x: (clientX - r.left - v.x) / v.k, y: (clientY - r.top - v.y) / v.k }
  }

  // Roda: Ctrl/⌘ amplia à volta do cursor; sem modificador, desloca.
  useEffect(() => {
    const el = wrapRef.current
    if (!el) return
    const onWheel = (e: WheelEvent) => {
      e.preventDefault()
      const v = viewRef.current
      if (e.ctrlKey || e.metaKey) {
        const r = el.getBoundingClientRect()
        const px = e.clientX - r.left
        const py = e.clientY - r.top
        const k = clampZoom(v.k * Math.exp(-e.deltaY * 0.0022))
        onView({ k, x: px - ((px - v.x) * k) / v.k, y: py - ((py - v.y) * k) / v.k })
      } else {
        onView({ ...v, x: v.x - e.deltaX, y: v.y - e.deltaY })
      }
    }
    el.addEventListener('wheel', onWheel, { passive: false })
    return () => el.removeEventListener('wheel', onWheel)
  }, [onView])

  function capture(e: ReactPointerEvent) {
    try {
      svgRef.current?.setPointerCapture(e.pointerId)
    } catch {
      /* ponteiro já libertado */
    }
  }

  /** A selecção corrente como `Multi` (a múltipla, ou o elemento/traço sozinho). */
  function currentPick(): Multi | null {
    if (multi) return multi
    if (selection?.kind === 'node') return expandPick(docRef.current, { nodes: [selection.id], strokes: [] })
    if (selection?.kind === 'stroke') return expandPick(docRef.current, { nodes: [], strokes: [selection.id] })
    return null
  }

  /** ⇧clique: junta ou tira o item (ou o grupo dele) da selecção múltipla. */
  function toggleItem(kind: 'node' | 'stroke', id: string) {
    const next = togglePick(docRef.current, currentPick(), kind, id)
    onSelect(null)
    onMulti(pickSize(next) > 0 ? next : null, 'clique')
  }

  /** Clicar num membro de um grupo escolhe o grupo inteiro e começa a arrastá-lo. */
  function pickGroupOf(e: ReactPointerEvent, kind: 'node' | 'stroke', id: string): boolean {
    const g = groupOfItem(docRef.current, kind, id)
    if (!g) return false
    const pick = { nodes: [...g.nodes], strokes: [...g.strokes] }
    onSelect(null)
    onMulti(pick, 'clique')
    startMove(e, pick)
    return true
  }

  function startNode(e: ReactPointerEvent, n: DNode) {
    if (e.button !== 0) return
    // A mão desloca a vista por cima de tudo: o evento segue para o fundo.
    if (tool.kind === 'hand') return
    e.stopPropagation()
    pointers.current.set(e.pointerId, { x: e.clientX, y: e.clientY })
    const p = toWorld(e.clientX, e.clientY)
    if (tool.kind === 'edge') {
      gesture.current = { t: 'connect', from: n.id, start: p }
      setGhost({ a: center(nodeBox(n)), b: p })
      capture(e)
      return
    }
    if (tool.kind === 'pen' || tool.kind === 'marker' || tool.kind === 'eraser' || tool.kind === 'shape') return
    if (e.shiftKey && (tool.kind === 'select' || tool.kind === 'lasso')) {
      toggleItem('node', n.id)
      return
    }
    if (multi && multi.nodes.includes(n.id)) {
      startMove(e, multi)
      return
    }
    if (tool.kind === 'move') {
      const cur = currentPick()
      if (cur && cur.nodes.includes(n.id)) startMove(e, cur)
      else if (!pickGroupOf(e, 'node', n.id)) {
        onMulti(null)
        onSelect({ kind: 'node', id: n.id })
        startMove(e, { nodes: [n.id], strokes: [] })
      }
      return
    }
    if (tool.kind === 'lasso') return
    // v5: dentro de uma piscina (ou pacote, fronteira…) arrastar desenha a caixa
    // de selecção — o template selecciona assim três tarefas da mesma piscina.
    // O contentor escolhe-se com um clique e arrasta-se depois de escolhido.
    if (CONTAINERS.has(n.type) && !(selection?.kind === 'node' && selection.id === n.id) && e.pointerType !== 'touch' && !groupOfItem(docRef.current, 'node', n.id)) {
      onMulti(null)
      onSelect(null)
      gesture.current = { t: 'box', a: p, b: p, add: null, click: n.id }
      setMarquee({ a: p, b: p })
      capture(e)
      return
    }
    if (pickGroupOf(e, 'node', n.id)) return
    onMulti(null)
    onSelect({ kind: 'node', id: n.id })
    const orig = new Map<string, Pt>([[n.id, { x: n.x, y: n.y }]])
    // Um contentor leva consigo o que está lá dentro.
    if (CONTAINERS.has(n.type)) {
      const box = nodeBox(n)
      for (const m of docRef.current.nodes) {
        if (m.id !== n.id && contains(box, center(nodeBox(m))) && !(m.type === 'pool' && n.type !== 'pool')) orig.set(m.id, { x: m.x, y: m.y })
      }
    }
    for (const host of [...docRef.current.nodes.filter((x) => orig.has(x.id))]) {
      for (const m of docRef.current.nodes) if (!orig.has(m.id) && carries(host, m)) orig.set(m.id, { x: m.x, y: m.y })
    }
    gesture.current = { t: 'drag', start: p, orig, before: docRef.current, moved: false }
    capture(e)
  }

  function startHandle(e: ReactPointerEvent, n: DNode, kind: 'connect' | 'resize') {
    if (e.button !== 0) return
    e.stopPropagation()
    const p = toWorld(e.clientX, e.clientY)
    if (kind === 'connect') {
      gesture.current = { t: 'connect', from: n.id, start: p }
      setGhost({ a: p, b: p })
    } else {
      const lanes = n.props.lanes ?? []
      gesture.current = {
        t: 'resize',
        id: n.id,
        start: p,
        orig: { w: n.w, h: n.h, len: n.props.length ?? 240, lastLane: lanes.length ? lanes[lanes.length - 1].size : 0 },
        before: docRef.current,
        moved: false,
      }
    }
    capture(e)
  }

  function startMove(e: ReactPointerEvent, pick: Multi) {
    e.stopPropagation()
    gesture.current = { t: 'moveMulti', start: toWorld(e.clientX, e.clientY), pick, before: docRef.current, moved: false, dx: 0, dy: 0 }
    capture(e)
  }

  function startStroke(e: ReactPointerEvent, id: string) {
    if (e.button !== 0 || (tool.kind !== 'select' && tool.kind !== 'lasso' && tool.kind !== 'move')) return
    if (e.shiftKey && tool.kind !== 'move') {
      e.stopPropagation()
      toggleItem('stroke', id)
      return
    }
    if (multi && multi.strokes.includes(id)) {
      startMove(e, multi)
      return
    }
    if (pickGroupOf(e, 'stroke', id)) return
    onMulti(null)
    onSelect({ kind: 'stroke', id })
    startMove(e, { nodes: [], strokes: [id] })
  }

  function startEdge(e: ReactPointerEvent, id: string) {
    if (e.button !== 0 || tool.kind !== 'select') return
    e.stopPropagation()
    onSelect({ kind: 'edge', id })
    const edge = docRef.current.edges.find((x) => x.id === id)
    if (edge && SEQ_MESSAGES.has(edge.type)) {
      gesture.current = { t: 'offset', id, start: toWorld(e.clientX, e.clientY), orig: edge.offset ?? 40, before: docRef.current, moved: false }
      capture(e)
    }
  }

  function startBackground(e: ReactPointerEvent) {
    pointers.current.set(e.pointerId, { x: e.clientX, y: e.clientY })
    if (pointers.current.size === 2) {
      const [a, b] = [...pointers.current.values()]
      gesture.current = { t: 'pinch', d0: Math.hypot(a.x - b.x, a.y - b.y), mid0: { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 }, orig: viewRef.current }
      setInk(null)
      capture(e)
      return
    }
    if (e.button !== 0 && e.pointerType === 'mouse' && e.button !== 1) return
    const p = toWorld(e.clientX, e.clientY)
    if ((tool.kind === 'pen' || tool.kind === 'marker') && e.button === 0) {
      gesture.current = { t: 'pen', points: [Math.round(p.x), Math.round(p.y)] }
      setInk([Math.round(p.x), Math.round(p.y)])
    } else if (tool.kind === 'shape' && e.button === 0) {
      const q = { x: Math.round(p.x), y: Math.round(p.y) }
      gesture.current = { t: 'shape', a: q, b: q }
    } else if (tool.kind === 'lasso' && e.button === 0) {
      if (!e.shiftKey) onMulti(null)
      lassoAdd.current = e.shiftKey ? currentPick() : null
      gesture.current = { t: 'lasso', points: [Math.round(p.x), Math.round(p.y)] }
      setLasso([Math.round(p.x), Math.round(p.y)])
    } else if (tool.kind === 'eraser' && e.button === 0) {
      gesture.current = { t: 'erase' }
      eraseAt(e.clientX, e.clientY)
    } else if (tool.kind === 'select' && e.button === 0 && e.pointerType !== 'touch') {
      // v5: arrastar no fundo desenha a caixa de selecção (a mão, H, desloca).
      const add = e.shiftKey ? currentPick() : null
      if (!e.shiftKey) {
        onSelect(null)
        onMulti(null)
      }
      gesture.current = { t: 'box', a: p, b: p, add }
      setMarquee({ a: p, b: p })
    } else if (tool.kind === 'move' && e.button === 0 && currentPick()) {
      startMove(e, currentPick()!)
      return
    } else {
      if (tool.kind === 'select') {
        onSelect(null)
        onMulti(null)
      }
      gesture.current = { t: 'pan', sx: e.clientX, sy: e.clientY, orig: viewRef.current }
    }
    capture(e)
  }

  function eraseAt(clientX: number, clientY: number) {
    const el = document.elementFromPoint(clientX, clientY)?.closest('[data-stroke-id]')
    const id = el?.getAttribute('data-stroke-id')
    if (!id) return
    const before = docRef.current
    onCommit({ ...before, strokes: before.strokes.filter((s) => s.id !== id) }, before)
  }

  function onMove(e: ReactPointerEvent) {
    if (pointers.current.has(e.pointerId)) pointers.current.set(e.pointerId, { x: e.clientX, y: e.clientY })
    const g = gesture.current
    if (!g) return
    const p = () => toWorld(e.clientX, e.clientY)
    const d = docRef.current
    switch (g.t) {
      case 'drag': {
        const q = p()
        const dx = snap(q.x - g.start.x)
        const dy = snap(q.y - g.start.y)
        if (!g.moved && Math.abs(dx) + Math.abs(dy) < 4) return
        g.moved = true
        onLive({ ...d, nodes: d.nodes.map((n) => (g.orig.has(n.id) ? { ...n, x: g.orig.get(n.id)!.x + dx, y: g.orig.get(n.id)!.y + dy } : n)) })
        break
      }
      case 'resize': {
        const q = p()
        const dx = snap(q.x - g.start.x)
        const dy = snap(q.y - g.start.y)
        g.moved = true
        onLive({
          ...d,
          nodes: d.nodes.map((n) => {
            if (n.id !== g.id) return n
            if (n.type === 'lifeline') return { ...n, props: { ...n.props, length: Math.max(60, g.orig.len + dy) } }
            const w = Math.max(40, g.orig.w + dx)
            if (WIDTH_ONLY.has(n.type)) return { ...n, w: Math.max(110, w) }
            const lanes = n.props.lanes ?? []
            if (n.type === 'pool' && lanes.length) {
              const last = { ...lanes[lanes.length - 1], size: Math.max(60, g.orig.lastLane + dy) }
              return { ...n, w: Math.max(200, w), props: { ...n.props, lanes: [...lanes.slice(0, -1), last] } }
            }
            return { ...n, w, h: Math.max(24, g.orig.h + dy) }
          }),
        })
        break
      }
      case 'offset': {
        const dy = snap(p().y - g.start.y)
        g.moved = true
        onLive({ ...d, edges: d.edges.map((x) => (x.id === g.id ? { ...x, offset: Math.max(8, g.orig + dy) } : x)) })
        break
      }
      case 'connect':
        setGhost((gh) => (gh ? { a: gh.a, b: p() } : gh))
        break
      case 'pan':
        onView({ ...g.orig, x: g.orig.x + e.clientX - g.sx, y: g.orig.y + e.clientY - g.sy })
        break
      case 'pinch': {
        if (pointers.current.size < 2) return
        const [a, b] = [...pointers.current.values()]
        const dist = Math.hypot(a.x - b.x, a.y - b.y)
        const r = svgRef.current!.getBoundingClientRect()
        const mid = { x: (a.x + b.x) / 2 - r.left, y: (a.y + b.y) / 2 - r.top }
        const k = clampZoom(g.orig.k * (dist / Math.max(1, g.d0)))
        const m0 = { x: g.mid0.x - r.left, y: g.mid0.y - r.top }
        onView({ k, x: mid.x - ((m0.x - g.orig.x) * k) / g.orig.k, y: mid.y - ((m0.y - g.orig.y) * k) / g.orig.k })
        break
      }
      case 'pen': {
        const q = p()
        const n = g.points.length
        if (Math.hypot(q.x - g.points[n - 2], q.y - g.points[n - 1]) < 2) return
        g.points.push(Math.round(q.x), Math.round(q.y))
        setInk([...g.points])
        break
      }
      case 'erase':
        eraseAt(e.clientX, e.clientY)
        break
      case 'shape': {
        const q = p()
        g.b = { x: Math.round(q.x), y: Math.round(q.y) }
        setDraft(shapeStroke(g.a, g.b, tool.kind === 'shape' ? tool.shape : 'rect'))
        break
      }
      case 'lasso': {
        const q = p()
        const n = g.points.length
        if (Math.hypot(q.x - g.points[n - 2], q.y - g.points[n - 1]) < 3) return
        g.points.push(Math.round(q.x), Math.round(q.y))
        setLasso([...g.points])
        break
      }
      case 'box': {
        g.b = p()
        setMarquee({ a: g.a, b: g.b })
        break
      }
      case 'moveMulti': {
        const q = p()
        const dx = snap(q.x - g.start.x)
        const dy = snap(q.y - g.start.y)
        if (!g.moved && Math.abs(dx) + Math.abs(dy) < 4) return
        g.moved = true
        g.dx = dx
        g.dy = dy
        onLive(moveSelection(g.before, g.pick, dx, dy))
        break
      }
    }
  }

  function shapeStroke(a: Pt, b: Pt, shape: QuickShape): Stroke {
    return { id: 'draft', points: [a.x, a.y, b.x, b.y], color: penColor, width: penWidth, opacity: penOpacity < 1 ? penOpacity : undefined, shape }
  }

  function onUp(e: ReactPointerEvent) {
    pointers.current.delete(e.pointerId)
    const g = gesture.current
    gesture.current = null
    if (!g) return
    const d = docRef.current
    switch (g.t) {
      case 'drag':
      case 'resize':
      case 'offset':
        if (g.moved) onCommit(d, g.before)
        break
      case 'connect': {
        setGhost(null)
        const el = document.elementFromPoint(e.clientX, e.clientY)?.closest('[data-node-id]')
        const toId = el?.getAttribute('data-node-id')
        const from = d.nodes.find((n) => n.id === g.from)
        const to = d.nodes.find((n) => n.id === toId)
        if (from && to) {
          const top = Math.min(from.y, to.y) + from.h
          onConnect(from, to, Math.max(8, snap(g.start.y - top)))
        }
        break
      }
      case 'pen': {
        setInk(null)
        if (g.points.length >= 2) {
          const marker = tool.kind === 'marker'
          const opacity = marker ? MARKER.opacity : penOpacity
          const s: Stroke = { id: uid('s'), points: g.points, color: penColor, width: marker ? MARKER.width : penWidth }
          if (opacity < 1) s.opacity = opacity
          onCommit({ ...d, strokes: [...d.strokes, s] }, d)
        }
        break
      }
      case 'shape': {
        setDraft(null)
        if (tool.kind === 'shape' && Math.hypot(g.b.x - g.a.x, g.b.y - g.a.y) >= 6) {
          const s = { ...shapeStroke(g.a, g.b, tool.shape), id: uid('s') }
          onCommit({ ...d, strokes: [...d.strokes, s] }, d)
          onSelect({ kind: 'stroke', id: s.id })
        }
        break
      }
      case 'lasso': {
        setLasso(null)
        const pick = merge(lassoAdd.current, expandPick(d, lassoPick(d, g.points)))
        lassoAdd.current = null
        onMulti(pickSize(pick) > 0 ? pick : null, 'laco')
        onSelect(null)
        break
      }
      case 'box': {
        setMarquee(null)
        const r = rectOf(g.a, g.b)
        // Um clique sem arrastar só limpa a selecção (já limpa ao carregar) — ou escolhe o contentor clicado.
        if (r.w * viewRef.current.k < 4 && r.h * viewRef.current.k < 4) {
          if (g.click) onSelect({ kind: 'node', id: g.click })
          break
        }
        const pick = merge(g.add, expandPick(d, boxPick(d, r)))
        onSelect(null)
        onMulti(pickSize(pick) > 0 ? pick : null, 'caixa')
        break
      }
      case 'moveMulti':
        if (g.moved) onCommit(d, g.before)
        break
    }
  }

  function nodeKey(e: ReactKeyboardEvent, n: DNode) {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault()
      onSelect({ kind: 'node', id: n.id })
    }
  }

  const byId = new Map(doc.nodes.map((n) => [n.id, n]))
  const containers = doc.nodes.filter((n) => CONTAINERS.has(n.type))
  const others = doc.nodes.filter((n) => !CONTAINERS.has(n.type))
  const selNode = selection?.kind === 'node' ? byId.get(selection.id) : undefined
  const selBox = selNode ? nodeBox(selNode) : null

  const renderNode = (n: DNode) => (
    <g
      key={n.id}
      data-node-id={n.id}
      role="button"
      tabIndex={0}
      aria-label={t('diagrams.canvas.elemento', { tipo: typeLabel(n), nome: n.name || n.props.text || t('diagrams.semNome') })}
      aria-pressed={selection?.kind === 'node' && selection.id === n.id}
      className={cx('dg-node', tool.kind === 'edge' && 'is-target')}
      onPointerDown={(e) => startNode(e, n)}
      onDoubleClick={() => {
        // Dentro de um grupo, o duplo clique escolhe só o elemento (para o editar).
        if (tool.kind !== 'select' || !groupOfItem(docRef.current, 'node', n.id)) return
        onMulti(null)
        onSelect({ kind: 'node', id: n.id })
      }}
      onKeyDown={(e) => nodeKey(e, n)}
      onFocus={() => tool.kind === 'select' && onSelect({ kind: 'node', id: n.id })}
    >
      <NodeShape n={n} sub={subLabel(n)} />
    </g>
  )

  return (
    <div
      ref={wrapRef}
      className={cx('dg-canvas', `is-${tool.kind}`)}
      onDragOver={(e) => {
        if (e.dataTransfer.types.includes(PALETTE_MIME)) {
          e.preventDefault()
          e.dataTransfer.dropEffect = 'copy'
        }
      }}
      onDrop={(e) => {
        const key = e.dataTransfer.getData(PALETTE_MIME)
        if (!key) return
        e.preventDefault()
        onDropPalette(key, toWorld(e.clientX, e.clientY))
      }}
    >
      <svg
        ref={svgRef}
        className="dg-svg"
        fontFamily={FONT}
        role="application"
        aria-label={t('diagrams.canvas.rotulo')}
        onPointerDown={startBackground}
        onPointerMove={onMove}
        onPointerUp={onUp}
        onPointerCancel={onUp}
      >
        <g data-content transform={`translate(${view.x} ${view.y}) scale(${view.k})`}>
          {containers.map(renderNode)}
          {groupsOf(doc).map((g) => {
            // A moldura do grupo é parte do documento (sai no SVG/PNG): cores do papel, não classes.
            const b = pickBox(doc, { nodes: g.nodes, strokes: g.strokes })
            if (!b) return null
            return (
              <g key={g.id} data-group-id={g.id} pointerEvents="none">
                <rect x={b.x - GROUP_PAD} y={b.y - GROUP_PAD} width={b.w + GROUP_PAD * 2} height={b.h + GROUP_PAD * 2} rx={4} fill="none" stroke={INK.muted} strokeWidth={1} strokeDasharray="6 4" />
                {g.name.trim() && (
                  <text x={b.x - GROUP_PAD + 2} y={b.y - GROUP_PAD - 5} fontSize={10.5} fontWeight={600} fill={INK.muted}>
                    {g.name}
                  </text>
                )}
              </g>
            )
          })}
          {doc.edges.map((e) => {
            const seg = edgeSegment(doc, e)
            if (!seg) return null
            const selected = selection?.kind === 'edge' && selection.id === e.id
            return (
              <g key={e.id} data-edge-id={e.id} className={cx('dg-edge', selected && 'is-selected')} onPointerDown={(ev) => startEdge(ev, e.id)}>
                <path data-ui d={`M${seg.a.x} ${seg.a.y}L${seg.b.x} ${seg.b.y}`} className="dg-edge__hit" />
                {selected && <path data-ui d={`M${seg.a.x} ${seg.a.y}L${seg.b.x} ${seg.b.y}`} className="dg-edge__sel" />}
                <EdgeShape e={e} a={seg.a} b={seg.b} sourceType={byId.get(e.from)?.type} selfLoop={e.from === e.to && !SEQ_MESSAGES.has(e.type)} />
              </g>
            )
          })}
          {others.map(renderNode)}
          {doc.strokes.map((s) => (
            <g key={s.id} data-stroke-id={s.id} className={cx('dg-stroke', selection?.kind === 'stroke' && selection.id === s.id && 'is-selected')} onPointerDown={(e) => startStroke(e, s.id)}>
              <path data-ui d={strokeD(s)} className="dg-stroke__hit" />
              <path d={strokeD(s)} fill="none" stroke={s.color} strokeWidth={s.width} strokeOpacity={s.opacity} strokeLinecap="round" strokeLinejoin="round" />
            </g>
          ))}
          {ink && (
            <path
              data-ui
              d={strokePath(ink)}
              fill="none"
              stroke={penColor}
              strokeWidth={tool.kind === 'marker' ? MARKER.width : penWidth}
              strokeOpacity={tool.kind === 'marker' ? MARKER.opacity : penOpacity}
              strokeLinecap="round"
              strokeLinejoin="round"
              pointerEvents="none"
            />
          )}
          {draft && <path data-ui d={strokeD(draft)} fill="none" stroke={draft.color} strokeWidth={draft.width} strokeOpacity={draft.opacity} strokeLinecap="round" strokeLinejoin="round" pointerEvents="none" />}
          {lasso && <path data-ui d={`${strokePath(lasso)}Z`} className="dg-lasso" pointerEvents="none" />}
          {marquee && (() => {
            const r = rectOf(marquee.a, marquee.b)
            return <rect data-ui x={r.x} y={r.y} width={r.w} height={r.h} className="dg-lasso" pointerEvents="none" />
          })()}
          {multi &&
            doc.nodes
              .filter((n) => multi.nodes.includes(n.id))
              .map((n) => {
                // Realce de cada elemento escolhido (template: borda vermelha e anel de 2 px).
                const b = nodeBox(n)
                return (
                  <g key={`hl-${n.id}`} data-ui pointerEvents="none">
                    <rect x={b.x - 2} y={b.y - 2} width={b.w + 4} height={b.h + 4} fill="none" stroke={INK.accent} strokeOpacity={0.18} strokeWidth={4} vectorEffect="non-scaling-stroke" />
                    <rect x={b.x} y={b.y} width={b.w} height={b.h} fill="none" stroke={INK.accent} strokeWidth={1.5} vectorEffect="non-scaling-stroke" />
                  </g>
                )
              })}
          {selBoxOf(doc, selection, multi) && (() => {
            const b = selBoxOf(doc, selection, multi)!
            return (
              <g data-ui>
                <rect
                  x={b.x - 6}
                  y={b.y - 6}
                  width={b.w + 12}
                  height={b.h + 12}
                  className="dg-multi"
                  onPointerDown={(e) => {
                    if (e.button !== 0 || (tool.kind !== 'select' && tool.kind !== 'lasso' && tool.kind !== 'move')) return
                    if (e.shiftKey) return
                    startMove(e, multi ?? { nodes: [], strokes: selection?.kind === 'stroke' ? [selection.id] : [] })
                  }}
                />
                {[
                  [b.x - 6, b.y - 6],
                  [b.x + b.w + 6, b.y - 6],
                  [b.x - 6, b.y + b.h + 6],
                  [b.x + b.w + 6, b.y + b.h + 6],
                ].map(([hx, hy], i) => (
                  <rect key={i} x={hx - 3.5 / view.k} y={hy - 3.5 / view.k} width={7 / view.k} height={7 / view.k} className="dg-multi__handle" />
                ))}
              </g>
            )
          })()}
          {selNode && selBox && tool.kind === 'select' && (
            <g data-ui className="dg-sel">
              <rect x={selBox.x - 4} y={selBox.y - 4} width={selBox.w + 8} height={selBox.h + 8} className="dg-sel__box" />
              <circle
                cx={selBox.x + selBox.w + 4}
                cy={selNode.type === 'lifeline' ? selNode.y + selNode.h / 2 : selBox.y + selBox.h / 2}
                r={7 / view.k}
                className="dg-sel__connect"
                onPointerDown={(e) => startHandle(e, selNode, 'connect')}
              >
                <title>{t('diagrams.canvas.ligar')}</title>
              </circle>
              {!NO_RESIZE.has(selNode.type) && (
                <rect
                  x={selBox.x + selBox.w + 4 - 5 / view.k}
                  y={selBox.y + selBox.h + 4 - 5 / view.k}
                  width={10 / view.k}
                  height={10 / view.k}
                  className="dg-sel__resize"
                  onPointerDown={(e) => startHandle(e, selNode, 'resize')}
                >
                  <title>{t('diagrams.canvas.redimensionar')}</title>
                </rect>
              )}
            </g>
          )}
          {ghost && <path data-ui d={`M${ghost.a.x} ${ghost.a.y}L${ghost.b.x} ${ghost.b.y}`} className="dg-ghost" pointerEvents="none" />}
        </g>
      </svg>
    </div>
  )
}

/** Caixa da selecção múltipla, ou do traço seleccionado. */
function selBoxOf(doc: DiagramDoc, selection: Sel | null, multi: Multi | null): Box | null {
  const strokes = multi ? multi.strokes : selection?.kind === 'stroke' ? [selection.id] : []
  const nodes = multi ? multi.nodes : []
  if (strokes.length + nodes.length === 0) return null
  let x0 = Infinity
  let y0 = Infinity
  let x1 = -Infinity
  let y1 = -Infinity
  for (const s of doc.strokes) {
    if (!strokes.includes(s.id)) continue
    for (let i = 0; i < s.points.length; i += 2) {
      x0 = Math.min(x0, s.points[i])
      y0 = Math.min(y0, s.points[i + 1])
      x1 = Math.max(x1, s.points[i])
      y1 = Math.max(y1, s.points[i + 1])
    }
  }
  for (const n of doc.nodes) {
    if (!nodes.includes(n.id)) continue
    const b = nodeBox(n)
    x0 = Math.min(x0, b.x)
    y0 = Math.min(y0, b.y)
    x1 = Math.max(x1, b.x + b.w)
    y1 = Math.max(y1, b.y + b.h)
  }
  return Number.isFinite(x0) ? { x: x0, y: y0, w: x1 - x0, h: y1 - y0 } : null
}

const rectOf = (a: Pt, b: Pt): Box => ({ x: Math.min(a.x, b.x), y: Math.min(a.y, b.y), w: Math.abs(b.x - a.x), h: Math.abs(b.y - a.y) })

const merge = (a: Multi | null, b: Multi): Multi => (a ? { nodes: [...new Set([...a.nodes, ...b.nodes])], strokes: [...new Set([...a.strokes, ...b.strokes])] } : b)

/** Caixa de selecção: entra o que tem o CENTRO lá dentro (a mesma regra do laço). */
function boxPick(doc: DiagramDoc, r: Box): Multi {
  const inside = (p: Pt) => contains(r, p)
  return {
    // Um contentor (piscina, pacote…) só entra inteiro: arrastar lá dentro escolhe o que tem dentro, não ele.
    nodes: doc.nodes
      .filter((n) => {
        const b = nodeBox(n)
        return CONTAINERS.has(n.type) ? inside({ x: b.x, y: b.y }) && inside({ x: b.x + b.w, y: b.y + b.h }) : inside(center(b))
      })
      .map((n) => n.id),
    strokes: doc.strokes.filter((s) => {
      const b = strokeBox(s.points)
      return !!b && inside(center(b))
    }).map((s) => s.id),
  }
}
