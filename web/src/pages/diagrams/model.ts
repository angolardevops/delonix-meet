/**
 * Modelo dos quadros de diagramas (UML 2.5, BPMN 2.0, arquitectura,
 * fluxograma e traço livre).
 *
 * Tudo aqui é dado puro: sem React, sem DOM, sem texto de interface (os nomes
 * visíveis são chaves `diagrams.tipos.*`). A validação, os exportadores e o
 * desenho lêem daqui, e é por isso que se testam sem browser.
 *
 * ONDE VIVE: o modelo editável fica no IndexedDB deste browser (`store.ts`).
 * O servidor só guarda um PNG (POST /api/whiteboards) — guardar o modelo no
 * servidor e editá-lo a várias mãos é do lote 2 (precisa de `doc JSONB` e de
 * uma mensagem de sinalização). Não se finge aqui que já existe.
 */

export type Notation = 'uml' | 'bpmn' | 'arch' | 'flow' | 'free'

export const NOTATIONS: Notation[] = ['uml', 'bpmn', 'arch', 'flow', 'free']

export type NodeType =
  // UML — classes
  | 'class'
  | 'interface'
  | 'enum'
  | 'package'
  | 'note'
  // UML — sequência
  | 'lifeline'
  | 'fragment'
  // UML — casos de uso
  | 'actor'
  | 'usecase'
  | 'boundary'
  // BPMN
  | 'startEvent'
  | 'intermediateEvent'
  | 'endEvent'
  | 'task'
  | 'subProcess'
  | 'gateway'
  | 'pool'
  | 'dataObject'
  | 'annotation'
  // Arquitectura
  | 'service'
  | 'database'
  | 'queue'
  | 'client'
  | 'external'
  | 'zone'
  // Fluxograma
  | 'terminator'
  | 'process'
  | 'decision'
  | 'io'
  | 'document'
  // Livre
  | 'text'

export type EdgeType =
  // UML
  | 'association'
  | 'aggregation'
  | 'composition'
  | 'generalization'
  | 'realization'
  | 'dependency'
  | 'anchor'
  | 'message'
  | 'reply'
  | 'include'
  | 'extend'
  // BPMN
  | 'sequenceFlow'
  | 'messageFlow'
  | 'dataAssociation'
  // Arquitectura
  | 'sync'
  | 'async'
  | 'dataFlow'
  // Fluxograma
  | 'flow'

export type TaskKind = 'none' | 'user' | 'service' | 'script' | 'manual' | 'send' | 'receive'
export type EventTrigger = 'none' | 'message' | 'timer' | 'signal'
export type GatewayKind = 'exclusive' | 'parallel' | 'inclusive' | 'eventBased'
export type MultiInstance = 'none' | 'parallel' | 'sequential'
export type FragmentOperator = 'alt' | 'opt' | 'loop' | 'par' | 'break' | 'critical'

export interface Lane {
  id: string
  name: string
  /** Altura em unidades do quadro. A soma das pistas é a altura da piscina. */
  size: number
}

export interface NodeProps {
  stereotype?: string
  package?: string
  attributes?: string[]
  operations?: string[]
  /** Nota, anotação, texto livre. */
  text?: string
  operator?: FragmentOperator
  /** Comprimento da linha de vida, abaixo da cabeça. */
  length?: number
  taskKind?: TaskKind
  multiInstance?: MultiInstance
  /** Executor de uma tarefa (implementação), ex.: `meet.studio.open`. */
  implementation?: string
  trigger?: EventTrigger
  gatewayKind?: GatewayKind
  lanes?: Lane[]
  /** Pôr em evidência (borda da cor de acção), como a classe seleccionada do template. */
  emphasis?: boolean
}

export interface DNode {
  id: string
  type: NodeType
  x: number
  y: number
  w: number
  h: number
  name: string
  /** Chave de `FILLS` (paint.ts). Vazio = o preenchimento do tipo. */
  fill?: string
  props: NodeProps
}

export interface DEdge {
  id: string
  type: EdgeType
  from: string
  to: string
  label: string
  srcMult?: string
  dstMult?: string
  /** Condição de um fluxo de sequência (BPMN). */
  condition?: string
  /** Fluxo por omissão de uma gateway ou tarefa (BPMN). */
  isDefault?: boolean
  /** Mensagens de sequência: distância vertical ao topo das linhas de vida. */
  offset?: number
}

export interface Stroke {
  id: string
  /** Pares x,y seguidos. */
  points: number[]
  color: string
  width: number
}

export interface DiagramDoc {
  v: 1
  id: string
  title: string
  notation: Notation
  /** Sala a que o quadro fica associado ao guardar (anexa às gravações dela). */
  roomCode: string
  nodes: DNode[]
  edges: DEdge[]
  strokes: Stroke[]
  createdAt: string
  updatedAt: string
  /** Último PNG guardado na biblioteca da organização. */
  boardId?: string
  savedAt?: string
}

export const NODE_NOTATION: Record<NodeType, Notation> = {
  class: 'uml',
  interface: 'uml',
  enum: 'uml',
  package: 'uml',
  note: 'uml',
  lifeline: 'uml',
  fragment: 'uml',
  actor: 'uml',
  usecase: 'uml',
  boundary: 'uml',
  startEvent: 'bpmn',
  intermediateEvent: 'bpmn',
  endEvent: 'bpmn',
  task: 'bpmn',
  subProcess: 'bpmn',
  gateway: 'bpmn',
  pool: 'bpmn',
  dataObject: 'bpmn',
  annotation: 'bpmn',
  service: 'arch',
  database: 'arch',
  queue: 'arch',
  client: 'arch',
  external: 'arch',
  zone: 'arch',
  terminator: 'flow',
  process: 'flow',
  decision: 'flow',
  io: 'flow',
  document: 'flow',
  text: 'free',
}

export const EDGE_NOTATION: Record<EdgeType, Notation> = {
  association: 'uml',
  aggregation: 'uml',
  composition: 'uml',
  generalization: 'uml',
  realization: 'uml',
  dependency: 'uml',
  anchor: 'uml',
  message: 'uml',
  reply: 'uml',
  include: 'uml',
  extend: 'uml',
  sequenceFlow: 'bpmn',
  messageFlow: 'bpmn',
  dataAssociation: 'bpmn',
  sync: 'arch',
  async: 'arch',
  dataFlow: 'arch',
  flow: 'flow',
}

/** Contentores: desenham-se por baixo e não se ligam por setas de fluxo. */
export const CONTAINERS: ReadonlySet<NodeType> = new Set(['package', 'fragment', 'boundary', 'pool', 'zone'])

export const CLASSIFIERS: ReadonlySet<NodeType> = new Set(['class', 'interface', 'enum'])

export const BPMN_FLOW_NODES: ReadonlySet<NodeType> = new Set([
  'startEvent',
  'intermediateEvent',
  'endEvent',
  'task',
  'subProcess',
  'gateway',
])

// ---------------------------------------------------------------------------
//  Paletas: o que cada separador oferece, agrupado como no template.
// ---------------------------------------------------------------------------

export type PaletteItem =
  | { kind: 'node'; key: string; type: NodeType; props?: NodeProps }
  | { kind: 'edge'; key: string; edge: EdgeType }
  | { kind: 'lane'; key: string }
  | { kind: 'pen'; key: string }
  | { kind: 'eraser'; key: string }

export interface PaletteGroup {
  key: string
  items: PaletteItem[]
}

export const PALETTES: Record<Notation, PaletteGroup[]> = {
  uml: [
    {
      key: 'classes',
      items: [
        { kind: 'node', key: 'class', type: 'class' },
        { kind: 'node', key: 'interface', type: 'interface' },
        { kind: 'node', key: 'enum', type: 'enum' },
        { kind: 'node', key: 'package', type: 'package' },
        { kind: 'node', key: 'note', type: 'note' },
        { kind: 'edge', key: 'generalization', edge: 'generalization' },
        { kind: 'edge', key: 'association', edge: 'association' },
        { kind: 'edge', key: 'composition', edge: 'composition' },
      ],
    },
    {
      key: 'sequence',
      items: [
        { kind: 'node', key: 'lifeline', type: 'lifeline' },
        { kind: 'edge', key: 'message', edge: 'message' },
        { kind: 'edge', key: 'reply', edge: 'reply' },
        { kind: 'node', key: 'fragment', type: 'fragment' },
      ],
    },
    {
      key: 'usecases',
      items: [
        { kind: 'node', key: 'actor', type: 'actor' },
        { kind: 'node', key: 'usecase', type: 'usecase' },
        { kind: 'edge', key: 'include', edge: 'include' },
        { kind: 'node', key: 'boundary', type: 'boundary' },
      ],
    },
  ],
  bpmn: [
    {
      key: 'events',
      items: [
        { kind: 'node', key: 'startEvent', type: 'startEvent', props: { trigger: 'none' } },
        { kind: 'node', key: 'messageEvent', type: 'startEvent', props: { trigger: 'message' } },
        { kind: 'node', key: 'timerEvent', type: 'intermediateEvent', props: { trigger: 'timer' } },
        { kind: 'node', key: 'endEvent', type: 'endEvent', props: { trigger: 'none' } },
      ],
    },
    {
      key: 'tasks',
      items: [
        { kind: 'node', key: 'task', type: 'task', props: { taskKind: 'none' } },
        { kind: 'node', key: 'userTask', type: 'task', props: { taskKind: 'user' } },
        { kind: 'node', key: 'serviceTask', type: 'task', props: { taskKind: 'service' } },
        { kind: 'node', key: 'subProcess', type: 'subProcess' },
      ],
    },
    {
      key: 'gateways',
      items: [
        { kind: 'node', key: 'exclusiveGateway', type: 'gateway', props: { gatewayKind: 'exclusive' } },
        { kind: 'node', key: 'parallelGateway', type: 'gateway', props: { gatewayKind: 'parallel' } },
        { kind: 'node', key: 'inclusiveGateway', type: 'gateway', props: { gatewayKind: 'inclusive' } },
        { kind: 'node', key: 'eventGateway', type: 'gateway', props: { gatewayKind: 'eventBased' } },
      ],
    },
    {
      key: 'structure',
      items: [
        { kind: 'node', key: 'pool', type: 'pool' },
        { kind: 'lane', key: 'lane' },
        { kind: 'node', key: 'dataObject', type: 'dataObject' },
        { kind: 'node', key: 'annotation', type: 'annotation' },
      ],
    },
  ],
  arch: [
    {
      key: 'components',
      items: [
        { kind: 'node', key: 'service', type: 'service' },
        { kind: 'node', key: 'database', type: 'database' },
        { kind: 'node', key: 'queue', type: 'queue' },
        { kind: 'node', key: 'client', type: 'client' },
        { kind: 'node', key: 'external', type: 'external' },
        { kind: 'node', key: 'zone', type: 'zone' },
      ],
    },
    {
      key: 'links',
      items: [
        { kind: 'edge', key: 'sync', edge: 'sync' },
        { kind: 'edge', key: 'async', edge: 'async' },
        { kind: 'edge', key: 'dataFlow', edge: 'dataFlow' },
        { kind: 'node', key: 'note', type: 'note' },
      ],
    },
  ],
  flow: [
    {
      key: 'shapes',
      items: [
        { kind: 'node', key: 'terminator', type: 'terminator' },
        { kind: 'node', key: 'process', type: 'process' },
        { kind: 'node', key: 'decision', type: 'decision' },
        { kind: 'node', key: 'io', type: 'io' },
        { kind: 'node', key: 'document', type: 'document' },
        { kind: 'edge', key: 'flow', edge: 'flow' },
      ],
    },
  ],
  free: [
    {
      key: 'draw',
      items: [
        { kind: 'pen', key: 'pen' },
        { kind: 'eraser', key: 'eraser' },
        { kind: 'node', key: 'text', type: 'text' },
        { kind: 'node', key: 'note', type: 'note' },
      ],
    },
  ],
}

// ---------------------------------------------------------------------------
//  Tamanhos por omissão
// ---------------------------------------------------------------------------

const SIZES: Record<NodeType, [number, number]> = {
  class: [200, 0],
  interface: [190, 0],
  enum: [170, 0],
  package: [320, 220],
  note: [190, 70],
  lifeline: [126, 34],
  fragment: [360, 120],
  actor: [60, 86],
  usecase: [180, 44],
  boundary: [300, 240],
  startEvent: [36, 36],
  intermediateEvent: [36, 36],
  endEvent: [36, 36],
  task: [130, 64],
  subProcess: [150, 80],
  gateway: [50, 50],
  pool: [760, 300],
  dataObject: [40, 52],
  annotation: [150, 50],
  service: [160, 64],
  database: [120, 80],
  queue: [150, 50],
  client: [130, 60],
  external: [160, 64],
  zone: [420, 260],
  terminator: [140, 48],
  process: [150, 60],
  decision: [120, 80],
  io: [150, 56],
  document: [150, 66],
  text: [240, 30],
}

export function uid(prefix = 'n'): string {
  const rnd = Math.random().toString(36).slice(2, 8)
  return prefix + Date.now().toString(36) + rnd
}

/**
 * Elemento novo. `name` já vem traduzido (quem chama sabe o idioma); os
 * atributos por omissão são sintaxe UML, não prosa.
 */
export function makeNode(type: NodeType, x: number, y: number, name: string, props: NodeProps = {}, id = uid('n')): DNode {
  const [w, h] = SIZES[type]
  const base: NodeProps = {}
  if (type === 'class') Object.assign(base, { attributes: [], operations: [] })
  if (type === 'interface') Object.assign(base, { stereotype: 'interface', attributes: [], operations: [] })
  if (type === 'enum') Object.assign(base, { stereotype: 'enumeration', attributes: [], operations: [] })
  if (type === 'lifeline') Object.assign(base, { length: 240 })
  if (type === 'fragment') Object.assign(base, { operator: 'alt' })
  if (type === 'task') Object.assign(base, { taskKind: 'none', multiInstance: 'none' })
  if (type === 'startEvent' || type === 'intermediateEvent' || type === 'endEvent') Object.assign(base, { trigger: 'none' })
  if (type === 'gateway') Object.assign(base, { gatewayKind: 'exclusive' })
  if (type === 'pool') Object.assign(base, { lanes: [] })
  return { id, type, x: Math.round(x), y: Math.round(y), w, h, name, props: { ...base, ...props } }
}

export function emptyDoc(id: string, title: string, notation: Notation, roomCode = '', now = new Date().toISOString()): DiagramDoc {
  return { v: 1, id, title, notation, roomCode, nodes: [], edges: [], strokes: [], createdAt: now, updatedAt: now }
}

export function nodeById(doc: Pick<DiagramDoc, 'nodes'>, id: string): DNode | undefined {
  return doc.nodes.find((n) => n.id === id)
}

// ---------------------------------------------------------------------------
//  Que ligações fazem sentido — usado ao ligar E pela validação.
// ---------------------------------------------------------------------------

const USECASE_SIDE: ReadonlySet<NodeType> = new Set(['actor', 'usecase'])
const ARCH_NODES: ReadonlySet<NodeType> = new Set(['service', 'database', 'queue', 'client', 'external'])
const FLOW_NODES: ReadonlySet<NodeType> = new Set(['terminator', 'process', 'decision', 'io', 'document'])

/** `true` se uma aresta deste tipo pode ligar estes dois elementos. */
export function canConnect(type: EdgeType, a: DNode, b: DNode): boolean {
  if (type === 'anchor') return (a.type === 'note') !== (b.type === 'note')
  switch (type) {
    case 'association':
      return (CLASSIFIERS.has(a.type) && CLASSIFIERS.has(b.type)) || (USECASE_SIDE.has(a.type) && USECASE_SIDE.has(b.type) && a.type !== b.type)
    case 'aggregation':
    case 'composition':
      return CLASSIFIERS.has(a.type) && CLASSIFIERS.has(b.type)
    case 'generalization':
      return (
        (CLASSIFIERS.has(a.type) && CLASSIFIERS.has(b.type)) ||
        (a.type === 'actor' && b.type === 'actor') ||
        (a.type === 'usecase' && b.type === 'usecase')
      )
    case 'realization':
      return CLASSIFIERS.has(a.type) && CLASSIFIERS.has(b.type)
    case 'dependency':
      return (CLASSIFIERS.has(a.type) || a.type === 'package') && (CLASSIFIERS.has(b.type) || b.type === 'package')
    case 'message':
    case 'reply':
      return a.type === 'lifeline' && b.type === 'lifeline'
    case 'include':
    case 'extend':
      return a.type === 'usecase' && b.type === 'usecase' && a.id !== b.id
    case 'sequenceFlow':
      return BPMN_FLOW_NODES.has(a.type) && BPMN_FLOW_NODES.has(b.type)
    case 'messageFlow':
      return (BPMN_FLOW_NODES.has(a.type) || a.type === 'pool') && (BPMN_FLOW_NODES.has(b.type) || b.type === 'pool')
    case 'dataAssociation':
      return (
        ((a.type === 'dataObject' || a.type === 'annotation') && b.type !== 'pool') ||
        ((b.type === 'dataObject' || b.type === 'annotation') && a.type !== 'pool')
      )
    case 'sync':
    case 'async':
    case 'dataFlow':
      return ARCH_NODES.has(a.type) && ARCH_NODES.has(b.type)
    case 'flow':
      return FLOW_NODES.has(a.type) && FLOW_NODES.has(b.type)
  }
  return false
}

/** Tipos de aresta que se podem escolher para um par (o inspector usa isto). */
export function edgeTypesFor(a: DNode, b: DNode): EdgeType[] {
  return (Object.keys(EDGE_NOTATION) as EdgeType[]).filter((t) => canConnect(t, a, b))
}

/**
 * Tipo de ligação por omissão quando se arrasta do puxador de um elemento
 * para outro, sem ferramenta de aresta escolhida.
 */
export function defaultEdgeType(a: DNode, b: DNode): EdgeType | null {
  const pref: EdgeType[] = [
    'message',
    'include',
    'association',
    'anchor',
    'sequenceFlow',
    'dataAssociation',
    'messageFlow',
    'sync',
    'flow',
    'dependency',
  ]
  for (const t of pref) if (canConnect(t, a, b)) return t
  return null
}

/** Sintaxe de atributo UML: `− nome: Tipo [mult] = valor`. */
export interface ParsedMember {
  visibility: '+' | '-' | '#' | '~' | ''
  name: string
  type: string
  params: string
  isOperation: boolean
}

export function parseMember(raw: string): ParsedMember {
  const s = raw.trim().replace(/^−/, '-')
  const vis = /^[+\-#~]/.test(s) ? (s[0] as ParsedMember['visibility']) : ''
  const rest = (vis ? s.slice(1) : s).trim()
  const op = /^([^(:]+)\(([^)]*)\)\s*(?::\s*(.+))?$/.exec(rest)
  if (op) return { visibility: vis, name: op[1].trim(), params: op[2].trim(), type: (op[3] ?? '').trim(), isOperation: true }
  const at = /^([^:=]+?)\s*(?::\s*([^=]+?))?\s*(?:=.*)?$/.exec(rest)
  return { visibility: vis, name: (at?.[1] ?? rest).trim(), type: (at?.[2] ?? '').trim(), params: '', isOperation: false }
}

/** Multiplicidade UML válida: `1`, `*`, `0..1`, `1..*`, `2..5`. */
export function isValidMultiplicity(m: string): boolean {
  const s = m.trim()
  if (!s) return true
  const r = /^(\d+|\*)(?:\.\.(\d+|\*))?$/.exec(s)
  if (!r) return false
  if (r[1] === '*' && r[2] !== undefined) return false
  if (r[2] === undefined || r[2] === '*') return true
  return Number(r[1]) <= Number(r[2])
}

/**
 * Forma canónica de uma multiplicidade, quando há uma sem ambiguidade:
 * espaços fora, `1..1` → `1`, `0..n`/`n` → `*`, `5..2` → `2..5`. `null` se
 * não houver correcção determinística.
 */
export function normalizeMultiplicity(m: string): string | null {
  let s = m.replace(/\s+/g, '').replace(/…/g, '..').replace(/\.{3,}/g, '..').replace(/[nN]/g, '*')
  if (s === '*..*') s = '*'
  const r = /^(\d+|\*)(?:\.\.(\d+|\*))?$/.exec(s)
  if (!r) return null
  const [lo, hi] = [r[1], r[2]]
  if (hi === undefined) return lo
  if (lo === '*') return hi === '*' ? '*' : null
  if (hi === '*') return `${lo}..*`
  const a = Number(lo)
  const b = Number(hi)
  if (a === b) return String(a)
  return a < b ? `${a}..${b}` : `${b}..${a}`
}
