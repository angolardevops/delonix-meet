/**
 * Coluna esquerda: procurar elemento e as paletas da notação activa.
 *
 * Cada item é um botão: carregar põe o elemento no centro da vista (é o
 * caminho do teclado e do toque); arrastar para o quadro põe-no onde cair.
 * Os itens de ligação escolhem a ferramenta de aresta.
 */
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Icon } from '../../ui/icons'
import { cx, TextInput } from '../../ui/kit'
import type { Tool } from './Canvas'
import { PALETTE_MIME } from './Canvas'
import { DiagramDoc, DNode, Notation, PaletteGroup, PALETTES, PaletteItem } from './model'
import { PENS } from './paint'

/** Glifos da paleta, desenhados como no template (viewBox 22×18). */
const GLYPH: Record<string, string> = {
  class: 'M2 2h18v14H2zM2 7h18M2 11h18',
  interface: 'M2 3h18v12H2zM2 8h18',
  enum: 'M3 2h16v14H3zM3 6h16M7 10h8',
  package: 'M2 5h18v11H2zM2 5V2h8v3',
  note: 'M2 2h14l4 4v10H2z',
  generalization: 'M11 16V7M11 2 6 8h10z',
  association: 'M2 9h18',
  composition: 'M2 9l3-3 3 3-3 3zM8 9h12',
  lifeline: 'M4 2h14v5H4zM11 7v9',
  message: 'M2 9h16M14 5l4 4-4 4',
  reply: 'M20 9H4M8 5 4 9l4 4',
  fragment: 'M2 2h18v14H2zM2 6h7l2 2',
  actor: 'M11 5a2.4 2.4 0 1 0 0-4 2.4 2.4 0 0 0 0 4ZM11 5v7M6 8h10M11 12l-3 5M11 12l3 5',
  usecase: 'M11 3c5 0 9 2.7 9 6s-4 6-9 6-9-2.7-9-6 4-6 9-6Z',
  include: 'M2 9h16M14 6l4 3-4 3',
  boundary: 'M2 2h18v14H2z',
  startEvent: 'M11 2a7 7 0 1 0 0 14 7 7 0 0 0 0-14Z',
  messageEvent: 'M11 2a7 7 0 1 0 0 14 7 7 0 0 0 0-14ZM7.6 7h6.8v4H7.6zM7.6 7 11 9.6 14.4 7',
  timerEvent: 'M11 2a7 7 0 1 0 0 14 7 7 0 0 0 0-14ZM11 6v3.2l2.2 1.4',
  endEvent: 'M11 1.4a7.6 7.6 0 1 0 0 15.2 7.6 7.6 0 0 0 0-15.2ZM11 4a5 5 0 1 0 0 10 5 5 0 0 0 0-10Z',
  task: 'M2 3h18v12H2z',
  userTask: 'M2 3h18v12H2zM6 7.4a1.4 1.4 0 1 0 0-2.8 1.4 1.4 0 0 0 0 2.8',
  serviceTask: 'M2 3h18v12H2zM6 9a1.6 1.6 0 1 0 0-3.2A1.6 1.6 0 0 0 6 9',
  subProcess: 'M2 3h18v12H2zM8 12h6M11 9v6',
  exclusiveGateway: 'M11 2 20 9l-9 7-9-7zM8 6.4l6 5.2M14 6.4l-6 5.2',
  parallelGateway: 'M11 2 20 9l-9 7-9-7zM11 5.4v7.2M7.4 9h7.2',
  inclusiveGateway: 'M11 2 20 9l-9 7-9-7zM11 5.6a3.4 3.4 0 1 0 0 6.8 3.4 3.4 0 0 0 0-6.8',
  eventGateway: 'M11 2 20 9l-9 7-9-7zM11 5.4 13 9l-2 3.6L9 9z',
  pool: 'M2 3h18v12H2zM5 3v12',
  lane: 'M2 3h18v6H2zM2 9h18v6H2z',
  dataObject: 'M4 2h9l5 4v10H4z',
  annotation: 'M6 2v14M6 2h12M6 16h12',
  service: 'M2 3h18v12H2zM6 7h10M6 11h6',
  database: 'M11 2c4 0 8 1 8 2.5v9c0 1.5-4 2.5-8 2.5s-8-1-8-2.5v-9C3 3 7 2 11 2ZM3 4.5c0 1.5 4 2.5 8 2.5s8-1 8-2.5',
  queue: 'M2 4h18v10H2zM13 4v10M16 4v10',
  client: 'M3 3h16v10H3zM8 16h6M11 13v3',
  external: 'M2 3h18v12H2z',
  zone: 'M2 2h18v14H2z',
  sync: 'M2 9h16M14 5l4 4-4 4',
  async: 'M2 9h3M8 9h3M14 9h4M14 5l4 4-4 4',
  dataFlow: 'M2 9h1M5 9h1M8 9h1M11 9h1M14 9h4M14 5l4 4-4 4',
  terminator: 'M6 4h10a5 5 0 0 1 0 10H6A5 5 0 0 1 6 4Z',
  process: 'M2 4h18v10H2z',
  decision: 'M11 2 20 9l-9 7-9-7z',
  io: 'M6 4h14l-4 10H2z',
  document: 'M3 2h16v11c-4-2-8 3-16 1z',
  flow: 'M2 9h16M14 5l4 4-4 4',
  pen: 'M4 16l3-1 9-9-2-2-9 9zM12 6l2 2',
  eraser: 'M8 16h10M4 12l7-7 5 5-5 5H7z',
  text: 'M4 4h14M11 4v12M8 16h6',
  // UML — mais ligações
  aggregation: 'M2 9l3-3 3 3-3 3zM8 9h12',
  realization: 'M11 16V7M11 2 6 8h10z',
  dependency: 'M2 9h16M14 5l4 4-4 4',
  activation: 'M11 1v3M8 4h6v10H8zM11 14v3',
  selfMessage: 'M5 3v12M5 5h10v6H7M9 9l-2 2 2 2',
  lostMessage: 'M2 9h13M17 9a2 2 0 1 0 0 .1',
  foundMessage: 'M5 9a2 2 0 1 0 0 .1M7 9h13M16 5l4 4-4 4',
  extend: 'M2 9h16M14 6l4 3-4 3',
  ucGeneralization: 'M2 9h13M20 9l-6-4v8z',
  // UML — actividade
  initialNode: 'M11 4a5 5 0 1 0 0 10 5 5 0 0 0 0-10Z',
  activityFinal: 'M11 2a7 7 0 1 0 0 14 7 7 0 0 0 0-14ZM11 6a3 3 0 1 0 0 6 3 3 0 0 0 0-6Z',
  flowFinal: 'M11 2a7 7 0 1 0 0 14 7 7 0 0 0 0-14ZM6.5 4.5l9 9M15.5 4.5l-9 9',
  action: 'M6 3h10a4 4 0 0 1 4 4v4a4 4 0 0 1-4 4H6a4 4 0 0 1-4-4V7a4 4 0 0 1 4-4Z',
  decisionNode: 'M11 3 17 9l-6 6-6-6z',
  forkNode: 'M2 8h18v2H2zM6 3v5M16 3v5M11 10v5',
  partition: 'M3 2h16v14H3zM3 6h16M11 2v14',
  objectNode: 'M3 4h16v10H3z',
  controlFlow: 'M2 9h16M14 5l4 4-4 4',
  // UML — estados
  state: 'M6 3h10a4 4 0 0 1 4 4v4a4 4 0 0 1-4 4H6a4 4 0 0 1-4-4V7a4 4 0 0 1 4-4ZM2 8h18',
  compositeState: 'M5 2h12a3 3 0 0 1 3 3v8a3 3 0 0 1-3 3H5a3 3 0 0 1-3-3V5a3 3 0 0 1 3-3ZM2 6h18M6 9h5v4H6z',
  stateInitial: 'M11 4a5 5 0 1 0 0 10 5 5 0 0 0 0-10Z',
  stateFinal: 'M11 2a7 7 0 1 0 0 14 7 7 0 0 0 0-14ZM11 6a3 3 0 1 0 0 6 3 3 0 0 0 0-6Z',
  choice: 'M11 3 17 9l-6 6-6-6z',
  history: 'M11 2a7 7 0 1 0 0 14 7 7 0 0 0 0-14ZM8.5 5.5v7M13.5 5.5v7M8.5 9h5',
  transition: 'M2 12c4-8 12-8 16-2M15 6l3 4-4 1',
  // UML — componentes e implantação
  component: 'M4 3h15v12H4zM2 6h5v2.5H2zM2 10h5v2.5H2z',
  port: 'M8 6h6v6H8zM2 9h6M14 9h6',
  providedInterface: 'M2 9h9M15 5a4 4 0 1 0 0 8 4 4 0 0 0 0-8Z',
  requiredInterface: 'M2 9h9M17 4a5 5 0 0 0 0 10',
  deviceNode: 'M2 6l4-4h14v11l-4 4M2 6h14v11H2zM16 6l4-4',
  artifact: 'M4 2h10l4 4v10H4zM14 2v4h4',
  usage: 'M2 9h16M14 5l4 4-4 4',
  deploy: 'M2 9h16M14 5l4 4-4 4',
  manifest: 'M2 9h16M14 5l4 4-4 4',
  // UML — objectos
  object: 'M2 3h18v12H2zM2 8h18M6 6h10',
  link: 'M2 9h18',
  // BPMN — quadros-formas
  catchMessage: 'M11 2a7 7 0 1 0 0 14 7 7 0 0 0 0-14ZM11 4.2a4.8 4.8 0 1 0 0 9.6 4.8 4.8 0 0 0 0-9.6ZM8.4 7.4h5.2v3.2H8.4zM8.4 7.4 11 9.2l2.6-1.8',
  throwMessage: 'M11 2a7 7 0 1 0 0 14 7 7 0 0 0 0-14ZM11 4.2a4.8 4.8 0 1 0 0 9.6 4.8 4.8 0 0 0 0-9.6ZM8.4 7.4h5.2v3.2H8.4zM8.4 7.4 11 9.2l2.6-1.8M9 8.6h4M9 9.6h4',
  catchSignal: 'M11 2a7 7 0 1 0 0 14 7 7 0 0 0 0-14ZM11 4.2a4.8 4.8 0 1 0 0 9.6 4.8 4.8 0 0 0 0-9.6ZM11 6.4l2.6 4.4H8.4z',
  throwSignal: 'M11 2a7 7 0 1 0 0 14 7 7 0 0 0 0-14ZM11 4.2a4.8 4.8 0 1 0 0 9.6 4.8 4.8 0 0 0 0-9.6ZM11 6.4l2.6 4.4H8.4zM11 8v2',
  conditionalEvent: 'M11 2a7 7 0 1 0 0 14 7 7 0 0 0 0-14ZM11 4.2a4.8 4.8 0 1 0 0 9.6 4.8 4.8 0 0 0 0-9.6ZM9 6.4h4v5.2H9zM9.6 8h2.8M9.6 9.8h2.8',
  throwEscalation: 'M11 2a7 7 0 1 0 0 14 7 7 0 0 0 0-14ZM11 4.2a4.8 4.8 0 1 0 0 9.6 4.8 4.8 0 0 0 0-9.6ZM11 6.2l2 5-2-1.6-2 1.6z',
  throwCompensation: 'M11 2a7 7 0 1 0 0 14 7 7 0 0 0 0-14ZM11 4.2a4.8 4.8 0 1 0 0 9.6 4.8 4.8 0 0 0 0-9.6ZM11 7v4l-2.6-2zM13.6 7v4L11 9z',
  catchLink: 'M11 2a7 7 0 1 0 0 14 7 7 0 0 0 0-14ZM11 4.2a4.8 4.8 0 1 0 0 9.6 4.8 4.8 0 0 0 0-9.6ZM8.4 8h2.6V6.6L13.6 9 11 11.4V10H8.4z',
  throwLink: 'M11 2a7 7 0 1 0 0 14 7 7 0 0 0 0-14ZM11 4.2a4.8 4.8 0 1 0 0 9.6 4.8 4.8 0 0 0 0-9.6ZM8.4 8h2.6V6.6L13.6 9 11 11.4V10H8.4zM9 9h3',
  boundaryTimer: 'M2 12h18M11 5a4 4 0 1 0 0 8 4 4 0 0 0 0-8ZM11 7v2l1.4 1',
  boundaryError: 'M2 12h18M11 5a4 4 0 1 0 0 8 4 4 0 0 0 0-8ZM9.6 10.6l1-3 .8 2 1-1.6-.8 3-.8-2z',
  boundaryMessage: 'M2 12h18M11 5a4 4 0 1 0 0 8 4 4 0 0 0 0-8ZM9.4 8h3.2v2H9.4z',
  messageEnd: 'M11 1.4a7.6 7.6 0 1 0 0 15.2 7.6 7.6 0 0 0 0-15.2ZM8.4 7.4h5.2v3.2H8.4z',
  signalEnd: 'M11 1.4a7.6 7.6 0 1 0 0 15.2 7.6 7.6 0 0 0 0-15.2ZM11 6.2l2.8 4.6H8.2z',
  errorEnd: 'M11 1.4a7.6 7.6 0 1 0 0 15.2 7.6 7.6 0 0 0 0-15.2ZM9 11.6l1.3-4.4 1.2 2.6 1.3-3-1.1 4.4-1.3-2.4z',
  escalationEnd: 'M11 1.4a7.6 7.6 0 1 0 0 15.2 7.6 7.6 0 0 0 0-15.2ZM11 6l2 5.2-2-1.6-2 1.6z',
  compensationEnd: 'M11 1.4a7.6 7.6 0 1 0 0 15.2 7.6 7.6 0 0 0 0-15.2ZM11 7v4l-2.6-2zM13.6 7v4L11 9z',
  terminateEnd: 'M11 1.4a7.6 7.6 0 1 0 0 15.2 7.6 7.6 0 0 0 0-15.2ZM11 6a3 3 0 1 0 0 6 3 3 0 0 0 0-6Z',
  timerStart: 'M11 2a7 7 0 1 0 0 14 7 7 0 0 0 0-14ZM11 5.6v3.4l2.2 1.4',
  signalStart: 'M11 2a7 7 0 1 0 0 14 7 7 0 0 0 0-14ZM11 6l3 5H8z',
  conditionalStart: 'M11 2a7 7 0 1 0 0 14 7 7 0 0 0 0-14ZM9 6h4v6H9zM9.6 8h2.8M9.6 10h2.8',
  sendTask: 'M2 3h18v12H2zM5 5.6h6v4H5zM5 5.6l3 2 3-2',
  receiveTask: 'M2 3h18v12H2zM5 5.6h6v4H5zM5 5.6l3 2 3-2',
  manualTask: 'M2 3h18v12H2zM5 10V8l2-2h2l-.6 1.2H12',
  scriptTask: 'M2 3h18v12H2zM5 5h4c-1 1-1 2 0 3s1 2 0 3H5c1-1 1-2 0-3s-1-2 0-3Z',
  businessRuleTask: 'M2 3h18v12H2zM5 5h7v5H5zM5 7h7M7.4 7v3',
  callActivity: 'M2 3h18v12H2zM3.5 4.5h15v9h-15z',
  loopTask: 'M2 3h18v12H2zM9 12.6a2.4 2.4 0 1 1 3.8 0M9 12.6H7.6',
  multiParallel: 'M2 3h18v12H2zM9 10v4M11 10v4M13 10v4',
  multiSequential: 'M2 3h18v12H2zM8.6 10.4h4.8M8.6 12h4.8M8.6 13.6h4.8',
  compensationTask: 'M2 3h18v12H2zM11 10.4v3.2l-2.4-1.6zM13.4 10.4v3.2L11 12z',
  adHocSubProcess: 'M2 3h18v12H2zM8 12c1-1.6 2-1.6 3 0s2 1.6 3 0',
  complexGateway: 'M11 2 20 9l-9 7-9-7zM11 5.4v7.2M7.4 9h7.2M8.6 6.6l4.8 4.8M13.4 6.6l-4.8 4.8',
  eventInstantiateGateway: 'M11 2 20 9l-9 7-9-7zM11 5.6a3.4 3.4 0 1 0 0 6.8 3.4 3.4 0 0 0 0-6.8M11 7l1.3 1-.5 1.6h-1.6L9.7 8z',
  eventParallelGateway: 'M11 2 20 9l-9 7-9-7zM11 5.6a3.4 3.4 0 1 0 0 6.8 3.4 3.4 0 0 0 0-6.8M11 7v4M9 9h4',
  dataStore: 'M11 2c4 0 7 1 7 2.4v9.2c0 1.4-3 2.4-7 2.4s-7-1-7-2.4V4.4C4 3 7 2 11 2ZM4 4.4c0 1.4 3 2.4 7 2.4s7-1 7-2.4M4 7c0 1.4 3 2.4 7 2.4s7-1 7-2.4',
  dataInput: 'M4 2h9l5 4v10H4zM6 5h3V3.4L12 6 9 8.6V7H6z',
  dataOutput: 'M4 2h9l5 4v10H4zM6 5h3V3.4L12 6 9 8.6V7H6zM7 6h3',
  dataCollection: 'M4 2h9l5 4v10H4zM9 11v4M11 11v4M13 11v4',
  group: 'M5 2h12a3 3 0 0 1 3 3v8a3 3 0 0 1-3 3H5a3 3 0 0 1-3-3V5a3 3 0 0 1 3-3Z',
  sequenceFlow: 'M2 9h16M14 5l4 4-4 4',
  messageFlow: 'M4 9a2 2 0 1 0 0 .1M6 9h2M10 9h2M14 9h3M17 6l3 3-3 3',
  dataAssociation: 'M2 9h2M6 9h2M10 9h2M14 9h2M18 9h2',
  // Fluxograma — quadros-formas
  predefinedProcess: 'M2 4h18v10H2zM5 4v10M17 4v10',
  preparation: 'M2 9l4-5h10l4 5-4 5H6z',
  manualInput: 'M2 7l18-3v10H2z',
  manualOperation: 'M2 4h18l-4 10H6z',
  delay: 'M2 4h11a5 5 0 0 1 0 10H2z',
  merge: 'M4 4h14l-7 10z',
  loopLimit: 'M5 4h12l3 3v7H2V7z',
  display: 'M2 9l4-5h11a4 5 0 0 1 0 10H6z',
  multiDocument: 'M7 1h13v9M5 3h13v9M3 5h13v8c-3-1.6-6 2.4-13 .8z',
  flowDatabase: 'M11 2c4 0 7 1 7 2.4v9.2c0 1.4-3 2.4-7 2.4s-7-1-7-2.4V4.4C4 3 7 2 11 2ZM4 4.4c0 1.4 3 2.4 7 2.4s7-1 7-2.4',
  storedData: 'M6 4h14a3 5 0 0 0 0 10H6a3 5 0 0 1 0-10Z',
  internalStorage: 'M3 3h16v12H3zM6 3v12M3 6h16',
  sequentialStorage: 'M11 16a7 7 0 1 1 7-6.6V16z',
  directAccessStorage: 'M5 4h12a3 5 0 0 1 0 10H5a3 5 0 0 1 0-10ZM17 4a3 5 0 0 0 0 10',
  connector: 'M11 3a6 6 0 1 0 0 12 6 6 0 0 0 0-12Z',
  offPageConnector: 'M5 2h12v9l-6 5-6-5z',
  flowAnnotation: 'M6 2v14M6 2h12M6 16h12',
  flowNote: 'M2 9h2M6 9h2M10 9h2M14 9h2M18 9h2',
}

const DASHED = new Set(['flowNote', 'group', 'messageFlow', 'dataAssociation', 'boundary', 'external', 'zone', 'partition', 'reply', 'realization', 'dependency', 'usage', 'deploy', 'manifest', 'extend'])

const CLOSED_KEY = 'dx-diagram-palette-closed'

function readClosed(): Record<string, boolean> {
  try {
    const raw = localStorage.getItem(CLOSED_KEY)
    return raw ? (JSON.parse(raw) as Record<string, boolean>) : {}
  } catch {
    return {}
  }
}

function writeClosed(v: Record<string, boolean>) {
  try {
    localStorage.setItem(CLOSED_KEY, JSON.stringify(v))
  } catch {
    /* sem armazenamento: o estado dura só esta visita */
  }
}

/** Texto em que «Procurar elemento» procura dentro de um elemento do quadro. */
export function searchableText(n: DNode, typeLabel: string): string[] {
  const p = n.props
  return [n.name, typeLabel, p.text ?? '', p.stereotype ?? '', p.instanceOf ?? '', p.implementation ?? '', ...(p.attributes ?? []), ...(p.operations ?? [])]
}

export default function Palette({
  notation,
  doc,
  tool,
  penColor,
  typeLabel,
  onPenColor,
  onPick,
  onFind,
}: {
  notation: Notation
  doc: DiagramDoc
  tool: Tool
  penColor: string
  typeLabel: (n: DNode) => string
  onPenColor: (c: string) => void
  onPick: (item: PaletteItem) => void
  onFind: (n: DNode) => void
}) {
  const { t } = useTranslation()
  const [query, setQuery] = useState('')
  const [closed, setClosed] = useState<Record<string, boolean>>(readClosed)
  const q = query.trim().toLowerCase()
  const results = useMemo(() => {
    if (!q) return []
    return doc.nodes.filter((n) => searchableText(n, typeLabel(n)).some((s) => s.toLowerCase().includes(q))).slice(0, 12)
  }, [doc.nodes, q, typeLabel])

  const groups = PALETTES[notation]
  const itemLabel = (it: PaletteItem) => t(`diagrams.paleta.itens.${it.key}`)
  const itemMatches = (g: PaletteGroup, it: PaletteItem) => {
    if (!q) return true
    const words = [itemLabel(it), t(`diagrams.paleta.grupos.${g.key}`)]
    if (it.kind === 'node') words.push(t(`diagrams.tipos.${it.type}`))
    if (it.kind === 'edge') words.push(t(`diagrams.arestas.${it.edge}`))
    return words.some((w) => w.toLowerCase().includes(q))
  }
  const visible = groups.map((g) => ({ g, items: g.items.filter((it) => itemMatches(g, it)) })).filter((x) => x.items.length > 0)
  const isClosed = (g: PaletteGroup) => (q ? false : closed[`${notation}:${g.key}`] ?? !!g.closed)
  const toggle = (g: PaletteGroup) => {
    const next = { ...closed, [`${notation}:${g.key}`]: !isClosed(g) }
    setClosed(next)
    writeClosed(next)
  }

  const isActive = (it: PaletteItem) =>
    (it.kind === 'edge' && tool.kind === 'edge' && (tool.key ? tool.key === it.key : tool.edge === it.edge)) ||
    (it.kind === 'pen' && tool.kind === 'pen') ||
    (it.kind === 'eraser' && tool.kind === 'eraser')

  return (
    <div className="dg-palette">
      <div className="dg-find">
        <label className="dg-find__field">
          <Icon name="search" size={13} />
          <TextInput
            type="search"
            value={query}
            autoComplete="off"
            placeholder={t('diagrams.paleta.procurar')}
            aria-label={t('diagrams.paleta.procurarRotulo')}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && results[0]) onFind(results[0])
              else if (e.key === 'Enter' && visible[0]) onPick(visible[0].items[0])
              if (e.key === 'Escape') setQuery('')
            }}
          />
        </label>
        {q && (
          <ul className="dg-find__results" aria-live="polite" aria-label={t('diagrams.paleta.noQuadro')}>
            {results.length === 0 ? (
              <li className="dg-find__none">{visible.length === 0 ? t('diagrams.paleta.semResultados') : t('diagrams.paleta.soNaPaleta')}</li>
            ) : (
              results.map((n) => (
                <li key={n.id}>
                  <button type="button" onClick={() => onFind(n)}>
                    <span className="dg-find__type">{typeLabel(n)}</span>
                    <span className="dg-find__name">{n.name || n.props.text || t('diagrams.semNome')}</span>
                  </button>
                </li>
              ))
            )}
          </ul>
        )}
      </div>

      {visible.map(({ g, items }) => {
        const shut = isClosed(g)
        const title = t(`diagrams.paleta.grupos.${g.key}`)
        return (
          <section key={g.key} className="dg-group" aria-label={title} data-palette-group={g.key}>
            <h3 className="dg-group__title">
              <button type="button" className="dg-group__toggle" aria-expanded={!shut} onClick={() => toggle(g)} disabled={!!q}>
                <Icon name={shut ? 'chevronRight' : 'chevronDown'} size={11} />
                <span>{title}</span>
                <span className="dg-group__count dx-num">{items.length}</span>
              </button>
            </h3>
            {!shut && (
              <div className="dg-group__grid">
                {items.map((it) => (
                  <button
                    key={it.key}
                    type="button"
                    className={cx('dg-item', isActive(it) && 'is-active')}
                    data-palette-item={it.key}
                    aria-pressed={it.kind === 'edge' || it.kind === 'pen' || it.kind === 'eraser' ? isActive(it) : undefined}
                    draggable={it.kind === 'node'}
                    onDragStart={(e) => {
                      e.dataTransfer.setData(PALETTE_MIME, `${notation}:${g.key}:${it.key}`)
                      e.dataTransfer.effectAllowed = 'copy'
                    }}
                    onClick={() => onPick(it)}
                  >
                    <PaletteGlyph it={it} />
                    <span>{itemLabel(it)}</span>
                  </button>
                ))}
              </div>
            )}
          </section>
        )
      })}

      {tool.kind === 'edge' && <p className="dg-help">{t('diagrams.paleta.ajudaAresta')}</p>}
      {tool.kind === 'select' && notation !== 'free' && <p className="dg-help">{t('diagrams.paleta.ajuda')}</p>}

      {notation === 'free' && (
        <div className="dg-pens" role="group" aria-label={t('diagrams.paleta.cores')}>
          {Object.entries(PENS).map(([key, color]) => (
            <button
              key={key}
              type="button"
              className="dg-pen"
              aria-pressed={penColor === color}
              aria-label={t(`diagrams.paleta.cor.${key}`)}
              title={t(`diagrams.paleta.cor.${key}`)}
              style={{ color }}
              onClick={() => onPenColor(color)}
            >
              <span aria-hidden="true" />
            </button>
          ))}
        </div>
      )}
    </div>
  )
}

function PaletteGlyph({ it }: { it: PaletteItem }) {
  return (
    <svg viewBox="0 0 22 18" width={17} height={15} fill="none" stroke="currentColor" strokeWidth={1.3} strokeLinecap="round" strokeLinejoin="round" strokeDasharray={DASHED.has(it.key) ? '3 2' : undefined} aria-hidden="true">
      <path d={GLYPH[it.key] ?? GLYPH.process} />
    </svg>
  )
}

/** Encontra o item de paleta pela chave usada no arrasto (`notação:grupo:item`). */
export function paletteItemByKey(key: string): PaletteItem | null {
  const [notation, group, item] = key.split(':')
  const g = PALETTES[notation as Notation]?.find((x) => x.key === group)
  return g?.items.find((x) => x.key === item) ?? null
}
