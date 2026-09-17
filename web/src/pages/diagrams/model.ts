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

import { CATALOG_CONTAINERS, CATALOG_GROUPS, CATALOG_ITEMS, catalogKey } from './catalog'

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
  // UML — sequência (execução)
  | 'activation'
  // UML — actividade
  | 'initialNode'
  | 'activityFinal'
  | 'flowFinal'
  | 'action'
  | 'decisionNode'
  | 'forkNode'
  | 'partition'
  | 'objectNode'
  // UML — estados
  | 'state'
  | 'compositeState'
  | 'stateInitial'
  | 'stateFinal'
  | 'choice'
  | 'history'
  // UML — componentes e implantação
  | 'component'
  | 'port'
  | 'providedInterface'
  | 'requiredInterface'
  | 'deviceNode'
  | 'artifact'
  // UML — objectos
  | 'object'
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
  | 'dataStore'
  | 'group'
  // Arquitectura
  | 'service'
  | 'database'
  | 'queue'
  | 'client'
  | 'external'
  | 'zone'
  // Arquitectura — C4
  | 'c4Person'
  | 'c4System'
  | 'c4Container'
  | 'c4Component'
  | 'c4Code'
  | 'c4Boundary'
  | 'c4DeploymentNode'
  // Arquitectura — catálogo (cloud, fornecedores, Kubernetes, rede, on-prem, plataforma)
  | 'resource'
  | 'resourceGroup'
  // Fluxograma
  | 'terminator'
  | 'process'
  | 'decision'
  | 'io'
  | 'document'
  | 'predefinedProcess'
  | 'manualInput'
  | 'manualOperation'
  | 'preparation'
  | 'delay'
  | 'merge'
  | 'loopLimit'
  | 'display'
  | 'multiDocument'
  | 'storedData'
  | 'flowDatabase'
  | 'internalStorage'
  | 'sequentialStorage'
  | 'directAccessStorage'
  | 'connector'
  | 'offPageConnector'
  | 'flowAnnotation'
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
  | 'lostMessage'
  | 'foundMessage'
  | 'controlFlow'
  | 'transition'
  | 'usage'
  | 'deploy'
  | 'manifest'
  | 'link'
  // BPMN
  | 'sequenceFlow'
  | 'messageFlow'
  | 'dataAssociation'
  // Arquitectura
  | 'sync'
  | 'async'
  | 'dataFlow'
  | 'c4Rel'
  // Fluxograma
  | 'flow'
  | 'flowNote'

export type TaskKind = 'none' | 'user' | 'service' | 'script' | 'manual' | 'send' | 'receive' | 'businessRule' | 'call'
export const TASK_KINDS: TaskKind[] = ['none', 'user', 'service', 'script', 'manual', 'send', 'receive', 'businessRule', 'call']
export type EventTrigger = 'none' | 'message' | 'timer' | 'signal' | 'error' | 'escalation' | 'compensation' | 'conditional' | 'link' | 'terminate'
/** Gatilhos que cada tipo de evento aceita (BPMN 2.0, tabela 10.93). */
export const TRIGGERS: Record<'startEvent' | 'intermediateEvent' | 'endEvent', EventTrigger[]> = {
  startEvent: ['none', 'message', 'timer', 'signal', 'conditional'],
  intermediateEvent: ['message', 'timer', 'signal', 'error', 'escalation', 'compensation', 'conditional', 'link'],
  endEvent: ['none', 'message', 'signal', 'error', 'escalation', 'compensation', 'terminate'],
}
/** Gatilhos que um evento intermédio pode LANÇAR (os outros só se apanham). */
export const THROWABLE: ReadonlySet<EventTrigger> = new Set(['message', 'signal', 'escalation', 'compensation', 'link'])
export type GatewayKind = 'exclusive' | 'parallel' | 'inclusive' | 'eventBased' | 'complex' | 'eventInstantiate' | 'eventParallel'
export const GATEWAY_KINDS: GatewayKind[] = ['exclusive', 'parallel', 'inclusive', 'eventBased', 'complex', 'eventInstantiate', 'eventParallel']
export const EVENT_GATEWAYS: ReadonlySet<GatewayKind> = new Set(['eventBased', 'eventInstantiate', 'eventParallel'])
export type DataRole = 'none' | 'input' | 'output'
export type C4Shape = 'app' | 'db' | 'queue' | 'web' | 'mobile'
export const C4_SHAPES: C4Shape[] = ['app', 'db', 'queue', 'web', 'mobile']
export type BoundaryKind = 'system' | 'container' | 'enterprise'
export const BOUNDARY_KINDS: BoundaryKind[] = ['system', 'container', 'enterprise']
export type MultiInstance = 'none' | 'parallel' | 'sequential'
export type FragmentOperator = 'alt' | 'opt' | 'loop' | 'par' | 'break' | 'critical' | 'neg' | 'strict' | 'seq' | 'ignore' | 'consider' | 'assert' | 'ref'

export const FRAGMENT_OPERATORS: FragmentOperator[] = ['alt', 'opt', 'loop', 'par', 'break', 'critical', 'neg', 'strict', 'seq', 'ignore', 'consider', 'assert', 'ref']

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
  /** Evento intermédio que LANÇA (marcador preenchido) em vez de apanhar. */
  throwing?: boolean
  /** Evento intermédio preso à borda de uma actividade. */
  boundary?: boolean
  /** Evento de fronteira que NÃO interrompe a actividade (traço interrompido). */
  nonInterrupting?: boolean
  /** Marcadores de actividade BPMN. */
  loop?: boolean
  compensation?: boolean
  adHoc?: boolean
  dataRole?: DataRole
  collection?: boolean
  gatewayKind?: GatewayKind
  /** C4 e catálogo: descrição curta (o que faz). */
  description?: string
  /** C4 e catálogo: tecnologia (`Rust / Axum`, `PostgreSQL 17`). */
  technology?: string
  /** C4: elemento externo ao sistema que se está a descrever. */
  external?: boolean
  c4Shape?: C4Shape
  boundaryKind?: BoundaryKind
  /** Catálogo: chave `grupo.item` (`aws.ec2`, `k8s.pod`). */
  catalog?: string
  lanes?: Lane[]
  /** Objecto UML: a classe de que é instância (`s1: Session`). */
  instanceOf?: string
  /** Histórico profundo (`H*`) em vez de superficial (`H`). */
  deep?: boolean
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
  /** C4 e arquitectura: tecnologia da relação (`HTTPS/JSON`, `gRPC`). */
  technology?: string
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
  activation: 'uml',
  initialNode: 'uml',
  activityFinal: 'uml',
  flowFinal: 'uml',
  action: 'uml',
  decisionNode: 'uml',
  forkNode: 'uml',
  partition: 'uml',
  objectNode: 'uml',
  state: 'uml',
  compositeState: 'uml',
  stateInitial: 'uml',
  stateFinal: 'uml',
  choice: 'uml',
  history: 'uml',
  component: 'uml',
  port: 'uml',
  providedInterface: 'uml',
  requiredInterface: 'uml',
  deviceNode: 'uml',
  artifact: 'uml',
  object: 'uml',
  startEvent: 'bpmn',
  intermediateEvent: 'bpmn',
  endEvent: 'bpmn',
  task: 'bpmn',
  subProcess: 'bpmn',
  gateway: 'bpmn',
  pool: 'bpmn',
  dataObject: 'bpmn',
  annotation: 'bpmn',
  dataStore: 'bpmn',
  group: 'bpmn',
  service: 'arch',
  database: 'arch',
  queue: 'arch',
  client: 'arch',
  external: 'arch',
  zone: 'arch',
  c4Person: 'arch',
  c4System: 'arch',
  c4Container: 'arch',
  c4Component: 'arch',
  c4Code: 'arch',
  c4Boundary: 'arch',
  c4DeploymentNode: 'arch',
  resource: 'arch',
  resourceGroup: 'arch',
  terminator: 'flow',
  process: 'flow',
  decision: 'flow',
  io: 'flow',
  document: 'flow',
  predefinedProcess: 'flow',
  manualInput: 'flow',
  manualOperation: 'flow',
  preparation: 'flow',
  delay: 'flow',
  merge: 'flow',
  loopLimit: 'flow',
  display: 'flow',
  multiDocument: 'flow',
  storedData: 'flow',
  flowDatabase: 'flow',
  internalStorage: 'flow',
  sequentialStorage: 'flow',
  directAccessStorage: 'flow',
  connector: 'flow',
  offPageConnector: 'flow',
  flowAnnotation: 'flow',
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
  lostMessage: 'uml',
  foundMessage: 'uml',
  controlFlow: 'uml',
  transition: 'uml',
  usage: 'uml',
  deploy: 'uml',
  manifest: 'uml',
  link: 'uml',
  sequenceFlow: 'bpmn',
  messageFlow: 'bpmn',
  dataAssociation: 'bpmn',
  sync: 'arch',
  async: 'arch',
  dataFlow: 'arch',
  c4Rel: 'arch',
  flow: 'flow',
  flowNote: 'flow',
}

/** Contentores: desenham-se por baixo e não se ligam por setas de fluxo. */
export const CONTAINERS: ReadonlySet<NodeType> = new Set(['package', 'fragment', 'boundary', 'pool', 'zone', 'partition', 'compositeState', 'deviceNode', 'group', 'c4Boundary', 'c4DeploymentNode', 'resourceGroup'])

/** Elementos C4 (não contentores). */
export const C4_ELEMENTS: ReadonlySet<NodeType> = new Set(['c4Person', 'c4System', 'c4Container', 'c4Component', 'c4Code'])

/** Nós de actividade UML (ligam-se por fluxo de controlo). */
export const ACTIVITY_NODES: ReadonlySet<NodeType> = new Set(['initialNode', 'activityFinal', 'flowFinal', 'action', 'decisionNode', 'forkNode', 'objectNode'])

/** Vértices de máquina de estados UML (ligam-se por transição). */
export const STATE_NODES: ReadonlySet<NodeType> = new Set(['state', 'compositeState', 'stateInitial', 'stateFinal', 'choice', 'history'])

/** Elementos de tamanho fixo: o nome vai por baixo e não se redimensionam. */
export const FIXED_SIZE: ReadonlySet<NodeType> = new Set([
  'actor',
  'connector',
  'initialNode',
  'activityFinal',
  'flowFinal',
  'decisionNode',
  'stateInitial',
  'stateFinal',
  'choice',
  'history',
  'port',
  'providedInterface',
  'requiredInterface',
])

/** Elementos cujo nome se escreve por BAIXO da forma (conta para o enquadramento). */
export const LABEL_BELOW: ReadonlySet<NodeType> = new Set([
  'actor',
  'decisionNode',
  'choice',
  'history',
  'port',
  'providedInterface',
  'requiredInterface',
  'initialNode',
  'activityFinal',
  'flowFinal',
  'stateInitial',
  'stateFinal',
  'dataStore',
])

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
  /** Recolhido por omissão (a pessoa pode abri-lo; a escolha fica no browser). */
  closed?: boolean
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
        { kind: 'edge', key: 'aggregation', edge: 'aggregation' },
        { kind: 'edge', key: 'realization', edge: 'realization' },
        { kind: 'edge', key: 'dependency', edge: 'dependency' },
      ],
    },
    {
      key: 'sequence',
      items: [
        { kind: 'node', key: 'lifeline', type: 'lifeline' },
        { kind: 'edge', key: 'message', edge: 'message' },
        { kind: 'edge', key: 'reply', edge: 'reply' },
        { kind: 'node', key: 'fragment', type: 'fragment' },
        { kind: 'node', key: 'activation', type: 'activation' },
        { kind: 'edge', key: 'selfMessage', edge: 'message' },
        { kind: 'edge', key: 'lostMessage', edge: 'lostMessage' },
        { kind: 'edge', key: 'foundMessage', edge: 'foundMessage' },
      ],
    },
    {
      key: 'usecases',
      items: [
        { kind: 'node', key: 'actor', type: 'actor' },
        { kind: 'node', key: 'usecase', type: 'usecase' },
        { kind: 'edge', key: 'include', edge: 'include' },
        { kind: 'node', key: 'boundary', type: 'boundary' },
        { kind: 'edge', key: 'extend', edge: 'extend' },
        { kind: 'edge', key: 'ucGeneralization', edge: 'generalization' },
      ],
    },
    {
      key: 'activity',
      closed: true,
      items: [
        { kind: 'node', key: 'initialNode', type: 'initialNode' },
        { kind: 'node', key: 'activityFinal', type: 'activityFinal' },
        { kind: 'node', key: 'flowFinal', type: 'flowFinal' },
        { kind: 'node', key: 'action', type: 'action' },
        { kind: 'node', key: 'decisionNode', type: 'decisionNode' },
        { kind: 'node', key: 'forkNode', type: 'forkNode' },
        { kind: 'node', key: 'partition', type: 'partition' },
        { kind: 'node', key: 'objectNode', type: 'objectNode' },
        { kind: 'edge', key: 'controlFlow', edge: 'controlFlow' },
      ],
    },
    {
      key: 'states',
      closed: true,
      items: [
        { kind: 'node', key: 'state', type: 'state' },
        { kind: 'node', key: 'compositeState', type: 'compositeState' },
        { kind: 'node', key: 'stateInitial', type: 'stateInitial' },
        { kind: 'node', key: 'stateFinal', type: 'stateFinal' },
        { kind: 'node', key: 'choice', type: 'choice' },
        { kind: 'node', key: 'history', type: 'history' },
        { kind: 'edge', key: 'transition', edge: 'transition' },
      ],
    },
    {
      key: 'components',
      closed: true,
      items: [
        { kind: 'node', key: 'component', type: 'component' },
        { kind: 'node', key: 'port', type: 'port' },
        { kind: 'node', key: 'providedInterface', type: 'providedInterface' },
        { kind: 'node', key: 'requiredInterface', type: 'requiredInterface' },
        { kind: 'node', key: 'deviceNode', type: 'deviceNode' },
        { kind: 'node', key: 'artifact', type: 'artifact' },
        { kind: 'edge', key: 'usage', edge: 'usage' },
        { kind: 'edge', key: 'deploy', edge: 'deploy' },
        { kind: 'edge', key: 'manifest', edge: 'manifest' },
      ],
    },
    {
      key: 'objects',
      closed: true,
      items: [
        { kind: 'node', key: 'object', type: 'object' },
        { kind: 'edge', key: 'link', edge: 'link' },
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
    {
      key: 'intermediate',
      closed: true,
      items: [
        { kind: 'node', key: 'catchMessage', type: 'intermediateEvent', props: { trigger: 'message' } },
        { kind: 'node', key: 'throwMessage', type: 'intermediateEvent', props: { trigger: 'message', throwing: true } },
        { kind: 'node', key: 'catchSignal', type: 'intermediateEvent', props: { trigger: 'signal' } },
        { kind: 'node', key: 'throwSignal', type: 'intermediateEvent', props: { trigger: 'signal', throwing: true } },
        { kind: 'node', key: 'conditionalEvent', type: 'intermediateEvent', props: { trigger: 'conditional' } },
        { kind: 'node', key: 'throwEscalation', type: 'intermediateEvent', props: { trigger: 'escalation', throwing: true } },
        { kind: 'node', key: 'throwCompensation', type: 'intermediateEvent', props: { trigger: 'compensation', throwing: true } },
        { kind: 'node', key: 'catchLink', type: 'intermediateEvent', props: { trigger: 'link' } },
        { kind: 'node', key: 'throwLink', type: 'intermediateEvent', props: { trigger: 'link', throwing: true } },
        { kind: 'node', key: 'boundaryTimer', type: 'intermediateEvent', props: { trigger: 'timer', boundary: true } },
        { kind: 'node', key: 'boundaryError', type: 'intermediateEvent', props: { trigger: 'error', boundary: true } },
        { kind: 'node', key: 'boundaryMessage', type: 'intermediateEvent', props: { trigger: 'message', boundary: true, nonInterrupting: true } },
      ],
    },
    {
      key: 'endEvents',
      closed: true,
      items: [
        { kind: 'node', key: 'messageEnd', type: 'endEvent', props: { trigger: 'message' } },
        { kind: 'node', key: 'signalEnd', type: 'endEvent', props: { trigger: 'signal' } },
        { kind: 'node', key: 'errorEnd', type: 'endEvent', props: { trigger: 'error' } },
        { kind: 'node', key: 'escalationEnd', type: 'endEvent', props: { trigger: 'escalation' } },
        { kind: 'node', key: 'compensationEnd', type: 'endEvent', props: { trigger: 'compensation' } },
        { kind: 'node', key: 'terminateEnd', type: 'endEvent', props: { trigger: 'terminate' } },
        { kind: 'node', key: 'timerStart', type: 'startEvent', props: { trigger: 'timer' } },
        { kind: 'node', key: 'signalStart', type: 'startEvent', props: { trigger: 'signal' } },
        { kind: 'node', key: 'conditionalStart', type: 'startEvent', props: { trigger: 'conditional' } },
      ],
    },
    {
      key: 'taskTypes',
      closed: true,
      items: [
        { kind: 'node', key: 'sendTask', type: 'task', props: { taskKind: 'send' } },
        { kind: 'node', key: 'receiveTask', type: 'task', props: { taskKind: 'receive' } },
        { kind: 'node', key: 'manualTask', type: 'task', props: { taskKind: 'manual' } },
        { kind: 'node', key: 'scriptTask', type: 'task', props: { taskKind: 'script' } },
        { kind: 'node', key: 'businessRuleTask', type: 'task', props: { taskKind: 'businessRule' } },
        { kind: 'node', key: 'callActivity', type: 'task', props: { taskKind: 'call' } },
        { kind: 'node', key: 'loopTask', type: 'task', props: { loop: true } },
        { kind: 'node', key: 'multiParallel', type: 'task', props: { multiInstance: 'parallel' } },
        { kind: 'node', key: 'multiSequential', type: 'task', props: { multiInstance: 'sequential' } },
        { kind: 'node', key: 'compensationTask', type: 'task', props: { compensation: true } },
        { kind: 'node', key: 'adHocSubProcess', type: 'subProcess', props: { adHoc: true } },
      ],
    },
    {
      key: 'moreGateways',
      closed: true,
      items: [
        { kind: 'node', key: 'complexGateway', type: 'gateway', props: { gatewayKind: 'complex' } },
        { kind: 'node', key: 'eventInstantiateGateway', type: 'gateway', props: { gatewayKind: 'eventInstantiate' } },
        { kind: 'node', key: 'eventParallelGateway', type: 'gateway', props: { gatewayKind: 'eventParallel' } },
      ],
    },
    {
      key: 'dataAndFlows',
      closed: true,
      items: [
        { kind: 'node', key: 'dataStore', type: 'dataStore' },
        { kind: 'node', key: 'dataInput', type: 'dataObject', props: { dataRole: 'input' } },
        { kind: 'node', key: 'dataOutput', type: 'dataObject', props: { dataRole: 'output' } },
        { kind: 'node', key: 'dataCollection', type: 'dataObject', props: { collection: true } },
        { kind: 'node', key: 'group', type: 'group' },
        { kind: 'edge', key: 'sequenceFlow', edge: 'sequenceFlow' },
        { kind: 'edge', key: 'messageFlow', edge: 'messageFlow' },
        { kind: 'edge', key: 'dataAssociation', edge: 'dataAssociation' },
      ],
    },
  ],
  arch: [
    {
      key: 'c4',
      items: [
        { kind: 'node', key: 'c4Person', type: 'c4Person' },
        { kind: 'node', key: 'c4PersonExt', type: 'c4Person', props: { external: true } },
        { kind: 'node', key: 'c4System', type: 'c4System' },
        { kind: 'node', key: 'c4SystemExt', type: 'c4System', props: { external: true } },
        { kind: 'node', key: 'c4Container', type: 'c4Container', props: { c4Shape: 'app' } },
        { kind: 'node', key: 'c4ContainerDb', type: 'c4Container', props: { c4Shape: 'db' } },
        { kind: 'node', key: 'c4ContainerQueue', type: 'c4Container', props: { c4Shape: 'queue' } },
        { kind: 'node', key: 'c4ContainerWeb', type: 'c4Container', props: { c4Shape: 'web' } },
        { kind: 'node', key: 'c4ContainerMobile', type: 'c4Container', props: { c4Shape: 'mobile' } },
        { kind: 'node', key: 'c4Component', type: 'c4Component' },
        { kind: 'node', key: 'c4Code', type: 'c4Code' },
        { kind: 'node', key: 'c4SystemBoundary', type: 'c4Boundary', props: { boundaryKind: 'system' } },
        { kind: 'node', key: 'c4ContainerBoundary', type: 'c4Boundary', props: { boundaryKind: 'container' } },
        { kind: 'node', key: 'c4DeploymentNode', type: 'c4DeploymentNode' },
        { kind: 'edge', key: 'c4Rel', edge: 'c4Rel' },
      ],
    },
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
    ...CATALOG_GROUPS.map(
      (g): PaletteGroup => ({
        key: g,
        closed: g !== 'cloud',
        items: CATALOG_ITEMS[g].map((item): PaletteItem => {
          const key = catalogKey(g, item)
          return { kind: 'node', key, type: CATALOG_CONTAINERS.has(key) ? 'resourceGroup' : 'resource', props: { catalog: key } }
        }),
      }),
    ),
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
    {
      key: 'flowProcess',
      items: [
        { kind: 'node', key: 'predefinedProcess', type: 'predefinedProcess' },
        { kind: 'node', key: 'preparation', type: 'preparation' },
        { kind: 'node', key: 'manualInput', type: 'manualInput' },
        { kind: 'node', key: 'manualOperation', type: 'manualOperation' },
        { kind: 'node', key: 'delay', type: 'delay' },
        { kind: 'node', key: 'merge', type: 'merge' },
        { kind: 'node', key: 'loopLimit', type: 'loopLimit' },
        { kind: 'node', key: 'display', type: 'display' },
        { kind: 'node', key: 'multiDocument', type: 'multiDocument' },
      ],
    },
    {
      key: 'flowData',
      items: [
        { kind: 'node', key: 'flowDatabase', type: 'flowDatabase' },
        { kind: 'node', key: 'storedData', type: 'storedData' },
        { kind: 'node', key: 'internalStorage', type: 'internalStorage' },
        { kind: 'node', key: 'sequentialStorage', type: 'sequentialStorage' },
        { kind: 'node', key: 'directAccessStorage', type: 'directAccessStorage' },
      ],
    },
    {
      key: 'flowConnectors',
      items: [
        { kind: 'node', key: 'connector', type: 'connector' },
        { kind: 'node', key: 'offPageConnector', type: 'offPageConnector' },
        { kind: 'node', key: 'flowAnnotation', type: 'flowAnnotation' },
        { kind: 'edge', key: 'flowNote', edge: 'flowNote' },
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
  activation: [14, 70],
  initialNode: [26, 26],
  activityFinal: [28, 28],
  flowFinal: [28, 28],
  action: [150, 52],
  decisionNode: [40, 40],
  forkNode: [120, 8],
  partition: [220, 360],
  objectNode: [130, 44],
  state: [160, 56],
  compositeState: [340, 220],
  stateInitial: [26, 26],
  stateFinal: [28, 28],
  choice: [36, 36],
  history: [30, 30],
  component: [180, 70],
  port: [14, 14],
  providedInterface: [20, 20],
  requiredInterface: [22, 22],
  deviceNode: [320, 200],
  artifact: [160, 56],
  object: [180, 0],
  startEvent: [36, 36],
  intermediateEvent: [36, 36],
  endEvent: [36, 36],
  task: [130, 64],
  subProcess: [150, 80],
  gateway: [50, 50],
  pool: [760, 300],
  dataObject: [40, 52],
  annotation: [150, 50],
  dataStore: [50, 44],
  group: [300, 180],
  service: [160, 64],
  database: [120, 80],
  queue: [150, 50],
  client: [130, 60],
  external: [160, 64],
  zone: [420, 260],
  c4Person: [170, 150],
  c4System: [220, 120],
  c4Container: [220, 120],
  c4Component: [200, 110],
  c4Code: [180, 90],
  c4Boundary: [520, 320],
  c4DeploymentNode: [420, 280],
  resource: [190, 56],
  resourceGroup: [420, 260],
  terminator: [140, 48],
  process: [150, 60],
  decision: [120, 80],
  io: [150, 56],
  document: [150, 66],
  predefinedProcess: [150, 60],
  manualInput: [150, 60],
  manualOperation: [150, 56],
  preparation: [160, 56],
  delay: [130, 56],
  merge: [60, 50],
  loopLimit: [150, 56],
  display: [150, 56],
  multiDocument: [150, 72],
  storedData: [150, 56],
  flowDatabase: [110, 76],
  internalStorage: [130, 70],
  sequentialStorage: [70, 70],
  directAccessStorage: [150, 60],
  connector: [36, 36],
  offPageConnector: [44, 48],
  flowAnnotation: [160, 50],
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
  if (type === 'object') Object.assign(base, { instanceOf: '', attributes: [] })
  if (type === 'state') Object.assign(base, { attributes: [] })
  if (type === 'history') Object.assign(base, { deep: false })
  if (type === 'component') Object.assign(base, { stereotype: 'component' })
  if (type === 'deviceNode') Object.assign(base, { stereotype: 'device' })
  if (type === 'artifact') Object.assign(base, { stereotype: 'artifact' })
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

const DATA_SIDE: ReadonlySet<NodeType> = new Set(['dataObject', 'annotation', 'dataStore'])
const USECASE_SIDE: ReadonlySet<NodeType> = new Set(['actor', 'usecase'])
const ARCH_NODES: ReadonlySet<NodeType> = new Set(['service', 'database', 'queue', 'client', 'external', 'resource', 'c4Person', 'c4System', 'c4Container', 'c4Component', 'c4Code'])
export const FLOW_NODES: ReadonlySet<NodeType> = new Set([
  'terminator',
  'process',
  'decision',
  'io',
  'document',
  'predefinedProcess',
  'manualInput',
  'manualOperation',
  'preparation',
  'delay',
  'merge',
  'loopLimit',
  'display',
  'multiDocument',
  'storedData',
  'flowDatabase',
  'internalStorage',
  'sequentialStorage',
  'directAccessStorage',
  'connector',
  'offPageConnector',
])

const PROVIDERS: ReadonlySet<NodeType> = new Set(['class', 'component', 'port'])
const DEPENDS: ReadonlySet<NodeType> = new Set(['class', 'interface', 'enum', 'package', 'component', 'deviceNode', 'artifact', 'providedInterface', 'requiredInterface'])

/** `true` se uma aresta deste tipo pode ligar estes dois elementos. */
export function canConnect(type: EdgeType, a: DNode, b: DNode): boolean {
  if (type === 'anchor') return (a.type === 'note') !== (b.type === 'note')
  switch (type) {
    case 'association':
      return (
        (CLASSIFIERS.has(a.type) && CLASSIFIERS.has(b.type)) ||
        (USECASE_SIDE.has(a.type) && USECASE_SIDE.has(b.type) && a.type !== b.type) ||
        (a.type === 'deviceNode' && b.type === 'deviceNode' && a.id !== b.id)
      )
    case 'link':
      return a.type === 'object' && b.type === 'object'
    case 'lostMessage':
    case 'foundMessage':
      return a.type === 'lifeline' && a.id === b.id
    case 'controlFlow':
      return ACTIVITY_NODES.has(a.type) && ACTIVITY_NODES.has(b.type) && a.id !== b.id && a.type !== 'activityFinal' && a.type !== 'flowFinal' && b.type !== 'initialNode'
    case 'transition':
      return STATE_NODES.has(a.type) && STATE_NODES.has(b.type) && a.type !== 'stateFinal' && b.type !== 'stateInitial' && (a.id !== b.id || a.type === 'state' || a.type === 'compositeState')
    case 'usage':
      return PROVIDERS.has(a.type) && (b.type === 'requiredInterface' || b.type === 'interface')
    case 'deploy':
      return a.type === 'artifact' && b.type === 'deviceNode'
    case 'manifest':
      return a.type === 'artifact' && (b.type === 'component' || CLASSIFIERS.has(b.type))
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
      return (CLASSIFIERS.has(a.type) && CLASSIFIERS.has(b.type)) || (PROVIDERS.has(a.type) && (b.type === 'providedInterface' || b.type === 'interface'))
    case 'dependency':
      return DEPENDS.has(a.type) && DEPENDS.has(b.type)
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
        (DATA_SIDE.has(a.type) && b.type !== 'pool' && b.type !== 'group' && a.id !== b.id) ||
        (DATA_SIDE.has(b.type) && a.type !== 'pool' && a.type !== 'group' && a.id !== b.id)
      )
    case 'sync':
    case 'async':
    case 'dataFlow':
      return ARCH_NODES.has(a.type) && ARCH_NODES.has(b.type)
    case 'c4Rel':
      return ARCH_NODES.has(a.type) && ARCH_NODES.has(b.type) && (C4_ELEMENTS.has(a.type) || C4_ELEMENTS.has(b.type))
    case 'flow':
      return FLOW_NODES.has(a.type) && FLOW_NODES.has(b.type)
    case 'flowNote':
      return (a.type === 'flowAnnotation') !== (b.type === 'flowAnnotation') && (FLOW_NODES.has(a.type) || FLOW_NODES.has(b.type))
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
    'controlFlow',
    'transition',
    'link',
    'realization',
    'usage',
    'deploy',
    'manifest',
    'sequenceFlow',
    'dataAssociation',
    'messageFlow',
    'c4Rel',
    'sync',
    'flow',
    'flowNote',
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
