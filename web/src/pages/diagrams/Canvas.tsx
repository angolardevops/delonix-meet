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
import { center, clampZoom, contains, edgeSegment, nodeBox, Pt } from './geometry'
import { CONTAINERS, DiagramDoc, DNode, EdgeType, Stroke, uid } from './model'
import { EdgeShape, NodeShape, strokePath } from './shapes'
import { FONT } from './paint'

export type Sel = { kind: 'node' | 'edge' | 'stroke'; id: string }
export type Tool = { kind: 'select' } | { kind: 'edge'; edge: EdgeType } | { kind: 'pen' } | { kind: 'eraser' }
export interface View {
  x: number
  y: number
  k: number
}

export const PALETTE_MIME = 'application/x-delonix-diagram'

const NO_RESIZE = new Set<DNode['type']>(['startEvent', 'intermediateEvent', 'endEvent', 'gateway', 'actor', 'dataObject'])
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

const snap = (v: number) => Math.round(v / 4) * 4

export default function Canvas({
  doc,
  selection,
  tool,
  view,
  svgRef,
  penColor,
  typeLabel,
  subLabel,
  onView,
  onSelect,
  onLive,
  onCommit,
  onConnect,
  onDropPalette,
}: {
  doc: DiagramDoc
  selection: Sel | null
  tool: Tool
  view: View
  svgRef: RefObject<SVGSVGElement | null>
  penColor: string
  typeLabel: (n: DNode) => string
  subLabel: (n: DNode) => string | undefined
  onView: (v: View) => void
  onSelect: (s: Sel | null) => void
  onLive: (d: DiagramDoc) => void
  onCommit: (next: DiagramDoc, before: DiagramDoc) => void
  onConnect: (from: DNode, to: DNode, offset: number) => void
  onDropPalette: (key: string, at: Pt) => void
}) {
  const { t } = useTranslation()
  const wrapRef = useRef<HTMLDivElement>(null)
  const gesture = useRef<Gesture | null>(null)
  const pointers = useRef(new Map<number, Pt>())
  const [ghost, setGhost] = useState<{ a: Pt; b: Pt } | null>(null)
  const [ink, setInk] = useState<number[] | null>(null)
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

  function startNode(e: ReactPointerEvent, n: DNode) {
    if (e.button !== 0) return
    e.stopPropagation()
    pointers.current.set(e.pointerId, { x: e.clientX, y: e.clientY })
    const p = toWorld(e.clientX, e.clientY)
    if (tool.kind === 'edge') {
      gesture.current = { t: 'connect', from: n.id, start: p }
      setGhost({ a: center(nodeBox(n)), b: p })
      capture(e)
      return
    }
    if (tool.kind === 'pen' || tool.kind === 'eraser') return
    onSelect({ kind: 'node', id: n.id })
    const orig = new Map<string, Pt>([[n.id, { x: n.x, y: n.y }]])
    // Um contentor leva consigo o que está lá dentro.
    if (CONTAINERS.has(n.type)) {
      const box = nodeBox(n)
      for (const m of docRef.current.nodes) {
        if (m.id !== n.id && contains(box, center(nodeBox(m))) && !(m.type === 'pool' && n.type !== 'pool')) orig.set(m.id, { x: m.x, y: m.y })
      }
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

  function startEdge(e: ReactPointerEvent, id: string) {
    if (e.button !== 0 || tool.kind !== 'select') return
    e.stopPropagation()
    onSelect({ kind: 'edge', id })
    const edge = docRef.current.edges.find((x) => x.id === id)
    if (edge && (edge.type === 'message' || edge.type === 'reply')) {
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
    if (tool.kind === 'pen' && e.button === 0) {
      gesture.current = { t: 'pen', points: [Math.round(p.x), Math.round(p.y)] }
      setInk([Math.round(p.x), Math.round(p.y)])
    } else if (tool.kind === 'eraser' && e.button === 0) {
      gesture.current = { t: 'erase' }
      eraseAt(e.clientX, e.clientY)
    } else {
      if (tool.kind === 'select') onSelect(null)
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
    }
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
          const s: Stroke = { id: uid('s'), points: g.points, color: penColor, width: 2.5 }
          onCommit({ ...d, strokes: [...d.strokes, s] }, d)
        }
        break
      }
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
          {doc.edges.map((e) => {
            const seg = edgeSegment(doc, e)
            if (!seg) return null
            const selected = selection?.kind === 'edge' && selection.id === e.id
            return (
              <g key={e.id} data-edge-id={e.id} className={cx('dg-edge', selected && 'is-selected')} onPointerDown={(ev) => startEdge(ev, e.id)}>
                <path data-ui d={`M${seg.a.x} ${seg.a.y}L${seg.b.x} ${seg.b.y}`} className="dg-edge__hit" />
                {selected && <path data-ui d={`M${seg.a.x} ${seg.a.y}L${seg.b.x} ${seg.b.y}`} className="dg-edge__sel" />}
                <EdgeShape e={e} a={seg.a} b={seg.b} sourceType={byId.get(e.from)?.type} selfLoop={e.from === e.to && e.type !== 'message' && e.type !== 'reply'} />
              </g>
            )
          })}
          {others.map(renderNode)}
          {doc.strokes.map((s) => (
            <g key={s.id} data-stroke-id={s.id} className="dg-stroke">
              <path data-ui d={strokePath(s.points)} className="dg-stroke__hit" />
              <path d={strokePath(s.points)} fill="none" stroke={s.color} strokeWidth={s.width} strokeLinecap="round" strokeLinejoin="round" />
            </g>
          ))}
          {ink && <path data-ui d={strokePath(ink)} fill="none" stroke={penColor} strokeWidth={2.5} strokeLinecap="round" strokeLinejoin="round" pointerEvents="none" />}
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
