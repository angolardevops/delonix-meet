/**
 * Quadro de diagramas — DelonixCanvasUML e DelonixCanvasBPMN do template, com
 * os separadores Arquitectura, Fluxograma e Livre na mesma gramática.
 *
 * Rota: `#/whiteboards/diagram/<id>` (sem id cria um novo; `?sala=<código>`
 * pré-associa a sala).
 *
 * O que funciona aqui, e onde vive:
 *  - o modelo editável fica no IndexedDB deste browser (`diagrams/store.ts`),
 *    com gravação automática;
 *  - «Guardar no storage» manda um PNG para a biblioteca da organização
 *    (POST /api/whiteboards), com a sala — é isso que o anexa às gravações;
 *  - validação, correcções determinísticas e as exportações XMI, PlantUML,
 *    .bpmn, SVG, PNG e JSON são feitas no browser.
 *
 * O que o template mostra e NÃO está aqui, por não haver servidor para isso:
 * avatares e cursores de quem está a editar (a sinalização só retransmite
 * traços livres da sala), «Gerar do modelo» por IA (não há rota), «Guardar em
 * MinIO / Nextcloud» (não há conector de escrita), guardar o modelo no
 * servidor e «Pôr no palco» (o Estúdio não tem ainda uma fonte de quadro).
 */
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { WhiteboardMeta } from '../api'
import { useShell } from '../components/shellContext'
import { DelonixSymbol, Icon } from '../ui/icons'
import { Alert, Button, cx, IconButton, Spinner } from '../ui/kit'
import '../ui/diagrams.css'
import Canvas, { Multi, Sel, Tool, View } from './diagrams/Canvas'
import { fileBase, parseJson, toBpmn, toC4PlantUml, toJson, toPlantUml, toXmi } from './diagrams/exporters'
import { clampZoom, contentBox, fitView, laneOf, nodeBox, Pt } from './diagrams/geometry'
import Inspector, { InspectorTab, tabsFor } from './diagrams/Inspector'
import {
  canConnect,
  CLASSIFIERS,
  CONTAINERS,
  DEdge,
  defaultEdgeType,
  DiagramDoc,
  DNode,
  EDGE_NOTATION,
  emptyDoc,
  FLOW_NODES,
  makeNode,
  NODE_NOTATION,
  Notation,
  NOTATIONS,
  PaletteItem,
  uid,
} from './diagrams/model'
import { PENS } from './diagrams/paint'
import Palette, { paletteItemByKey } from './diagrams/Palette'
import SaveDialog from './diagrams/SaveDialog'
import { downloadBlob, downloadText, pngFromSvg, svgFromCanvas } from './diagrams/snapshot'
import { getDiagram, putDiagram } from './diagrams/store'
import { applyFix, fixAll, Issue, issueParams, validate } from './diagrams/validate'
import { example, examplesFor } from './diagrams/examples'
import { ensureCatalog } from './diagrams/catalog'
import { loadCatalogLabels } from './diagrams/catalog/labels'
import { useCatalogVersion } from './diagrams/catalog/useCatalog'
import i18n from 'i18next'
import { resolveLang } from '../i18n'

type Load = { s: 'loading' } | { s: 'ready' } | { s: 'missing' } | { s: 'error' }
type Persist = 'idle' | 'saving' | 'saved' | 'error'
type Notice = { tone: 'success' | 'danger' | 'warning'; text: string; link?: boolean } | null
type ExportKind = 'xmi' | 'plantuml' | 'bpmn' | 'c4' | 'svg' | 'png' | 'json'

const HISTORY_MAX = 100

function roomFromHash(): string {
  const m = location.hash.match(/[?&]sala=([a-z-]+)/)
  return m ? m[1] : ''
}

/**
 * `?tipo=bpmn` escolhe a notação de um quadro novo; `?exemplo=1` começa do
 * (primeiro) exemplo dela, `?exemplo=cloud` de um exemplo pelo nome.
 */
function startFromHash(): { notation: Notation; example: string | null } {
  const tipo = location.hash.match(/[?&]tipo=([a-z]+)/)?.[1] as Notation | undefined
  const notation = tipo && NOTATIONS.includes(tipo) ? tipo : 'uml'
  const asked = location.hash.match(/[?&]exemplo=([a-z0-9]+)/)?.[1]
  const variants = examplesFor(notation)
  return { notation, example: !asked ? null : asked === '1' ? variants[0] ?? null : variants.includes(asked) ? asked : null }
}

function isTyping(el: EventTarget | null): boolean {
  const e = el as HTMLElement | null
  if (!e) return false
  return e.tagName === 'INPUT' || e.tagName === 'TEXTAREA' || e.tagName === 'SELECT' || e.isContentEditable
}

export default function Diagram({ id }: { id: string | null }) {
  const { t } = useTranslation()
  const { setNavOpen } = useShell()
  // Os rótulos e desenhos do catálogo chegam depois: re-desenha quando chegam.
  useCatalogVersion()
  const [load, setLoad] = useState<Load>({ s: 'loading' })
  const [doc, setDoc] = useState<DiagramDoc | null>(null)
  const past = useRef<DiagramDoc[]>([])
  const future = useRef<DiagramDoc[]>([])
  const lastKey = useRef<{ key: string; at: number } | null>(null)
  const [, setHistoryTick] = useState(0)
  const [selection, setSelection] = useState<Sel | null>(null)
  const [tool, setTool] = useState<Tool>({ kind: 'select' })
  const [view, setView] = useState<View>({ x: 40, y: 40, k: 1 })
  const [tab, setTab] = useState<InspectorTab>('element')
  const [penColor, setPenColor] = useState<string>(PENS.ink)
  const [penWidth, setPenWidth] = useState(2.5)
  const [penOpacity, setPenOpacity] = useState(1)
  const [multi, setMulti] = useState<Multi | null>(null)
  const [persist, setPersist] = useState<Persist>('idle')
  const [notice, setNotice] = useState<Notice>(null)
  const [saving, setSaving] = useState(false)
  const [menu, setMenu] = useState(false)
  const [drawer, setDrawer] = useState<'palette' | 'inspector' | null>(null)
  const [showValidation, setShowValidation] = useState(false)
  const svgRef = useRef<SVGSVGElement>(null)
  const fileRef = useRef<HTMLInputElement>(null)
  const loadedId = useRef<string | null>(null)
  const createdFor = useRef('')
  const fitted = useRef(false)

  // ---------------------------------------------------------------- carregar
  useEffect(() => {
    if (id && id === loadedId.current) return
    let live = true
    if (!id) {
      // O duplo efeito do StrictMode não pode criar dois diagramas; um
      // «novo» pedido noutro endereço cria outro.
      if (loadedId.current && createdFor.current === location.hash) return
      createdFor.current = location.hash
      past.current = []
      future.current = []
      fitted.current = false
      const start = startFromHash()
      const base = emptyDoc(uid('d'), t('diagrams.semTitulo'), start.notation, roomFromHash())
      const sample = start.example ? example(start.example, (k) => t(`diagrams.exemplos.${k}`)) : null
      const fresh = sample ? { ...base, title: t(`diagrams.exemplos.${start.example}.nome`), ...sample } : base
      loadedId.current = fresh.id
      setDoc(fresh)
      // Um exemplo abre com o elemento em destaque seleccionado, como o template.
      const featured = fresh.nodes.find((n) => n.props.emphasis)
      setSelection(featured ? { kind: 'node', id: featured.id } : null)
      setLoad({ s: 'ready' })
      putDiagram(fresh)
        .then(() => location.replace(`#/whiteboards/diagram/${fresh.id}`))
        .catch(() => setPersist('error'))
      return
    }
    setLoad({ s: 'loading' })
    getDiagram(id)
      .then((d) => {
        if (!live) return
        loadedId.current = id
        if (!d) {
          setLoad({ s: 'missing' })
          return
        }
        past.current = []
        future.current = []
        fitted.current = false
        setDoc(d)
        setSelection(null)
        setLoad({ s: 'ready' })
      })
      .catch(() => live && setLoad({ s: 'error' }))
    return () => {
      live = false
    }
  }, [id, t])

  // Catálogo de arquitectura: carrega os grupos que o quadro usa e os rótulos
  // (na língua activa e sempre que ela muda).
  const catalogKeys = doc ? doc.nodes.map((n) => n.props.catalog).filter(Boolean).sort().join(',') : ''
  const needsCatalog = doc?.notation === 'arch' || catalogKeys !== ''
  useEffect(() => {
    if (!needsCatalog) return
    const failed = () => setNotice({ tone: 'danger', text: t('diagrams.catalogo.erro') })
    loadCatalogLabels().catch(failed)
    ensureCatalog(catalogKeys.split(',')).catch(failed)
    const onLang = (lng: string) => {
      const lang = resolveLang(lng)
      if (lang) loadCatalogLabels(lang).catch(failed)
    }
    i18n.on('languageChanged', onLang)
    return () => i18n.off('languageChanged', onLang)
  }, [needsCatalog, catalogKeys, t])

  // Gravação automática, meio segundo depois da última alteração.
  useEffect(() => {
    if (!doc || load.s !== 'ready') return
    const h = setTimeout(() => {
      setPersist('saving')
      putDiagram(doc)
        .then(() => setPersist('saved'))
        .catch(() => setPersist('error'))
    }, 500)
    return () => clearTimeout(h)
  }, [doc, load.s])

  // ---------------------------------------------------------------- histórico
  // O documento corrente numa ref: o histórico não pode viver dentro de um
  // `setDoc(updater)` — o StrictMode corre os updaters duas vezes e cada
  // passo entrava duas vezes no «desfazer».
  const docRef = useRef<DiagramDoc | null>(null)
  docRef.current = doc

  const commit = useCallback((next: DiagramDoc, before?: DiagramDoc, coalesce?: string) => {
    const cur = docRef.current
    if (!cur) return
    const now = Date.now()
    const same = coalesce && lastKey.current?.key === coalesce && now - lastKey.current.at < 1500
    if (!same) past.current = [...past.current, before ?? cur].slice(-HISTORY_MAX)
    lastKey.current = coalesce ? { key: coalesce, at: now } : null
    future.current = []
    const stamped = { ...next, updatedAt: new Date().toISOString() }
    docRef.current = stamped
    setDoc(stamped)
    setHistoryTick((n) => n + 1)
  }, [])

  const live = useCallback((next: DiagramDoc) => {
    docRef.current = next
    setDoc(next)
  }, [])

  const undo = useCallback(() => {
    const cur = docRef.current
    const prev = past.current[past.current.length - 1]
    if (!cur || !prev) return
    past.current = past.current.slice(0, -1)
    future.current = [cur, ...future.current]
    lastKey.current = null
    docRef.current = prev
    setDoc(prev)
    setHistoryTick((n) => n + 1)
  }, [])
  const redo = useCallback(() => {
    const cur = docRef.current
    const next = future.current[0]
    if (!cur || !next) return
    future.current = future.current.slice(1)
    past.current = [...past.current, cur]
    lastKey.current = null
    docRef.current = next
    setDoc(next)
    setHistoryTick((n) => n + 1)
  }, [])

  // ---------------------------------------------------------------- vista
  const canvasSize = () => {
    const r = svgRef.current?.getBoundingClientRect()
    return { w: r?.width ?? 800, h: r?.height ?? 600 }
  }
  const fit = useCallback(() => {
    if (!doc) return
    const { w, h } = canvasSize()
    setView(fitView(contentBox(doc), w, h))
  }, [doc])
  const zoomBy = (f: number) => {
    const { w, h } = canvasSize()
    setView((v) => {
      const k = clampZoom(v.k * f)
      return { k, x: w / 2 - ((w / 2 - v.x) * k) / v.k, y: h / 2 - ((h / 2 - v.y) * k) / v.k }
    })
  }
  useEffect(() => {
    if (load.s !== 'ready' || !doc || fitted.current) return
    fitted.current = true
    if (doc.nodes.length || doc.strokes.length) requestAnimationFrame(() => fit())
  }, [load.s, doc, fit])

  const centreOn = (n: DNode) => {
    const { w, h } = canvasSize()
    const b = nodeBox(n)
    setView((v) => ({ ...v, x: w / 2 - (b.x + b.w / 2) * v.k, y: h / 2 - (b.y + b.h / 2) * v.k }))
  }

  // ---------------------------------------------------------------- derivados
  const notation: Notation = doc?.notation ?? 'uml'
  const issues = useMemo(() => (doc ? validate(doc, notation) : []), [doc, notation])
  const typeLabel = useCallback(
    (n: DNode) => (n.props.catalog && (n.type === 'resource' || n.type === 'resourceGroup') ? t(`diagramCatalog.itens.${n.props.catalog}`) : t(`diagrams.tipos.${n.type}`)),
    [t],
  )
  // Linha secundária das tarefas BPMN: o executor, ou o tipo («service task»).
  // Linha secundária do C4 («[Contentor: Rust]») e do catálogo (tecnologia, ou o tipo quando o nome é outro).
  const subLabel = useCallback(
    (n: DNode) => {
      const tech = n.props.technology?.trim()
      switch (n.type) {
        case 'task':
          return n.props.implementation?.trim() || (n.props.taskKind && n.props.taskKind !== 'none' ? t(`diagrams.opcoes.tarefaCurta.${n.props.taskKind}`) : undefined)
        case 'c4Person':
        case 'c4System':
          return t(`diagrams.c4.tag.${n.type}${n.props.external ? 'Ext' : ''}`)
        case 'c4Container':
        case 'c4Component':
        case 'c4Code':
          return tech ? t(`diagrams.c4.tag.${n.type}Tech`, { tech }) : t(`diagrams.c4.tag.${n.type}`)
        case 'c4Boundary':
          return t(`diagrams.c4.fronteira.${n.props.boundaryKind ?? 'system'}`)
        case 'c4DeploymentNode':
          return tech ? `[${tech}]` : undefined
        case 'resource':
        case 'resourceGroup': {
          if (tech) return tech
          const type = n.props.catalog ? t(`diagramCatalog.itens.${n.props.catalog}`) : ''
          return type && type !== n.name.trim() ? type : undefined
        }
        default:
          return undefined
      }
    },
    [t],
  )

  // ---------------------------------------------------------------- criar
  function defaultName(d: DiagramDoc, type: DNode['type'], key: string): string {
    const base = key.includes('.') ? t(`diagramCatalog.itens.${key}`) : t(`diagrams.novos.${key}`)
    if (!base) return ''
    const tight = CLASSIFIERS.has(type) || type === 'lifeline'
    const taken = new Set(d.nodes.map((n) => n.name))
    if (!taken.has(base)) return base
    for (let i = 2; ; i++) {
      const c = tight ? `${base}${i}` : `${base} ${i}`
      if (!taken.has(c)) return c
    }
  }

  function addNode(item: Extract<PaletteItem, { kind: 'node' }>, at?: Pt) {
    if (!doc) return
    const { w, h } = canvasSize()
    const stagger = (doc.nodes.length % 6) * 18
    const probe = makeNode(item.type, 0, 0, '', item.props)
    const pos = at
      ? { x: at.x - probe.w / 2, y: at.y - (probe.h || 40) / 2 }
      : { x: (w / 2 - view.x) / view.k - probe.w / 2 + stagger, y: (h / 2 - view.y) / view.k - (probe.h || 60) / 2 + stagger }
    const textual = item.type === 'note' || item.type === 'annotation' || item.type === 'flowAnnotation' || item.type === 'sticky'
    const name = textual ? '' : defaultName(doc, item.type, item.key)
    const props = { ...item.props }
    if (textual) props.text = t(`diagrams.novos.${item.key}`)
    if (item.type === 'pool') {
      props.lanes = [
        { id: uid('l'), name: t('diagrams.inspector.pistaN', { n: 1 }), size: 150 },
        { id: uid('l'), name: t('diagrams.inspector.pistaN', { n: 2 }), size: 150 },
      ]
    }
    // Um evento de fronteira com uma actividade seleccionada prende-se à borda de baixo dela.
    const host = selection?.kind === 'node' && !at && props.boundary ? doc.nodes.find((x) => x.id === selection.id && (x.type === 'task' || x.type === 'subProcess')) : undefined
    if (host) {
      const siblings = doc.nodes.filter((x) => x.type === 'intermediateEvent' && x.props.boundary && Math.abs(x.y + x.h / 2 - (host.y + host.h)) < 4).length
      pos.x = host.x + host.w - probe.w / 2 - 22 - siblings * 42
      pos.y = host.y + host.h - probe.h / 2
    }
    const n = makeNode(item.type, Math.round(pos.x / 4) * 4, Math.round(pos.y / 4) * 4, name, props)
    // Contentores entram por baixo de tudo; o resto por cima.
    const nodes = CONTAINERS.has(item.type) ? [n, ...doc.nodes] : [...doc.nodes, n]
    commit({ ...doc, nodes })
    setSelection({ kind: 'node', id: n.id })
    setTab('element')
    setDrawer(null)
  }

  function addLane() {
    if (!doc) return
    const selected = selection?.kind === 'node' ? doc.nodes.find((n) => n.id === selection.id) : undefined
    const pools = doc.nodes.filter((n) => n.type === 'pool')
    const pool = selected?.type === 'pool' ? selected : selected ? laneOf(doc, selected)?.pool : pools.length === 1 ? pools[0] : undefined
    if (!pool) {
      setNotice({ tone: 'warning', text: t(pools.length ? 'diagrams.paleta.pistaEscolher' : 'diagrams.paleta.pistaSemPiscina') })
      return
    }
    const lanes = pool.props.lanes ?? []
    const next = [...lanes, { id: uid('l'), name: t('diagrams.inspector.pistaN', { n: lanes.length + 1 }), size: lanes.length ? 140 : pool.h }]
    commit({ ...doc, nodes: doc.nodes.map((n) => (n.id === pool.id ? { ...n, props: { ...n.props, lanes: next } } : n)) })
    setSelection({ kind: 'node', id: pool.id })
    setTab('lanes')
  }

  function pick(item: PaletteItem) {
    setNotice(null)
    if (item.kind === 'node') addNode(item)
    else if (item.kind === 'lane') addLane()
    else if (item.kind === 'edge') {
      setTool((cur) => (cur.kind === 'edge' && cur.key === item.key ? { kind: 'select' } : { kind: 'edge', edge: item.edge, key: item.key }))
      setDrawer(null)
    } else if (item.kind === 'pen') setTool((cur) => (cur.kind === 'pen' ? { kind: 'select' } : { kind: 'pen' }))
    else if (item.kind === 'eraser') setTool((cur) => (cur.kind === 'eraser' ? { kind: 'select' } : { kind: 'eraser' }))
    else if (item.kind === 'marker') setTool((cur) => (cur.kind === 'marker' ? { kind: 'select' } : { kind: 'marker' }))
    else if (item.kind === 'lasso') setTool((cur) => (cur.kind === 'lasso' ? { kind: 'select' } : { kind: 'lasso' }))
    else if (item.kind === 'shape') setTool((cur) => (cur.kind === 'shape' && cur.shape === item.shape ? { kind: 'select' } : { kind: 'shape', shape: item.shape }))
    if (item.kind !== 'node') setMulti(null)
  }

  function connect(from: DNode, toNode: DNode, offset: number) {
    let to = toNode
    if (!doc) return
    let type = tool.kind === 'edge' ? tool.edge : defaultEdgeType(from, to)
    // Perdida e encontrada só têm uma linha de vida: a de onde se arrasta.
    if (type === 'lostMessage' || type === 'foundMessage') to = from
    if (type === 'sequenceFlow') {
      const pa = laneOf(doc, from)?.pool.id
      const pb = laneOf(doc, to)?.pool.id
      if (pa && pb && pa !== pb) type = 'messageFlow'
    }
    if (!type || !canConnect(type, from, to)) {
      setNotice({
        tone: 'warning',
        text: type
          ? t('diagrams.canvas.ligacaoInvalida', { tipo: t(`diagrams.arestas.${type}`), de: typeLabel(from), para: typeLabel(to) })
          : t('diagrams.canvas.semLigacao'),
      })
      return
    }
    if (from.id === to.id && !['generalization', 'association', 'message', 'lostMessage', 'foundMessage', 'transition'].includes(type)) {
      return
    }
    const e: DEdge = { id: uid('e'), type, from: from.id, to: to.id, label: '' }
    if (type === 'message' || type === 'reply' || type === 'lostMessage' || type === 'foundMessage') e.offset = offset
    commit({ ...doc, edges: [...doc.edges, e] })
    setSelection({ kind: 'edge', id: e.id })
    setNotice(null)
  }

  const removeSelection = useCallback(() => {
    if (doc && multi) {
      const ns = new Set(multi.nodes)
      const ss = new Set(multi.strokes)
      commit({ ...doc, nodes: doc.nodes.filter((n) => !ns.has(n.id)), edges: doc.edges.filter((e) => !ns.has(e.from) && !ns.has(e.to)), strokes: doc.strokes.filter((s) => !ss.has(s.id)) })
      setMulti(null)
      setSelection(null)
      return
    }
    if (!doc || !selection) return
    if (selection.kind === 'node') {
      commit({ ...doc, nodes: doc.nodes.filter((n) => n.id !== selection.id), edges: doc.edges.filter((e) => e.from !== selection.id && e.to !== selection.id) })
    } else if (selection.kind === 'edge') {
      commit({ ...doc, edges: doc.edges.filter((e) => e.id !== selection.id) })
    } else {
      commit({ ...doc, strokes: doc.strokes.filter((s) => s.id !== selection.id) })
    }
    setSelection(null)
  }, [doc, selection, multi, commit])

  const duplicate = useCallback(() => {
    if (!doc || selection?.kind !== 'node') return
    const n = doc.nodes.find((x) => x.id === selection.id)
    if (!n) return
    const copy: DNode = { ...n, id: uid('n'), x: n.x + 24, y: n.y + 24, props: JSON.parse(JSON.stringify(n.props)) }
    if (copy.props.lanes) copy.props.lanes = copy.props.lanes.map((l) => ({ ...l, id: uid('l') }))
    commit({ ...doc, nodes: [...doc.nodes, copy] })
    setSelection({ kind: 'node', id: copy.id })
  }, [doc, selection, commit])

  function setNotation(n: Notation) {
    if (!doc || n === doc.notation) return
    commit({ ...doc, notation: n })
    setTool({ kind: 'select' })
    if (!tabsFor(n).includes(tab)) setTab('element')
  }

  // ---------------------------------------------------------------- validação
  const fixNames = { start: t('diagrams.novos.startEvent'), end: t('diagrams.novos.endEvent') }
  function fixOne(i: Issue) {
    if (!doc) return
    const next = applyFix(doc, i, fixNames)
    if (next !== doc) commit(next)
  }
  function fixEverything() {
    if (!doc) return
    const { doc: next, fixed } = fixAll(doc, fixNames, notation)
    if (fixed > 0) {
      commit(next)
      setNotice({ tone: 'success', text: t('diagrams.validacao.corrigidos', { count: fixed }) })
    } else setNotice({ tone: 'warning', text: t('diagrams.validacao.nadaACorrigir') })
  }
  // BPMN tem o separador «Validação» (como o template); nas outras notações o
  // cartão de validação vive no separador «Elemento».
  function openValidation() {
    setTab(notation === 'bpmn' ? 'validation' : 'element')
    setShowValidation(true)
    setDrawer('inspector')
  }
  function runValidation() {
    openValidation()
    setNotice(
      issues.length === 0
        ? { tone: 'success', text: t('diagrams.validacao.semProblemas') }
        : { tone: 'warning', text: t('diagrams.estado.problemas', { count: issues.length }) },
    )
  }

  // ---------------------------------------------------------------- saídas
  async function exportAs(kind: ExportKind) {
    if (!doc) return
    setMenu(false)
    const base = fileBase(doc.title || t('diagrams.semTitulo'))
    try {
      let file = ''
      if (kind === 'xmi') downloadText((file = `${base}.xmi`), toXmi(doc), 'application/xml')
      if (kind === 'plantuml') downloadText((file = `${base}.puml`), toPlantUml(doc), 'text/plain')
      if (kind === 'bpmn') downloadText((file = `${base}.bpmn`), toBpmn(doc), 'application/xml')
      if (kind === 'c4') downloadText((file = `${base}.c4.puml`), toC4PlantUml(doc, typeLabel), 'text/plain')
      if (kind === 'json') downloadText((file = `${base}.delonix-diagram.json`), toJson(doc), 'application/json')
      if (kind === 'svg' || kind === 'png') {
        // O desenho dos grupos do catálogo tem de estar no ecrã antes de se copiar o SVG.
        await ensureCatalog(doc.nodes.map((n) => n.props.catalog))
        await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)))
        const svg = svgFromCanvas(svgRef.current!, doc)
        if (kind === 'svg') downloadText((file = `${base}.svg`), svg, 'image/svg+xml')
        else downloadBlob((file = `${base}.png`), (await pngFromSvg(svg)).blob)
      }
      setNotice({ tone: 'success', text: t('diagrams.exportar.feito', { ficheiro: file }) })
    } catch {
      setNotice({ tone: 'danger', text: t('diagrams.exportar.erro') })
    }
  }

  async function importFile(f: File) {
    const parsed = parseJson(await f.text())
    if (!parsed) {
      setNotice({ tone: 'danger', text: t('diagrams.importar.erro') })
      return
    }
    const fresh: DiagramDoc = { ...parsed, id: uid('d'), boardId: undefined, savedAt: undefined }
    try {
      await putDiagram(fresh)
      location.hash = `/whiteboards/diagram/${fresh.id}`
    } catch {
      setNotice({ tone: 'danger', text: t('diagrams.local.erro') })
    }
  }

  function onSaved(meta: WhiteboardMeta, title: string, roomCode: string) {
    setSaving(false)
    setDoc((cur) => (cur ? { ...cur, title: title || cur.title, roomCode, boardId: meta.id, savedAt: meta.created_at } : cur))
    setNotice({ tone: 'success', text: t('diagrams.guardar.feito'), link: true })
  }

  // ---------------------------------------------------------------- teclado
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (isTyping(e.target) || saving) return
      const mod = e.ctrlKey || e.metaKey
      if (mod && e.key.toLowerCase() === 'z') {
        e.preventDefault()
        if (e.shiftKey) redo()
        else undo()
      } else if (mod && e.key.toLowerCase() === 'y') {
        e.preventDefault()
        redo()
      } else if (mod && e.key.toLowerCase() === 'd') {
        e.preventDefault()
        duplicate()
      } else if (e.key === 'Delete' || e.key === 'Backspace') {
        if (selection || multi) {
          e.preventDefault()
          removeSelection()
        }
      } else if (e.key === 'Escape') {
        if (menu) setMenu(false)
        else if (drawer) setDrawer(null)
        else if (multi) setMulti(null)
        else if (tool.kind !== 'select') setTool({ kind: 'select' })
        else setSelection(null)
      } else if (!mod && (e.key === '+' || e.key === '=')) zoomBy(1.2)
      else if (!mod && e.key === '-') zoomBy(1 / 1.2)
      else if (!mod && e.key === '0') fit()
      else if (e.key.startsWith('Arrow') && selection?.kind === 'node' && doc) {
        e.preventDefault()
        const step = e.shiftKey ? 16 : 4
        const dx = e.key === 'ArrowLeft' ? -step : e.key === 'ArrowRight' ? step : 0
        const dy = e.key === 'ArrowUp' ? -step : e.key === 'ArrowDown' ? step : 0
        commit({ ...doc, nodes: doc.nodes.map((n) => (n.id === selection.id ? { ...n, x: n.x + dx, y: n.y + dy } : n)) }, undefined, `nudge:${selection.id}`)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  })

  // ---------------------------------------------------------------- ecrã
  if (load.s === 'loading' || (load.s === 'ready' && !doc)) {
    return (
      <div className="dg-wait" role="status">
        <Spinner label={t('diagrams.aCarregar')} />
      </div>
    )
  }
  if (load.s === 'missing' || load.s === 'error') {
    return (
      <div className="page">
        <Alert tone={load.s === 'error' ? 'danger' : 'warning'} icon="alert">
          {t(load.s === 'error' ? 'diagrams.local.erro' : 'diagrams.local.naoEncontrado')}
        </Alert>
        <div className="dg-actions">
          <Button variant="primary" icon="plus" onClick={() => (location.hash = '/whiteboards/diagram')}>
            {t('diagrams.novo')}
          </Button>
          <Button variant="ghost" onClick={() => (location.hash = '/whiteboards')}>
            {t('diagrams.barra.voltar')}
          </Button>
        </div>
      </div>
    )
  }
  const d = doc!
  const primaryExport: ExportKind = notation === 'uml' ? 'xmi' : notation === 'bpmn' ? 'bpmn' : 'svg'
  const formats: ExportKind[] =
    notation === 'uml'
      ? ['xmi', 'plantuml', 'svg', 'png', 'json']
      : notation === 'bpmn'
        ? ['bpmn', 'svg', 'png', 'json']
        : notation === 'arch'
          ? ['svg', 'c4', 'png', 'json']
          : ['svg', 'png', 'json']
  const empty = d.nodes.length === 0 && d.strokes.length === 0
  const first = issues[0]

  const outputs =
    notation === 'uml' || notation === 'bpmn' ? (
      <section className="dg-card dg-outputs" aria-label={t('diagrams.saidas.titulo')}>
        <h3 className="dg-card__title">{t('diagrams.saidas.titulo')}</h3>
        <ul className="dg-outputs__list">
          <li>
            <Icon name="check" size={11} />
            <span>
              {formats
                .filter((f) => f !== 'json')
                .map((f, i) => (
                  <span key={f}>
                    {i > 0 && ' · '}
                    <button type="button" className="dg-out" onClick={() => void exportAs(f)}>
                      {t(`diagrams.exportar.curto.${f}`)}
                    </button>
                  </span>
                ))}
            </span>
          </li>
          <li>
            <Icon name={d.roomCode ? 'check' : 'link'} size={11} />
            <button type="button" className="dg-out" onClick={() => setSaving(true)}>
              {d.roomCode ? t('diagrams.saidas.anexadaA', { sala: d.roomCode }) : t('diagrams.saidas.anexar')}
            </button>
          </li>
        </ul>
      </section>
    ) : null

  return (
    <div className="dg dx-stage">
      <header className="dg-bar">
        <button type="button" className="dx-iconbtn page-bar__burger" aria-label={t('shell.abrirNavegacao')} onClick={() => setNavOpen(true)}>
          <Icon name="menu" />
        </button>
        <span className="dg-bar__mark" aria-hidden="true">
          <DelonixSymbol size={24} />
        </span>
        <span className="dg-title-wrap">
          <span className="dg-title-prefix">{t('diagrams.barra.prefixo')}</span>
          <input
            className="dg-title"
            value={d.title}
            size={Math.max(8, Math.min(48, d.title.length + 1))}
            aria-label={t('diagrams.barra.titulo')}
            placeholder={t('diagrams.semTitulo')}
            maxLength={120}
            onChange={(e) => commit({ ...d, title: e.target.value }, undefined, 'title')}
          />
          <span className={cx('dg-persist dx-num', persist === 'error' && 'is-warn')} aria-live="polite">
            {persist === 'error' ? t('diagrams.estado.localErro') : persist === 'idle' ? '' : t('diagrams.estado.localGuardado')}
          </span>
        </span>
        <div className="dg-notations" role="tablist" aria-label={t('diagrams.notacoes.rotulo')}>
          {NOTATIONS.map((n) => (
            <button key={n} type="button" role="tab" aria-selected={notation === n} onClick={() => setNotation(n)}>
              {t(`diagrams.notacoes.${n}`)}
            </button>
          ))}
        </div>
        <div className="dx-spacer" />
        <div className="dg-bar__actions">
          <IconButton icon="undo" bare label={t('diagrams.barra.desfazer')} disabled={past.current.length === 0} onClick={undo} />
          <IconButton icon="undo" bare className="dg-redo" label={t('diagrams.barra.refazer')} disabled={future.current.length === 0} onClick={redo} />
          {notation !== 'free' && (
            <Button size="sm" className="dg-bar__btn dg-bar__validate" onClick={runValidation}>
              {t(notation === 'bpmn' ? 'diagrams.barra.validarBpmn' : 'diagrams.barra.validarUml')}
            </Button>
          )}
          <div className="dg-export">
            <Button size="sm" className="dg-bar__btn dg-export__main" onClick={() => void exportAs(primaryExport)}>
              {t(`diagrams.exportar.botao.${primaryExport}`)}
            </Button>
            <IconButton icon="more" label={t('diagrams.barra.mais')} aria-haspopup="menu" aria-expanded={menu} onClick={() => setMenu((m) => !m)} />
            {menu && (
              <div className="dg-menu" role="menu">
                {formats.map((f) => (
                  <button key={f} type="button" role="menuitem" onClick={() => void exportAs(f)}>
                    {t(`diagrams.exportar.${f}`)}
                  </button>
                ))}
                <button
                  type="button"
                  role="menuitem"
                  onClick={() => {
                    setMenu(false)
                    fileRef.current?.click()
                  }}
                >
                  {t('diagrams.barra.importar')}
                </button>
              </div>
            )}
          </div>
          <Button size="sm" variant="primary" className="dg-bar__save" onClick={() => setSaving(true)}>
            {t('diagrams.barra.guardar')}
          </Button>
        </div>
        <input
          ref={fileRef}
          type="file"
          accept="application/json,.json"
          hidden
          onChange={(e) => {
            const f = e.target.files?.[0]
            e.target.value = ''
            if (f) void importFile(f)
          }}
        />
      </header>

      <div className="dg-drawers" role="group" aria-label={t('diagrams.barra.paineis')}>
        <Button size="sm" variant={drawer === 'palette' ? 'primary' : 'secondary'} icon="grid" aria-expanded={drawer === 'palette'} onClick={() => setDrawer(drawer === 'palette' ? null : 'palette')}>
          {t('diagrams.barra.paleta')}
        </Button>
        <Button size="sm" variant={drawer === 'inspector' ? 'primary' : 'secondary'} icon="sliders" aria-expanded={drawer === 'inspector'} onClick={() => setDrawer(drawer === 'inspector' ? null : 'inspector')}>
          {t('diagrams.barra.propriedades')}
          {issues.length > 0 && <span className="dg-seg__count dx-num">{issues.length}</span>}
        </Button>
      </div>

      <div className="dg-body">
        <aside className={cx('dg-side dg-side--left', drawer === 'palette' && 'is-open')} aria-label={t('diagrams.barra.paleta')}>
          <Palette
            notation={notation}
            doc={d}
            tool={tool}
            penColor={penColor}
            penWidth={penWidth}
            penOpacity={penOpacity}
            typeLabel={typeLabel}
            onPenColor={setPenColor}
            onPenWidth={setPenWidth}
            onPenOpacity={setPenOpacity}
            onPick={pick}
            onFind={(n) => {
              setSelection({ kind: 'node', id: n.id })
              setTab('element')
              centreOn(n)
              setDrawer(null)
            }}
          />
        </aside>

        <main className="dg-stagewrap">
          <Canvas
            doc={d}
            selection={selection}
            tool={tool}
            view={view}
            svgRef={svgRef}
            penColor={penColor}
            penWidth={penWidth}
            penOpacity={penOpacity}
            multi={multi}
            onMulti={(m) => {
              setMulti(m)
              if (m) setTab('element')
            }}
            typeLabel={typeLabel}
            subLabel={subLabel}
            onView={setView}
            onSelect={(s) => {
              setSelection(s)
              if (s && tab !== 'element' && tab !== 'style' && tab !== 'lanes') setTab('element')
            }}
            onLive={live}
            onCommit={(next, before) => commit(next, before)}
            onConnect={connect}
            onDropPalette={(key, at) => {
              const it = paletteItemByKey(key)
              if (it?.kind === 'node') addNode(it, at)
            }}
          />

          {empty && (
            <div className="dg-hint" aria-live="polite">
              <p>{t(notation === 'free' ? 'diagrams.canvas.vazioLivre' : 'diagrams.canvas.vazio')}</p>
              {examplesFor(notation).map((variant, _i, all) => (
                <Button
                  key={variant}
                  size="sm"
                  variant="outline"
                  icon="sparkles"
                  data-example={variant}
                  onClick={() => {
                    const sample = example(variant, (k) => t(`diagrams.exemplos.${k}`))
                    if (!sample) return
                    commit({ ...d, ...sample, title: d.title === t('diagrams.semTitulo') ? t(`diagrams.exemplos.${variant}.nome`) : d.title })
                    fitted.current = false
                  }}
                >
                  {all.length > 1 ? t('diagrams.canvas.exemploDe', { nome: t(`diagrams.exemplos.${variant}.nome`) }) : t('diagrams.canvas.exemplo')}
                </Button>
              ))}
            </div>
          )}

          {notice && (
            <div className={cx('dg-notice', `is-${notice.tone}`)} role="status">
              <span>{notice.text}</span>
              {notice.link && (
                <a className="dg-link" href="#/whiteboards">
                  {t('diagrams.guardar.ver')}
                </a>
              )}
              <IconButton icon="x" bare label={t('ui.dispensar')} onClick={() => setNotice(null)} />
            </div>
          )}

          <div className="dg-status">
            <span className="dg-chip dx-num">{summary(t, d, notation)}</span>
            {notation !== 'free' && (
              <button
                type="button"
                className={cx('dg-chip', issues.length === 0 ? 'is-ok' : 'is-warn')}
                onClick={openValidation}
              >
                <Icon name={issues.length === 0 ? 'check' : 'alert'} size={12} />
                {issues.length === 0 ? t('diagrams.estado.valido') : t('diagrams.estado.problemaPrimeiro', { count: issues.length, primeiro: t(`diagrams.regras.${first.code}`, issueParams(d, first, typeLabel)) })}
              </button>
            )}
          </div>

          <div className="dg-zoom" role="group" aria-label={t('diagrams.zoom.rotulo')}>
            <IconButton icon="minus" bare label={t('diagrams.zoom.menos')} onClick={() => zoomBy(1 / 1.2)} />
            <button type="button" className="dg-zoom__fit dx-num" onClick={fit}>
              {t('diagrams.zoom.ajustar', { pct: Math.round(view.k * 100) })}
            </button>
            <IconButton icon="plus" bare label={t('diagrams.zoom.mais')} onClick={() => zoomBy(1.2)} />
          </div>
        </main>

        <aside className={cx('dg-side dg-side--right', drawer === 'inspector' && 'is-open')} aria-label={t('diagrams.inspector.rotulo')}>
          <Inspector
            doc={d}
            notation={notation}
            selection={selection}
            tab={tab}
            issues={issues}
            typeLabel={typeLabel}
            showValidation={showValidation}
            onTab={setTab}
            onChange={(next, key) => commit(next, undefined, key)}
            onSelect={(s) => {
              setSelection(s)
              const n = s?.kind === 'node' ? d.nodes.find((x) => x.id === s.id) : undefined
              if (n) centreOn(n)
            }}
            multi={multi}
            onDelete={removeSelection}
            onDuplicate={duplicate}
            onFix={fixOne}
            onFixAll={fixEverything}
            footer={outputs}
          />
        </aside>
        {drawer && <div className="dg-scrim" onClick={() => setDrawer(null)} aria-hidden="true" />}
      </div>

      {saving && (
        <SaveDialog
          title={d.title}
          roomCode={d.roomCode}
          empty={empty}
          makePng={async () => {
            await ensureCatalog(d.nodes.map((n) => n.props.catalog))
            return (await pngFromSvg(svgFromCanvas(svgRef.current!, d))).base64
          }}
          onClose={() => setSaving(false)}
          onSaved={onSaved}
        />
      )}
    </div>
  )
}

/** Linha de estado: o que há no quadro, contado para a notação activa. */
function summary(t: (k: string, o?: Record<string, unknown>) => string, d: DiagramDoc, notation: Notation): string {
  const of = (types: DNode['type'][]) => d.nodes.filter((n) => types.includes(n.type)).length
  const edges = (n: Notation) => d.edges.filter((e) => EDGE_NOTATION[e.type] === n).length
  const parts: string[] = [t(`diagrams.estado.norma.${notation}`)]
  const u = (key: string, count: number) => parts.push(t(`diagrams.estado.unidades.${key}`, { count }))
  if (notation === 'uml') {
    u('classes', of(['class', 'interface', 'enum']))
    u('linhasVida', of(['lifeline']))
    u('casos', of(['usecase']))
  } else if (notation === 'bpmn') {
    const pools = d.nodes.filter((n) => n.type === 'pool')
    u('piscinas', pools.length)
    u('pistas', pools.reduce((s, p) => s + (p.props.lanes?.length ?? 0), 0))
    u('elementos', d.nodes.filter((n) => NODE_NOTATION[n.type] === 'bpmn' && n.type !== 'pool').length)
  } else if (notation === 'arch') {
    u('componentes', d.nodes.filter((n) => NODE_NOTATION[n.type] === 'arch' && !CONTAINERS.has(n.type)).length)
    u('ligacoes', edges('arch'))
  } else if (notation === 'flow') {
    u('formas', d.nodes.filter((n) => FLOW_NODES.has(n.type)).length)
    u('ligacoes', edges('flow'))
  } else {
    u('tracos', d.strokes.length)
    u('textos', of(['text', 'note', 'sticky']))
  }
  return parts.join(' · ')
}

