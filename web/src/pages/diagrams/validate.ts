/**
 * Validação dos diagramas e correcções automáticas.
 *
 * Cada regra devolve um `Issue` com um código estável (a frase vive em
 * `diagrams.regras.<código>`) e os elementos envolvidos. Uma correcção só é
 * oferecida quando é DETERMINÍSTICA — a mesma entrada dá sempre o mesmo
 * modelo, e não há escolha escondida que a pessoa devesse fazer. Onde há
 * escolha (nomear uma classe, desfazer um ciclo de herança, decidir para onde
 * vai uma tarefa sem saída), a regra avisa e não mexe.
 */
import { center, contains, nodeBox, poolOf } from './geometry'
import {
  ACTIVITY_NODES,
  BPMN_FLOW_NODES,
  canConnect,
  CLASSIFIERS,
  DEdge,
  defaultEdgeType,
  DiagramDoc,
  DNode,
  EDGE_NOTATION,
  isValidMultiplicity,
  makeNode,
  NODE_NOTATION,
  normalizeMultiplicity,
  Notation,
  parseMember,
  STATE_NODES,
} from './model'

export type Severity = 'error' | 'warning'

export type RuleCode =
  | 'ligacaoExtremoEmFalta'
  | 'ligacaoInvalida'
  | 'umlNomeVazio'
  | 'umlNomeDuplicado'
  | 'umlAtributoSemTipo'
  | 'umlHerancaPropria'
  | 'umlHerancaCiclo'
  | 'umlHerancaParaInterface'
  | 'umlRealizacaoAlvo'
  | 'umlMultiplicidade'
  | 'umlArestaDuplicada'
  | 'umlActorIsolado'
  | 'umlMensagemSemNome'
  | 'umlActividadeSemInicio'
  | 'umlInicialComEntrada'
  | 'umlFinalComSaida'
  | 'umlDecisaoSaidas'
  | 'umlDecisaoGuarda'
  | 'umlEstadosSemInicial'
  | 'umlEscolhaSaidas'
  | 'umlEstadoInalcancavel'
  | 'bpmnSemInicio'
  | 'bpmnSemFim'
  | 'bpmnInicioComEntrada'
  | 'bpmnFimComSaida'
  | 'bpmnSemEntrada'
  | 'bpmnSemSaida'
  | 'bpmnGatewaySemOmissao'
  | 'bpmnCondicaoEmParalelo'
  | 'bpmnOmissaoComCondicao'
  | 'bpmnOmissaoOrigem'
  | 'bpmnVariasOmissao'
  | 'bpmnFluxoEntrePiscinas'
  | 'bpmnMensagemMesmaPiscina'
  | 'bpmnForaDaPiscina'
  | 'bpmnGatewayEventos'
  | 'bpmnTarefaSemNome'
  | 'archSemNome'
  | 'archIsolado'
  | 'flowSemInicio'
  | 'flowDecisaoSaidas'
  | 'flowDecisaoEtiquetas'
  | 'flowInalcancavel'

export interface Issue {
  id: string
  code: RuleCode
  severity: Severity
  /** Elementos e arestas envolvidos — o primeiro é o que se selecciona. */
  elements: string[]
  /** Valores para a frase (nomes, multiplicidades). */
  params: Record<string, string>
  fixable: boolean
}

function issue(code: RuleCode, severity: Severity, elements: string[], params: Record<string, string> = {}, fixable = false): Issue {
  return { id: `${code}:${elements.join(',')}`, code, severity, elements, params, fixable }
}

const label = (n: DNode | undefined) => (n ? n.name.trim() || n.props.text?.trim() || n.id : '')

/**
 * Valores da frase de um problema, com os ids de elementos sem nome trocados
 * pelo nome do TIPO («Decisão», «Nó inicial») — um id interno não diz nada a
 * quem lê.
 */
export function issueParams(doc: Pick<DiagramDoc, 'nodes'>, is: Issue, typeLabel: (n: DNode) => string): Record<string, string> {
  const byId = new Map(doc.nodes.map((n) => [n.id, n]))
  const out: Record<string, string> = {}
  for (const [k, v] of Object.entries(is.params)) {
    const n = byId.get(v)
    out[k] = n ? typeLabel(n) : v
  }
  return out
}

/** Todas as regras da notação pedida (por omissão, a do separador activo). */
export function validate(doc: DiagramDoc, notation: Notation = doc.notation): Issue[] {
  const byId = new Map(doc.nodes.map((n) => [n.id, n]))
  const out: Issue[] = []
  for (const e of doc.edges) {
    if (EDGE_NOTATION[e.type] !== notation) continue
    const a = byId.get(e.from)
    const b = byId.get(e.to)
    if (!a || !b) {
      out.push(issue('ligacaoExtremoEmFalta', 'error', [e.id], {}, true))
      continue
    }
    if (!canConnect(e.type, a, b)) out.push(issue('ligacaoInvalida', 'error', [e.id], { from: label(a), to: label(b) }, true))
  }
  if (notation === 'uml') out.push(...validateUml(doc, byId))
  if (notation === 'bpmn') out.push(...validateBpmn(doc, byId))
  if (notation === 'arch') out.push(...validateArch(doc))
  if (notation === 'flow') out.push(...validateFlow(doc, byId))
  return out
}

// ---------------------------------------------------------------------------
//  UML
// ---------------------------------------------------------------------------

function validateUml(doc: DiagramDoc, byId: Map<string, DNode>): Issue[] {
  const out: Issue[] = []
  const nodes = doc.nodes.filter((n) => NODE_NOTATION[n.type] === 'uml')
  const edges = doc.edges.filter((e) => EDGE_NOTATION[e.type] === 'uml' && byId.has(e.from) && byId.has(e.to))

  const named = new Set(['class', 'interface', 'enum', 'lifeline', 'actor', 'usecase', 'action', 'state', 'component', 'object', 'artifact', 'deviceNode'])
  for (const n of nodes) if (named.has(n.type) && !n.name.trim()) out.push(issue('umlNomeVazio', 'error', [n.id]))

  const seen = new Map<string, DNode>()
  for (const n of nodes) {
    if (!CLASSIFIERS.has(n.type) || !n.name.trim()) continue
    const key = `${(n.props.package ?? '').trim()}::${n.name.trim()}`
    const prev = seen.get(key)
    if (prev) out.push(issue('umlNomeDuplicado', 'error', [n.id, prev.id], { name: n.name.trim() }))
    else seen.set(key, n)
  }

  for (const n of nodes) {
    if (n.type !== 'class' && n.type !== 'interface') continue
    for (const raw of n.props.attributes ?? []) {
      const m = parseMember(raw)
      if (!m.isOperation && m.name && !m.type) out.push(issue('umlAtributoSemTipo', 'warning', [n.id], { element: label(n), member: m.name }))
    }
  }

  const gen = edges.filter((e) => e.type === 'generalization' || e.type === 'realization')
  for (const e of gen) {
    if (e.from === e.to) out.push(issue('umlHerancaPropria', 'error', [e.id], { name: label(byId.get(e.from)) }, true))
  }
  // Ciclo de herança: DFS no grafo filho → pai.
  const parents = new Map<string, string[]>()
  for (const e of edges) if (e.type === 'generalization' && e.from !== e.to) parents.set(e.from, [...(parents.get(e.from) ?? []), e.to])
  const reported = new Set<string>()
  for (const start of parents.keys()) {
    const stack: string[] = []
    const onStack = new Set<string>()
    const walk = (id: string): string[] | null => {
      if (onStack.has(id)) return stack.slice(stack.indexOf(id))
      stack.push(id)
      onStack.add(id)
      for (const p of parents.get(id) ?? []) {
        const c = walk(p)
        if (c) return c
      }
      stack.pop()
      onStack.delete(id)
      return null
    }
    const cycle = walk(start)
    if (cycle) {
      const key = [...cycle].sort().join(',')
      if (!reported.has(key)) {
        reported.add(key)
        out.push(issue('umlHerancaCiclo', 'error', cycle, { names: cycle.map((id) => label(byId.get(id))).join(' → ') }))
      }
    }
  }
  for (const e of edges) {
    const a = byId.get(e.from)!
    const b = byId.get(e.to)!
    if (e.type === 'generalization' && a.type === 'class' && b.type === 'interface') {
      out.push(issue('umlHerancaParaInterface', 'warning', [e.id], { from: label(a), to: label(b) }, true))
    }
    if (e.type === 'realization' && b.type !== 'interface' && canConnect('realization', a, b)) {
      out.push(issue('umlRealizacaoAlvo', 'error', [e.id], { from: label(a), to: label(b) }, a.type === b.type))
    }
    for (const side of ['srcMult', 'dstMult'] as const) {
      const m = e[side]
      if (m && !isValidMultiplicity(m)) {
        const norm = normalizeMultiplicity(m)
        out.push(issue('umlMultiplicidade', 'error', [e.id, side], { value: m }, norm !== null && isValidMultiplicity(norm)))
      }
    }
    if (e.type === 'message' && a.type === 'lifeline' && b.type === 'lifeline' && !e.label.trim()) out.push(issue('umlMensagemSemNome', 'warning', [e.id], { from: label(a), to: label(b) }))
  }
  const dup = new Map<string, DEdge>()
  for (const e of edges) {
    if (e.type === 'message' || e.type === 'reply' || e.type === 'lostMessage' || e.type === 'foundMessage') continue
    const key = `${e.type}|${e.from}|${e.to}`
    const first = dup.get(key)
    if (first) out.push(issue('umlArestaDuplicada', 'warning', [e.id, first.id], { from: label(byId.get(e.from)), to: label(byId.get(e.to)) }, true))
    else dup.set(key, e)
  }
  for (const n of nodes) {
    if (n.type !== 'actor') continue
    if (!edges.some((e) => e.from === n.id || e.to === n.id)) out.push(issue('umlActorIsolado', 'warning', [n.id], { name: label(n) }))
  }
  out.push(...validateBehaviour(nodes, edges))
  return out
}

/** Actividades e máquinas de estados: pontos de entrada, saídas e alcance. */
function validateBehaviour(nodes: DNode[], edges: DEdge[]): Issue[] {
  const out: Issue[] = []
  const flows = edges.filter((e) => e.type === 'controlFlow')
  const trans = edges.filter((e) => e.type === 'transition')
  const act = nodes.filter((n) => ACTIVITY_NODES.has(n.type))
  const initials = act.filter((n) => n.type === 'initialNode')
  if (act.some((n) => n.type === 'action') && initials.length === 0) out.push(issue('umlActividadeSemInicio', 'warning', [act.find((n) => n.type === 'action')!.id]))
  const states = nodes.filter((n) => STATE_NODES.has(n.type))
  const sInit = states.filter((n) => n.type === 'stateInitial')
  if (states.some((n) => n.type === 'state' || n.type === 'compositeState') && sInit.length === 0) {
    out.push(issue('umlEstadosSemInicial', 'warning', [states.find((n) => n.type === 'state' || n.type === 'compositeState')!.id]))
  }
  for (const [list, pool] of [[flows, act], [trans, states]] as const) {
    for (const n of pool) {
      const inc = list.filter((e) => e.to === n.id)
      const outs = list.filter((e) => e.from === n.id)
      if ((n.type === 'initialNode' || n.type === 'stateInitial') && inc.length > 0) out.push(issue('umlInicialComEntrada', 'error', [n.id], { name: label(n) }, true))
      if ((n.type === 'activityFinal' || n.type === 'flowFinal' || n.type === 'stateFinal') && outs.length > 0) out.push(issue('umlFinalComSaida', 'error', [n.id], { name: label(n) }, true))
      if (n.type === 'decisionNode') {
        // Junção (várias entradas, uma saída) é válida; decisão pede ≥ 2 saídas.
        const isMerge = inc.length >= 2 && outs.length === 1
        if (!isMerge && outs.length < 2) out.push(issue('umlDecisaoSaidas', 'warning', [n.id], { name: label(n) }))
        if (outs.length >= 2) for (const e of outs) if (!e.condition?.trim()) out.push(issue('umlDecisaoGuarda', 'warning', [e.id], { name: label(n) }))
      }
      if (n.type === 'choice' && outs.length < 2) out.push(issue('umlEscolhaSaidas', 'warning', [n.id], { name: label(n) }))
    }
  }
  if (sInit.length > 0) {
    const seen = new Set(sInit.map((n) => n.id))
    const queue = [...seen]
    while (queue.length) {
      const id = queue.shift()!
      for (const e of trans) if (e.from === id && !seen.has(e.to)) {
        seen.add(e.to)
        queue.push(e.to)
      }
    }
    // Um estado dentro de um composto alcançado conta como alcançado pelo composto.
    for (const n of states) {
      if (seen.has(n.id) || n.type === 'stateInitial' || n.type === 'compositeState') continue
      const inside = states.some((c) => c.type === 'compositeState' && seen.has(c.id) && contains(nodeBox(c), center(nodeBox(n))))
      if (!inside) out.push(issue('umlEstadoInalcancavel', 'warning', [n.id], { name: label(n) }))
    }
  }
  return out
}

// ---------------------------------------------------------------------------
//  BPMN
// ---------------------------------------------------------------------------

function validateBpmn(doc: DiagramDoc, byId: Map<string, DNode>): Issue[] {
  const out: Issue[] = []
  const flowNodes = doc.nodes.filter((n) => BPMN_FLOW_NODES.has(n.type))
  const seq = doc.edges.filter((e) => e.type === 'sequenceFlow' && byId.has(e.from) && byId.has(e.to))
  const incoming = (id: string) => seq.filter((e) => e.to === id)
  const outgoing = (id: string) => seq.filter((e) => e.from === id)
  const hasPools = doc.nodes.some((n) => n.type === 'pool')

  // Processos: um por piscina, e um para o que está fora de piscinas.
  const groups = new Map<string, DNode[]>()
  for (const n of flowNodes) {
    const key = poolOf(doc, n)?.id ?? ''
    groups.set(key, [...(groups.get(key) ?? []), n])
  }
  for (const [poolId, members] of groups) {
    const scope = poolId ? [poolId] : []
    if (!members.some((n) => n.type === 'startEvent')) {
      const candidates = members.filter((n) => n.type !== 'endEvent' && incoming(n.id).length === 0)
      out.push(issue('bpmnSemInicio', 'error', [...(candidates[0] ? [candidates[0].id] : []), ...scope], { pool: label(byId.get(poolId)) }, candidates.length === 1))
    }
    if (!members.some((n) => n.type === 'endEvent')) {
      const candidates = members.filter((n) => n.type !== 'startEvent' && outgoing(n.id).length === 0)
      out.push(issue('bpmnSemFim', 'error', [...(candidates[0] ? [candidates[0].id] : []), ...scope], { pool: label(byId.get(poolId)) }, candidates.length === 1))
    }
  }

  for (const n of flowNodes) {
    const inc = incoming(n.id)
    const outs = outgoing(n.id)
    if (n.type === 'startEvent' && inc.length > 0) out.push(issue('bpmnInicioComEntrada', 'error', [n.id], { name: label(n) }, true))
    if (n.type === 'endEvent' && outs.length > 0) out.push(issue('bpmnFimComSaida', 'error', [n.id], { name: label(n) }, true))
    if (n.type !== 'startEvent' && inc.length === 0) {
      out.push(issue('bpmnSemEntrada', 'warning', [n.id], { name: label(n) }))
    }
    if (n.type !== 'endEvent' && outs.length === 0) out.push(issue('bpmnSemSaida', 'warning', [n.id], { name: label(n) }))
    if ((n.type === 'task' || n.type === 'subProcess') && !n.name.trim()) out.push(issue('bpmnTarefaSemNome', 'warning', [n.id]))
    if (hasPools && !poolOf(doc, n)) out.push(issue('bpmnForaDaPiscina', 'warning', [n.id], { name: label(n) }))

    if (n.type === 'gateway') {
      const kind = n.props.gatewayKind ?? 'exclusive'
      if ((kind === 'exclusive' || kind === 'inclusive') && outs.length >= 2 && !outs.some((e) => e.isDefault)) {
        out.push(issue('bpmnGatewaySemOmissao', 'warning', [n.id], { name: label(n) }, outs.some((e) => !e.condition?.trim())))
      }
      if (kind === 'parallel') {
        for (const e of outs) if (e.condition?.trim() || e.isDefault) out.push(issue('bpmnCondicaoEmParalelo', 'error', [e.id], { name: label(n) }, true))
      }
      if (kind === 'eventBased') {
        for (const e of outs) {
          const target = byId.get(e.to)!
          const ok = target.type === 'intermediateEvent' || (target.type === 'task' && target.props.taskKind === 'receive')
          if (!ok) out.push(issue('bpmnGatewayEventos', 'error', [e.id], { name: label(n), target: label(target) }))
        }
      }
    }
    const defaults = outs.filter((e) => e.isDefault)
    if (defaults.length > 1) out.push(issue('bpmnVariasOmissao', 'error', [n.id, ...defaults.map((e) => e.id)], { name: label(n) }, true))
  }

  for (const e of seq) {
    const a = byId.get(e.from)!
    if (e.isDefault && e.condition?.trim()) out.push(issue('bpmnOmissaoComCondicao', 'error', [e.id], { name: label(a) }, true))
    const canDefault = (a.type === 'gateway' && (a.props.gatewayKind === 'exclusive' || a.props.gatewayKind === 'inclusive' || !a.props.gatewayKind)) || a.type === 'task' || a.type === 'subProcess'
    if (e.isDefault && !canDefault) out.push(issue('bpmnOmissaoOrigem', 'error', [e.id], { name: label(a) }, true))
    const pa = poolOf(doc, a)
    const pb = poolOf(doc, byId.get(e.to)!)
    if (pa && pb && pa.id !== pb.id) out.push(issue('bpmnFluxoEntrePiscinas', 'error', [e.id], { from: label(pa), to: label(pb) }, true))
  }
  for (const e of doc.edges) {
    if (e.type !== 'messageFlow') continue
    const a = byId.get(e.from)
    const b = byId.get(e.to)
    if (!a || !b) continue
    const pa = a.type === 'pool' ? a : poolOf(doc, a)
    const pb = b.type === 'pool' ? b : poolOf(doc, b)
    if (pa && pb && pa.id === pb.id) {
      out.push(issue('bpmnMensagemMesmaPiscina', 'error', [e.id], { pool: label(pa) }, BPMN_FLOW_NODES.has(a.type) && BPMN_FLOW_NODES.has(b.type)))
    }
  }
  return out
}

// ---------------------------------------------------------------------------
//  Arquitectura e fluxograma
// ---------------------------------------------------------------------------

function validateArch(doc: DiagramDoc): Issue[] {
  const out: Issue[] = []
  const comps = doc.nodes.filter((n) => NODE_NOTATION[n.type] === 'arch' && n.type !== 'zone')
  for (const n of comps) {
    if (!n.name.trim()) out.push(issue('archSemNome', 'warning', [n.id]))
    if (!doc.edges.some((e) => EDGE_NOTATION[e.type] === 'arch' && (e.from === n.id || e.to === n.id))) {
      out.push(issue('archIsolado', 'warning', [n.id], { name: label(n) }))
    }
  }
  return out
}

function validateFlow(doc: DiagramDoc, byId: Map<string, DNode>): Issue[] {
  const out: Issue[] = []
  const nodes = doc.nodes.filter((n) => NODE_NOTATION[n.type] === 'flow')
  if (nodes.length === 0) return out
  const edges = doc.edges.filter((e) => e.type === 'flow' && byId.has(e.from) && byId.has(e.to))
  const starts = nodes.filter((n) => n.type === 'terminator' && !edges.some((e) => e.to === n.id))
  if (starts.length === 0) out.push(issue('flowSemInicio', 'error', nodes[0] ? [nodes[0].id] : []))
  for (const n of nodes) {
    if (n.type !== 'decision') continue
    const outs = edges.filter((e) => e.from === n.id)
    if (outs.length < 2) out.push(issue('flowDecisaoSaidas', 'error', [n.id], { name: label(n) }))
    for (const e of outs) if (!e.label.trim()) out.push(issue('flowDecisaoEtiquetas', 'warning', [e.id], { name: label(n) }))
  }
  if (starts.length > 0) {
    const seen = new Set(starts.map((n) => n.id))
    const queue = [...seen]
    while (queue.length) {
      const id = queue.shift()!
      for (const e of edges) if (e.from === id && !seen.has(e.to)) {
        seen.add(e.to)
        queue.push(e.to)
      }
    }
    for (const n of nodes) if (!seen.has(n.id)) out.push(issue('flowInalcancavel', 'warning', [n.id], { name: label(n) }))
  }
  return out
}

// ---------------------------------------------------------------------------
//  Correcções
// ---------------------------------------------------------------------------

const removeEdges = (doc: DiagramDoc, ids: Set<string>): DiagramDoc => ({ ...doc, edges: doc.edges.filter((e) => !ids.has(e.id)) })
const patchEdge = (doc: DiagramDoc, id: string, patch: Partial<DEdge>): DiagramDoc => ({
  ...doc,
  edges: doc.edges.map((e) => (e.id === id ? { ...e, ...patch } : e)),
})

/**
 * Aplica a correcção de um problema. Um problema sem correcção devolve o
 * mesmo documento (a mesma referência), para quem chama saber que nada mudou.
 */
export function applyFix(doc: DiagramDoc, is: Issue, names: { start: string; end: string } = { start: '', end: '' }): DiagramDoc {
  if (!is.fixable) return doc
  const [first] = is.elements
  const byId = new Map(doc.nodes.map((n) => [n.id, n]))
  const edge = doc.edges.find((e) => e.id === first)
  switch (is.code) {
    case 'ligacaoExtremoEmFalta':
    case 'umlHerancaPropria':
      return removeEdges(doc, new Set([first]))
    case 'ligacaoInvalida': {
      if (!edge) return doc
      const a = byId.get(edge.from)
      const b = byId.get(edge.to)
      const t = a && b ? defaultEdgeType(a, b) : null
      return t && EDGE_NOTATION[t] === EDGE_NOTATION[edge.type] ? patchEdge(doc, edge.id, { type: t }) : removeEdges(doc, new Set([edge.id]))
    }
    case 'umlHerancaParaInterface':
      return patchEdge(doc, first, { type: 'realization' })
    case 'umlRealizacaoAlvo':
      return patchEdge(doc, first, { type: 'generalization' })
    case 'umlMultiplicidade': {
      if (!edge) return doc
      const side = is.elements[1] as 'srcMult' | 'dstMult'
      const norm = normalizeMultiplicity(edge[side] ?? '')
      return norm === null ? doc : patchEdge(doc, edge.id, { [side]: norm })
    }
    case 'umlArestaDuplicada':
      return removeEdges(doc, new Set([first]))
    case 'umlInicialComEntrada':
      return { ...doc, edges: doc.edges.filter((e) => !((e.type === 'controlFlow' || e.type === 'transition') && e.to === first)) }
    case 'umlFinalComSaida':
      return { ...doc, edges: doc.edges.filter((e) => !((e.type === 'controlFlow' || e.type === 'transition') && e.from === first)) }
    case 'bpmnInicioComEntrada':
      return { ...doc, edges: doc.edges.filter((e) => !(e.type === 'sequenceFlow' && e.to === first)) }
    case 'bpmnFimComSaida':
      return { ...doc, edges: doc.edges.filter((e) => !(e.type === 'sequenceFlow' && e.from === first)) }
    case 'bpmnSemInicio':
    case 'bpmnSemFim': {
      const target = byId.get(first)
      if (!target) return doc
      const b = nodeBox(target)
      const c = center(b)
      const isStart = is.code === 'bpmnSemInicio'
      const id = `${isStart ? 'start' : 'end'}_${target.id}`
      if (byId.has(id)) return doc
      const ev = makeNode(isStart ? 'startEvent' : 'endEvent', isStart ? b.x - 76 : b.x + b.w + 40, c.y - 18, isStart ? names.start : names.end, { trigger: 'none' }, id)
      const flow: DEdge = isStart
        ? { id: `f_${id}`, type: 'sequenceFlow', from: id, to: target.id, label: '' }
        : { id: `f_${id}`, type: 'sequenceFlow', from: target.id, to: id, label: '' }
      return { ...doc, nodes: [...doc.nodes, ev], edges: [...doc.edges, flow] }
    }
    case 'bpmnGatewaySemOmissao': {
      const pick = doc.edges.find((e) => e.type === 'sequenceFlow' && e.from === first && !e.condition?.trim())
      return pick ? patchEdge(doc, pick.id, { isDefault: true }) : doc
    }
    case 'bpmnCondicaoEmParalelo':
      return patchEdge(doc, first, { condition: '', isDefault: false })
    case 'bpmnOmissaoComCondicao':
      return patchEdge(doc, first, { condition: '' })
    case 'bpmnOmissaoOrigem':
      return patchEdge(doc, first, { isDefault: false })
    case 'bpmnVariasOmissao': {
      // elements = [origem, primeira por omissão (fica), ...as outras]
      const rest = is.elements.slice(2)
      const drop = new Set(rest)
      return { ...doc, edges: doc.edges.map((e) => (drop.has(e.id) ? { ...e, isDefault: false } : e)) }
    }
    case 'bpmnFluxoEntrePiscinas':
      return patchEdge(doc, first, { type: 'messageFlow', condition: '', isDefault: false })
    case 'bpmnMensagemMesmaPiscina':
      return patchEdge(doc, first, { type: 'sequenceFlow' })
    default:
      return doc
  }
}

/**
 * «Corrigir automaticamente»: aplica as correcções até não sobrar nenhuma
 * aplicável (com tecto — uma correcção pode revelar outra, nunca um ciclo).
 */
export function fixAll(doc: DiagramDoc, names?: { start: string; end: string }, notation: Notation = doc.notation): { doc: DiagramDoc; fixed: number } {
  let cur = doc
  let fixed = 0
  for (let round = 0; round < 20; round++) {
    const next = validate(cur, notation).find((i) => i.fixable && applyFix(cur, i, names) !== cur)
    if (!next) break
    cur = applyFix(cur, next, names)
    fixed++
  }
  return { doc: cur, fixed }
}
