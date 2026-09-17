/**
 * Saídas do quadro: XMI 2.5.1 e PlantUML (UML), BPMN 2.0 XML com diagrama
 * (DI), SVG e o modelo em JSON (para voltar a abrir noutro browser).
 *
 * Tudo string → string: o browser só entra para descarregar o ficheiro. Os
 * nomes que a pessoa escreveu vão escapados; os identificadores XML são os do
 * modelo com um prefixo, porque um `xmi:id`/`id` BPMN tem de ser NCName.
 *
 * Grupos de selecção (⌘G, v5) — decisão: um grupo NÃO muda a semântica do
 * modelo, por isso nunca sai como pacote (isso mudava o nome qualificado das
 * classes). Sai assim:
 *  - BPMN: `bpmn:group` com `bpmn:category` — é o artefacto que a norma tem
 *    exactamente para isto, sem efeito no fluxo — e forma no diagrama (DI);
 *  - XMI: `xmi:Extension extender="Delonix Meet"` com os membros por
 *    `xmi:idref` — o mecanismo da norma para dados de uma ferramenta; outras
 *    ferramentas ignoram-no e o modelo UML fica igual;
 *  - PlantUML: `together { … }` para as classes soltas do mesmo grupo (só
 *    aproxima no desenho); membros dentro de um `package` ficam onde estão;
 *  - SVG/PNG: a moldura tracejada com o nome, tal como no ecrã.
 */
import { attachedActivity, center, contentBox, edgeSegment, laneOf, nodeBox, poolHeight, poolOf } from './geometry'
import { cleanGroups, groupsOf, pickBox } from './groups'
import {
  ACTIVITY_NODES,
  BPMN_FLOW_NODES,
  CLASSIFIERS,
  DEdge,
  DiagramDoc,
  DNode,
  EDGE_NOTATION,
  NODE_NOTATION,
  parseMember,
  STATE_NODES,
} from './model'
import { FONT, INK } from './paint'

export function escapeXml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&apos;')
    // Caracteres de controlo não são XML 1.0 válido.
    // eslint-disable-next-line no-control-regex
    .replace(/[\u0000-\u0008\u000b\u000c\u000e-\u001f]/g, '')
}

/** Identificador NCName estável a partir do id do modelo. */
export function xmlId(prefix: string, id: string): string {
  return `${prefix}_${id.replace(/[^A-Za-z0-9_.-]/g, '_')}`
}

/** Nome de ficheiro seguro a partir do título. */
export function fileBase(title: string): string {
  const s = title
    .normalize('NFD')
    .replace(/[\u0300-\u036f]/g, '')
    .replace(/[^A-Za-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .toLowerCase()
  return s.slice(0, 60) || 'diagrama'
}

/** Margem da moldura de um grupo de selecção à volta dos membros (ecrã, SVG e DI do BPMN). */
export const GROUP_PAD = 12

const umlNodes = (doc: DiagramDoc) => doc.nodes.filter((n) => NODE_NOTATION[n.type] === 'uml')
const umlEdges = (doc: DiagramDoc) => {
  const ids = new Set(doc.nodes.map((n) => n.id))
  return doc.edges.filter((e) => EDGE_NOTATION[e.type] === 'uml' && ids.has(e.from) && ids.has(e.to))
}

const VIS: Record<string, string> = { '+': 'public', '-': 'private', '#': 'protected', '~': 'package', '': 'public' }

// ---------------------------------------------------------------------------
//  XMI 2.5.1
// ---------------------------------------------------------------------------

export function toXmi(doc: DiagramDoc): string {
  const nodes = umlNodes(doc)
  const edges = umlEdges(doc)
  const byId = new Map(doc.nodes.map((n) => [n.id, n]))
  const classifierByName = new Map<string, DNode>()
  for (const n of nodes) if (CLASSIFIERS.has(n.type) && n.name.trim()) classifierByName.set(n.name.trim(), n)

  const dataTypes = new Map<string, string>()
  const typeRef = (name: string): string => {
    const t = name.trim()
    if (!t) return ''
    const cls = classifierByName.get(t)
    if (cls) return xmlId('c', cls.id)
    if (!dataTypes.has(t)) dataTypes.set(t, `dt_${dataTypes.size + 1}`)
    return dataTypes.get(t)!
  }

  const mult = (s: string | undefined, owner: string): string => {
    const m = (s ?? '').trim()
    if (!m) return ''
    const [lo, hi] = m.includes('..') ? m.split('..') : [m === '*' ? '0' : m, m]
    const lower = `<lowerValue xmi:type="uml:LiteralInteger" xmi:id="${owner}_lo" value="${escapeXml(lo === '*' ? '0' : lo)}"/>`
    const upper = `<upperValue xmi:type="uml:LiteralUnlimitedNatural" xmi:id="${owner}_hi" value="${escapeXml(hi)}"/>`
    return lower + upper
  }

  const classifierXml = (n: DNode, ind: string): string => {
    const id = xmlId('c', n.id)
    const kind = n.type === 'interface' ? 'uml:Interface' : n.type === 'enum' ? 'uml:Enumeration' : 'uml:Class'
    const lines: string[] = [`${ind}<packagedElement xmi:type="${kind}" xmi:id="${id}" name="${escapeXml(n.name.trim())}">`]
    if (n.props.stereotype && n.type === 'class') {
      lines.push(`${ind}  <ownedComment xmi:type="uml:Comment" xmi:id="${id}_st" body="${escapeXml(`«${n.props.stereotype}»`)}"><annotatedElement xmi:idref="${id}"/></ownedComment>`)
    }
    ;(n.props.attributes ?? []).forEach((raw, i) => {
      const m = parseMember(raw)
      if (!m.name) return
      if (n.type === 'enum') {
        lines.push(`${ind}  <ownedLiteral xmi:type="uml:EnumerationLiteral" xmi:id="${id}_l${i}" name="${escapeXml(m.name)}"/>`)
        return
      }
      const tr = typeRef(m.type)
      lines.push(
        `${ind}  <ownedAttribute xmi:type="uml:Property" xmi:id="${id}_a${i}" name="${escapeXml(m.name)}" visibility="${VIS[m.visibility]}"${tr ? ` type="${tr}"` : ''}/>`,
      )
    })
    ;(n.props.operations ?? []).forEach((raw, i) => {
      const m = parseMember(raw)
      if (!m.name) return
      const tr = typeRef(m.type)
      const params = m.params
        .split(',')
        .map((p) => p.trim())
        .filter(Boolean)
        .map((p, j) => {
          const [pn, pt] = p.split(':').map((x) => x.trim())
          const ptr = pt ? typeRef(pt) : ''
          return `<ownedParameter xmi:type="uml:Parameter" xmi:id="${id}_o${i}_p${j}" name="${escapeXml(pn)}" direction="in"${ptr ? ` type="${ptr}"` : ''}/>`
        })
      const ret = tr ? `<ownedParameter xmi:type="uml:Parameter" xmi:id="${id}_o${i}_r" direction="return" type="${tr}"/>` : ''
      lines.push(`${ind}  <ownedOperation xmi:type="uml:Operation" xmi:id="${id}_o${i}" name="${escapeXml(m.name)}" visibility="${VIS[m.visibility]}">${params.join('')}${ret}</ownedOperation>`)
    })
    for (const e of edges) {
      if (e.from !== n.id) continue
      if (e.type === 'generalization') lines.push(`${ind}  <generalization xmi:type="uml:Generalization" xmi:id="${xmlId('g', e.id)}" general="${xmlId('c', e.to)}"/>`)
      if (e.type === 'realization') {
        const s = xmlId('r', e.id)
        lines.push(`${ind}  <interfaceRealization xmi:type="uml:InterfaceRealization" xmi:id="${s}" client="${id}" supplier="${xmlId('c', e.to)}" contract="${xmlId('c', e.to)}"/>`)
      }
    }
    lines.push(`${ind}</packagedElement>`)
    return lines.join('\n')
  }

  const out: string[] = []
  out.push('<?xml version="1.0" encoding="UTF-8"?>')
  out.push('<xmi:XMI xmi:version="20131001" xmlns:xmi="http://www.omg.org/spec/XMI/20131001" xmlns:uml="http://www.omg.org/spec/UML/20161101">')
  out.push(`  <uml:Model xmi:type="uml:Model" xmi:id="${xmlId('m', doc.id)}" name="${escapeXml(doc.title)}">`)

  // Pacotes: os nomeados no inspector e os desenhados.
  const packages = new Map<string, { nodeId?: string; members: DNode[] }>()
  for (const n of nodes) {
    if (n.type === 'package' && n.name.trim() && !packages.has(n.name.trim())) packages.set(n.name.trim(), { nodeId: n.id, members: [] })
  }
  const loose: DNode[] = []
  for (const n of nodes) {
    if (!CLASSIFIERS.has(n.type)) continue
    const p = n.props.package?.trim()
    if (!p) {
      loose.push(n)
      continue
    }
    const entry = packages.get(p) ?? { members: [] }
    entry.members.push(n)
    packages.set(p, entry)
  }
  let pi = 0
  for (const [name, { nodeId, members }] of packages) {
    pi++
    const pid = nodeId ? xmlId('pk', nodeId) : xmlId('p', `${doc.id}_${pi}`)
    out.push(`    <packagedElement xmi:type="uml:Package" xmi:id="${pid}" name="${escapeXml(name)}">`)
    for (const m of members) out.push(classifierXml(m, '      '))
    out.push('    </packagedElement>')
  }
  // Pacotes desenhados sem nome também existem no modelo.
  for (const n of nodes) {
    if (n.type === 'package' && !n.name.trim()) out.push(`    <packagedElement xmi:type="uml:Package" xmi:id="${xmlId('pk', n.id)}"/>`)
  }
  for (const m of loose) out.push(classifierXml(m, '    '))

  // Associações (incl. agregação e composição): extremos pertencentes à associação.
  for (const e of edges) {
    if (e.type !== 'association' && e.type !== 'aggregation' && e.type !== 'composition') continue
    const a = byId.get(e.from)!
    const b = byId.get(e.to)!
    if (!refOf(a) || !refOf(b) || a.type === 'deviceNode' || b.type === 'deviceNode') continue
    const id = xmlId('as', e.id)
    const agg = e.type === 'composition' ? ' aggregation="composite"' : e.type === 'aggregation' ? ' aggregation="shared"' : ''
    const typeA = refOf(a)
    const typeB = refOf(b)
    out.push(`    <packagedElement xmi:type="uml:Association" xmi:id="${id}"${e.label.trim() ? ` name="${escapeXml(e.label.trim())}"` : ''} memberEnd="${id}_src ${id}_dst">`)
    // O losango fica do lado do TODO (origem); `aggregation` marca-se no extremo tipado pela PARTE.
    out.push(`      <ownedEnd xmi:type="uml:Property" xmi:id="${id}_src" type="${typeA}" association="${id}">${mult(e.srcMult, `${id}_src`)}</ownedEnd>`)
    out.push(`      <ownedEnd xmi:type="uml:Property" xmi:id="${id}_dst" type="${typeB}" association="${id}"${agg}>${mult(e.dstMult, `${id}_dst`)}</ownedEnd>`)
    out.push('    </packagedElement>')
  }
  for (const e of edges) {
    if (e.type !== 'dependency' && e.type !== 'usage') continue
    const kind = e.type === 'usage' ? 'uml:Usage' : 'uml:Dependency'
    out.push(`    <packagedElement xmi:type="${kind}" xmi:id="${xmlId('d', e.id)}" client="${refOf(byId.get(e.from)!)}" supplier="${refOf(byId.get(e.to)!)}"/>`)
  }

  out.push(...structureXmi(doc, nodes, edges, byId))
  out.push(...activityXmi(doc, nodes, edges))
  out.push(...stateMachineXmi(doc, nodes, edges))

  // Casos de uso.
  for (const n of nodes) {
    if (n.type === 'actor') out.push(`    <packagedElement xmi:type="uml:Actor" xmi:id="${xmlId('u', n.id)}" name="${escapeXml(n.name.trim())}"/>`)
    if (n.type === 'boundary') {
      out.push(`    <packagedElement xmi:type="uml:Component" xmi:id="${xmlId('u', n.id)}" name="${escapeXml(n.name.trim())}"/>`)
    }
  }
  for (const n of nodes) {
    if (n.type !== 'usecase') continue
    const subject = nodes.find((b) => b.type === 'boundary' && insideBox(n, b))
    const inner: string[] = []
    for (const e of edges) {
      if (e.from !== n.id) continue
      if (e.type === 'include') inner.push(`<include xmi:type="uml:Include" xmi:id="${xmlId('i', e.id)}" addition="${xmlId('u', e.to)}"/>`)
      if (e.type === 'extend') inner.push(`<extend xmi:type="uml:Extend" xmi:id="${xmlId('x', e.id)}" extendedCase="${xmlId('u', e.to)}"/>`)
    }
    out.push(
      `    <packagedElement xmi:type="uml:UseCase" xmi:id="${xmlId('u', n.id)}" name="${escapeXml(n.name.trim())}"${subject ? ` subject="${xmlId('u', subject.id)}"` : ''}>${inner.join('')}</packagedElement>`,
    )
  }

  // Sequência: uma interacção com as linhas de vida e as mensagens por ordem vertical.
  const lifelines = nodes.filter((n) => n.type === 'lifeline')
  if (lifelines.length > 0) {
    const iid = xmlId('int', doc.id)
    out.push(`    <packagedElement xmi:type="uml:Interaction" xmi:id="${iid}" name="${escapeXml(doc.title)}">`)
    for (const l of lifelines) out.push(`      <lifeline xmi:type="uml:Lifeline" xmi:id="${xmlId('ll', l.id)}" name="${escapeXml(l.name.trim())}"/>`)
    const msgs = sequenceMessages(doc)
    for (const e of msgs) {
      const m = xmlId('msg', e.id)
      if (e.type !== 'foundMessage') out.push(`      <fragment xmi:type="uml:MessageOccurrenceSpecification" xmi:id="${m}_s" covered="${xmlId('ll', e.from)}" message="${m}"/>`)
      if (e.type !== 'lostMessage') out.push(`      <fragment xmi:type="uml:MessageOccurrenceSpecification" xmi:id="${m}_r" covered="${xmlId('ll', e.to)}" message="${m}"/>`)
    }
    for (const a of nodes.filter((n) => n.type === 'activation')) {
      const line = lifelines.find((l) => overlapsX(a, l))
      if (!line) continue
      const id = xmlId('ex', a.id)
      const ll = xmlId('ll', line.id)
      out.push(`      <fragment xmi:type="uml:ExecutionOccurrenceSpecification" xmi:id="${id}_start" covered="${ll}" execution="${id}"/>`)
      out.push(`      <fragment xmi:type="uml:BehaviorExecutionSpecification" xmi:id="${id}" covered="${ll}" start="${id}_start" finish="${id}_finish"/>`)
      out.push(`      <fragment xmi:type="uml:ExecutionOccurrenceSpecification" xmi:id="${id}_finish" covered="${ll}" execution="${id}"/>`)
    }
    for (const f of nodes.filter((n) => n.type === 'fragment')) {
      const covered = lifelines.filter((l) => overlapsX(l, f)).map((l) => xmlId('ll', l.id))
      out.push(
        `      <fragment xmi:type="uml:CombinedFragment" xmi:id="${xmlId('cf', f.id)}" interactionOperator="${f.props.operator ?? 'alt'}"${covered.length ? ` covered="${covered.join(' ')}"` : ''}><operand xmi:type="uml:InteractionOperand" xmi:id="${xmlId('op', f.id)}"><guard xmi:type="uml:InteractionConstraint" xmi:id="${xmlId('gd', f.id)}"><specification xmi:type="uml:LiteralString" xmi:id="${xmlId('sp', f.id)}" value="${escapeXml(f.name.trim())}"/></guard></operand></fragment>`,
      )
    }
    for (const e of msgs) {
      const m = xmlId('msg', e.id)
      const sort = e.type === 'reply' ? 'reply' : e.type === 'message' ? 'synchCall' : 'asynchSignal'
      const kind = e.type === 'lostMessage' ? ' messageKind="lost"' : e.type === 'foundMessage' ? ' messageKind="found"' : ''
      const ends = `${e.type !== 'foundMessage' ? ` sendEvent="${m}_s"` : ''}${e.type !== 'lostMessage' ? ` receiveEvent="${m}_r"` : ''}`
      out.push(`      <message xmi:type="uml:Message" xmi:id="${m}" name="${escapeXml(e.label.trim())}" messageSort="${sort}"${kind}${ends}/>`)
    }
    out.push('    </packagedElement>')
  }

  // Notas: comentários ligados pelas âncoras.
  for (const n of nodes) {
    if (n.type !== 'note') continue
    const annotated = edges
      .filter((e) => e.type === 'anchor' && (e.from === n.id || e.to === n.id))
      .map((e) => (e.from === n.id ? e.to : e.from))
      .map((id) => refOf(byId.get(id)!))
      .filter(Boolean)
    out.push(
      `    <ownedComment xmi:type="uml:Comment" xmi:id="${xmlId('nt', n.id)}" body="${escapeXml((n.props.text ?? n.name).trim())}">${annotated.map((a) => `<annotatedElement xmi:idref="${a}"/>`).join('')}</ownedComment>`,
    )
  }

  for (const [name, id] of dataTypes) out.push(`    <packagedElement xmi:type="uml:DataType" xmi:id="${id}" name="${escapeXml(name)}"/>`)
  out.push('  </uml:Model>')
  // Grupos de selecção: dados da ferramenta, fora do modelo (ver o cabeçalho).
  const umlIds = new Set(nodes.map((n) => n.id))
  const xmiGroups = groupsOf(doc)
    .map((g) => ({ g, refs: g.nodes.filter((id) => umlIds.has(id)).map((id) => (byId.get(id)!.type === 'note' ? xmlId('nt', id) : refOf(byId.get(id)!))).filter(Boolean) }))
    .filter((x) => x.refs.length > 0)
  if (xmiGroups.length > 0) {
    out.push('  <xmi:Extension extender="Delonix Meet">')
    for (const { g, refs } of xmiGroups) {
      out.push(`    <group xmi:id="${xmlId('grp', g.id)}" name="${escapeXml(g.name.trim())}">${refs.map((r) => `<member xmi:idref="${r}"/>`).join('')}</group>`)
    }
    out.push('  </xmi:Extension>')
  }
  out.push('</xmi:XMI>')
  return out.join('\n') + '\n'
}

/** O `xmi:id` com que cada tipo de elemento sai (vazio = não sai como elemento). */
function refOf(n: DNode): string {
  if (CLASSIFIERS.has(n.type)) return xmlId('c', n.id)
  if (n.type === 'package') return xmlId('pk', n.id)
  if (n.type === 'actor' || n.type === 'usecase' || n.type === 'boundary') return xmlId('u', n.id)
  if (n.type === 'lifeline') return xmlId('ll', n.id)
  if (n.type === 'component' || n.type === 'providedInterface' || n.type === 'requiredInterface' || n.type === 'deviceNode' || n.type === 'artifact' || n.type === 'port') return xmlId('k', n.id)
  if (n.type === 'object') return xmlId('o', n.id)
  if (ACTIVITY_NODES.has(n.type)) return xmlId('an', n.id)
  if (STATE_NODES.has(n.type)) return xmlId('sv', n.id)
  return ''
}

/** Componentes, interfaces fornecidas/requeridas, portos, nós, artefactos e objectos. */
function structureXmi(doc: DiagramDoc, nodes: DNode[], edges: DEdge[], byId: Map<string, DNode>): string[] {
  const out: string[] = []
  const name = (n: DNode) => escapeXml(n.name.trim())
  const comps = nodes.filter((n) => n.type === 'component')
  const ports = nodes.filter((n) => n.type === 'port')
  const portOwner = (p: DNode) => comps.find((c) => {
    const b = nodeBox(c)
    const q = center(nodeBox(p))
    return q.x >= b.x - 10 && q.x <= b.x + b.w + 10 && q.y >= b.y - 10 && q.y <= b.y + b.h + 10
  })
  for (const n of nodes.filter((x) => x.type === 'providedInterface' || x.type === 'requiredInterface')) {
    out.push(`    <packagedElement xmi:type="uml:Interface" xmi:id="${xmlId('k', n.id)}" name="${name(n)}"/>`)
  }
  for (const c of comps) {
    const id = xmlId('k', c.id)
    const inner: string[] = []
    for (const p of ports.filter((x) => portOwner(x)?.id === c.id)) inner.push(`<ownedAttribute xmi:type="uml:Port" xmi:id="${xmlId('k', p.id)}" name="${name(p)}"/>`)
    for (const e of edges) {
      if (e.type !== 'realization') continue
      const src = byId.get(e.from)!
      const owner = src.type === 'port' ? portOwner(src) : src
      if (owner?.id !== c.id) continue
      const target = byId.get(e.to)!
      const ref = target.type === 'interface' ? xmlId('c', target.id) : xmlId('k', target.id)
      inner.push(`<interfaceRealization xmi:type="uml:InterfaceRealization" xmi:id="${xmlId('r', e.id)}" client="${id}" supplier="${ref}" contract="${ref}"/>`)
    }
    out.push(`    <packagedElement xmi:type="uml:Component" xmi:id="${id}" name="${name(c)}">${inner.join('')}</packagedElement>`)
  }
  // Um porto fora de qualquer componente não tem dono no metamodelo: fica só no desenho.
  for (const a of nodes.filter((x) => x.type === 'artifact')) {
    const id = xmlId('k', a.id)
    const inner = edges
      .filter((e) => e.type === 'manifest' && e.from === a.id)
      .map((e) => {
        const target = byId.get(e.to)!
        return `<manifestation xmi:type="uml:Manifestation" xmi:id="${xmlId('mf', e.id)}" client="${id}" supplier="${refOf(target)}" utilizedElement="${refOf(target)}"/>`
      })
    out.push(`    <packagedElement xmi:type="uml:Artifact" xmi:id="${id}" name="${name(a)}" fileName="${name(a)}">${inner.join('')}</packagedElement>`)
  }
  for (const d of nodes.filter((x) => x.type === 'deviceNode')) {
    const id = xmlId('k', d.id)
    const inner = edges
      .filter((e) => e.type === 'deploy' && e.to === d.id)
      .map((e) => `<deployment xmi:type="uml:Deployment" xmi:id="${xmlId('dp', e.id)}" client="${id}" supplier="${xmlId('k', e.from)}" deployedArtifact="${xmlId('k', e.from)}"/>`)
    const kind = d.props.stereotype === 'executionEnvironment' ? 'uml:ExecutionEnvironment' : d.props.stereotype === 'device' ? 'uml:Device' : 'uml:Node'
    out.push(`    <packagedElement xmi:type="${kind}" xmi:id="${id}" name="${name(d)}">${inner.join('')}</packagedElement>`)
  }
  for (const e of edges) {
    if (e.type !== 'association') continue
    const a = byId.get(e.from)!
    const b = byId.get(e.to)!
    if (a.type !== 'deviceNode' || b.type !== 'deviceNode') continue
    const id = xmlId('cp', e.id)
    out.push(`    <packagedElement xmi:type="uml:CommunicationPath" xmi:id="${id}"${e.label.trim() ? ` name="${escapeXml(e.label.trim())}"` : ''} memberEnd="${id}_a ${id}_b"><ownedEnd xmi:type="uml:Property" xmi:id="${id}_a" type="${xmlId('k', a.id)}" association="${id}"/><ownedEnd xmi:type="uml:Property" xmi:id="${id}_b" type="${xmlId('k', b.id)}" association="${id}"/></packagedElement>`)
  }
  const classByName = new Map(nodes.filter((n) => CLASSIFIERS.has(n.type) && n.name.trim()).map((n) => [n.name.trim(), n]))
  for (const o of nodes.filter((x) => x.type === 'object')) {
    const id = xmlId('o', o.id)
    const cls = classByName.get((o.props.instanceOf ?? '').trim())
    const slots = (o.props.attributes ?? []).map((raw, i) => {
      const [k, ...v] = raw.split('=')
      const feature = cls ? (cls.props.attributes ?? []).findIndex((a) => parseMember(a).name === k.trim()) : -1
      const def = cls && feature >= 0 ? ` definingFeature="${xmlId('c', cls.id)}_a${feature}"` : ''
      return `<slot xmi:type="uml:Slot" xmi:id="${id}_s${i}"${def}><value xmi:type="uml:LiteralString" xmi:id="${id}_s${i}_v" name="${escapeXml(k.trim())}" value="${escapeXml(v.join('=').trim())}"/></slot>`
    })
    out.push(`    <packagedElement xmi:type="uml:InstanceSpecification" xmi:id="${id}" name="${name(o)}"${cls ? ` classifier="${xmlId('c', cls.id)}"` : ''}>${slots.join('')}</packagedElement>`)
  }
  void doc
  return out
}

/** Diagramas de actividade: uma `uml:Activity` com nós, fluxos e partições. */
function activityXmi(doc: DiagramDoc, nodes: DNode[], edges: DEdge[]): string[] {
  const act = nodes.filter((n) => ACTIVITY_NODES.has(n.type))
  if (act.length === 0) return []
  const flows = edges.filter((e) => e.type === 'controlFlow')
  const parts = nodes.filter((n) => n.type === 'partition')
  const partOf = (n: DNode) => parts.find((p) => insideBox(n, p))
  const kind = (n: DNode): string => {
    const outs = flows.filter((e) => e.from === n.id).length
    const ins = flows.filter((e) => e.to === n.id).length
    switch (n.type) {
      case 'initialNode':
        return 'uml:InitialNode'
      case 'activityFinal':
        return 'uml:ActivityFinalNode'
      case 'flowFinal':
        return 'uml:FlowFinalNode'
      case 'decisionNode':
        return ins >= 2 && outs <= 1 ? 'uml:MergeNode' : 'uml:DecisionNode'
      case 'forkNode':
        return ins >= 2 && outs <= 1 ? 'uml:JoinNode' : 'uml:ForkNode'
      case 'objectNode':
        return 'uml:CentralBufferNode'
      default:
        return 'uml:OpaqueAction'
    }
  }
  const out: string[] = [`    <packagedElement xmi:type="uml:Activity" xmi:id="${xmlId('act', doc.id)}" name="${escapeXml(doc.title)}">`]
  for (const p of parts) {
    const members = act.filter((n) => partOf(n)?.id === p.id).map((n) => xmlId('an', n.id))
    out.push(`      <group xmi:type="uml:ActivityPartition" xmi:id="${xmlId('ap', p.id)}" name="${escapeXml(p.name.trim())}"${members.length ? ` node="${members.join(' ')}"` : ''}/>`)
  }
  for (const n of act) {
    const p = partOf(n)
    const ins = flows.filter((e) => e.to === n.id).map((e) => xmlId('cf', e.id))
    const outs = flows.filter((e) => e.from === n.id).map((e) => xmlId('cf', e.id))
    out.push(
      `      <node xmi:type="${kind(n)}" xmi:id="${xmlId('an', n.id)}"${n.name.trim() ? ` name="${escapeXml(n.name.trim())}"` : ''}${p ? ` inPartition="${xmlId('ap', p.id)}"` : ''}${ins.length ? ` incoming="${ins.join(' ')}"` : ''}${outs.length ? ` outgoing="${outs.join(' ')}"` : ''}/>`,
    )
  }
  const byId = new Map(nodes.map((n) => [n.id, n]))
  for (const e of flows) {
    const objectEnd = byId.get(e.from)?.type === 'objectNode' || byId.get(e.to)?.type === 'objectNode'
    const guard = e.condition?.trim()
      ? `<guard xmi:type="uml:LiteralString" xmi:id="${xmlId('cf', e.id)}_g" value="${escapeXml(e.condition.trim())}"/>`
      : ''
    out.push(
      `      <edge xmi:type="${objectEnd ? 'uml:ObjectFlow' : 'uml:ControlFlow'}" xmi:id="${xmlId('cf', e.id)}"${e.label.trim() ? ` name="${escapeXml(e.label.trim())}"` : ''} source="${xmlId('an', e.from)}" target="${xmlId('an', e.to)}">${guard}</edge>`,
    )
  }
  out.push('    </packagedElement>')
  return out
}

/** `evento [guarda] / efeito` de uma transição. */
export function parseTransition(label: string): { trigger: string; guard: string; effect: string } {
  const m = /^\s*([^[/]*?)\s*(?:\[([^\]]*)\])?\s*(?:\/\s*(.*))?$/.exec(label)
  return { trigger: (m?.[1] ?? '').trim(), guard: (m?.[2] ?? '').trim(), effect: (m?.[3] ?? '').trim() }
}

/** Máquina de estados: uma região de topo e regiões aninhadas nos estados compostos. */
function stateMachineXmi(doc: DiagramDoc, nodes: DNode[], edges: DEdge[]): string[] {
  const states = nodes.filter((n) => STATE_NODES.has(n.type))
  if (states.length === 0) return []
  const trans = edges.filter((e) => e.type === 'transition')
  const composites = states.filter((n) => n.type === 'compositeState')
  // Dono directo: o composto mais pequeno que contém o centro.
  const parentOf = (n: DNode): DNode | undefined =>
    composites
      .filter((c) => c.id !== n.id && insideBox(n, c))
      .sort((a, b) => a.w * a.h - b.w * b.h)[0]
  const vertex = (n: DNode, ind: string): string => {
    const id = xmlId('sv', n.id)
    const nm = n.name.trim() ? ` name="${escapeXml(n.name.trim())}"` : ''
    switch (n.type) {
      case 'stateInitial':
        return `${ind}<subvertex xmi:type="uml:Pseudostate" xmi:id="${id}"${nm} kind="initial"/>`
      case 'choice':
        return `${ind}<subvertex xmi:type="uml:Pseudostate" xmi:id="${id}"${nm} kind="choice"/>`
      case 'history':
        return `${ind}<subvertex xmi:type="uml:Pseudostate" xmi:id="${id}"${nm} kind="${n.props.deep ? 'deepHistory' : 'shallowHistory'}"/>`
      case 'stateFinal':
        return `${ind}<subvertex xmi:type="uml:FinalState" xmi:id="${id}"${nm}/>`
      default: {
        const acts = (n.props.attributes ?? [])
          .map((raw, i) => {
            const m = /^\s*(entry|do|exit)\s*\/\s*(.*)$/.exec(raw)
            if (!m) return ''
            const tag = m[1] === 'do' ? 'doActivity' : m[1]
            return `<${tag} xmi:type="uml:OpaqueBehavior" xmi:id="${id}_b${i}" name="${escapeXml(m[2].trim())}"/>`
          })
          .join('')
        const children = states.filter((c) => parentOf(c)?.id === n.id)
        const region = n.type === 'compositeState'
          ? `\n${ind}  <region xmi:type="uml:Region" xmi:id="${id}_r">\n${children.map((c) => vertex(c, `${ind}    `)).join('\n')}${children.length ? '\n' : ''}${ind}  </region>\n${ind}`
          : ''
        return `${ind}<subvertex xmi:type="uml:State" xmi:id="${id}"${nm}>${acts}${region}</subvertex>`
      }
    }
  }
  const out: string[] = [`    <packagedElement xmi:type="uml:StateMachine" xmi:id="${xmlId('sm', doc.id)}" name="${escapeXml(doc.title)}">`]
  out.push(`      <region xmi:type="uml:Region" xmi:id="${xmlId('sm', doc.id)}_r">`)
  for (const n of states.filter((x) => !parentOf(x))) out.push(vertex(n, '        '))
  for (const e of trans) {
    const id = xmlId('tr', e.id)
    const { trigger, guard, effect } = parseTransition(e.label)
    const inner = [
      trigger ? `<trigger xmi:type="uml:Trigger" xmi:id="${id}_t" name="${escapeXml(trigger)}"/>` : '',
      guard ? `<guard xmi:type="uml:Constraint" xmi:id="${id}_g"><specification xmi:type="uml:OpaqueExpression" xmi:id="${id}_gs"><body>${escapeXml(guard)}</body></specification></guard>` : '',
      effect ? `<effect xmi:type="uml:OpaqueBehavior" xmi:id="${id}_e" name="${escapeXml(effect)}"/>` : '',
    ].join('')
    out.push(`        <transition xmi:type="uml:Transition" xmi:id="${id}" kind="external" source="${xmlId('sv', e.from)}" target="${xmlId('sv', e.to)}">${inner}</transition>`)
  }
  out.push('      </region>')
  out.push('    </packagedElement>')
  return out
}

function insideBox(n: DNode, container: DNode): boolean {
  const c = center(nodeBox(n))
  const b = nodeBox(container)
  return c.x >= b.x && c.x <= b.x + b.w && c.y >= b.y && c.y <= b.y + b.h
}

function overlapsX(a: DNode, b: DNode): boolean {
  const cx = a.x + a.w / 2
  return cx >= b.x && cx <= b.x + b.w
}

/** Mensagens por ordem vertical (a ordem do diagrama de sequência). */
export function sequenceMessages(doc: DiagramDoc): DEdge[] {
  return umlEdges(doc)
    .filter((e) => e.type === 'message' || e.type === 'reply' || e.type === 'lostMessage' || e.type === 'foundMessage')
    .map((e) => ({ e, y: edgeSegment(doc, e)?.a.y ?? 0 }))
    .sort((p, q) => p.y - q.y)
    .map((p) => p.e)
}

// ---------------------------------------------------------------------------
//  PlantUML
// ---------------------------------------------------------------------------

const puQuote = (s: string) => `"${s.replace(/"/g, "'").replace(/\n/g, ' ')}"`

export function toPlantUml(doc: DiagramDoc): string {
  const nodes = umlNodes(doc)
  const edges = umlEdges(doc)
  const byId = new Map(doc.nodes.map((n) => [n.id, n]))
  const alias = new Map<string, string>()
  nodes.forEach((n, i) => alias.set(n.id, `E${i + 1}`))
  const blocks: string[] = []
  const header = (suffix: string) => [`@startuml ${fileBase(doc.title)}${suffix}`, `title ${doc.title.replace(/\n/g, ' ')}`]

  const classNodes = nodes.filter((n) => CLASSIFIERS.has(n.type) || n.type === 'package' || n.type === 'note')
  const inClassBlock = new Set(classNodes.map((n) => n.id))
  const classEdges = edges.filter(
    (e) =>
      ['association', 'aggregation', 'composition', 'generalization', 'realization', 'dependency', 'anchor'].includes(e.type) &&
      inClassBlock.has(e.from) &&
      inClassBlock.has(e.to),
  )
  if (nodes.some((n) => CLASSIFIERS.has(n.type))) {
    const lines = header('')
    const pkgs = new Map<string, DNode[]>()
    const loose: DNode[] = []
    for (const n of classNodes) {
      if (!CLASSIFIERS.has(n.type)) continue
      const p = n.props.package?.trim()
      if (p) pkgs.set(p, [...(pkgs.get(p) ?? []), n])
      else loose.push(n)
    }
    const classifier = (n: DNode, ind: string) => {
      const kw = n.type === 'interface' ? 'interface' : n.type === 'enum' ? 'enum' : 'class'
      const st = n.props.stereotype && n.type === 'class' ? ` <<${n.props.stereotype}>>` : ''
      lines.push(`${ind}${kw} ${puQuote(n.name.trim() || alias.get(n.id)!)} as ${alias.get(n.id)}${st} {`)
      for (const a of n.props.attributes ?? []) {
        const m = parseMember(a)
        if (!m.name) continue
        lines.push(n.type === 'enum' ? `${ind}  ${m.name}` : `${ind}  ${m.visibility}${m.name}${m.type ? ` : ${m.type}` : ''}`)
      }
      for (const o of n.props.operations ?? []) {
        const m = parseMember(o)
        if (!m.name) continue
        lines.push(`${ind}  ${m.visibility}${m.name}(${m.params})${m.type ? ` : ${m.type}` : ''}`)
      }
      lines.push(`${ind}}`)
    }
    for (const [name, members] of pkgs) {
      lines.push(`package ${puQuote(name)} {`)
      for (const m of members) classifier(m, '  ')
      lines.push('}')
    }
    // Classes soltas do mesmo grupo de selecção ficam juntas no desenho.
    const together = new Set<string>()
    for (const g of groupsOf(doc)) {
      const members = loose.filter((n) => g.nodes.includes(n.id) && !together.has(n.id))
      if (members.length === 0) continue
      lines.push(`' ${g.name.replace(/\n/g, ' ').trim()}`)
      lines.push('together {')
      for (const m of members) {
        together.add(m.id)
        classifier(m, '  ')
      }
      lines.push('}')
    }
    for (const m of loose) if (!together.has(m.id)) classifier(m, '')
    for (const n of classNodes) {
      if (n.type === 'note') lines.push(`note ${puQuote((n.props.text ?? n.name).trim())} as ${alias.get(n.id)}`)
    }
    for (const e of classEdges) {
      const a = alias.get(e.from)!
      const b = alias.get(e.to)!
      const src = e.srcMult?.trim() ? ` "${e.srcMult.trim()}"` : ''
      const dst = e.dstMult?.trim() ? `"${e.dstMult.trim()}" ` : ''
      const arrow = {
        association: '--',
        aggregation: 'o--',
        composition: '*--',
        generalization: '--|>',
        realization: '..|>',
        dependency: '..>',
        anchor: '..',
      }[e.type as 'association']
      const lbl = e.label.trim() ? ` : ${e.label.trim().replace(/\n/g, ' ')}` : ''
      if (e.type === 'anchor') {
        if (byId.get(e.from)?.type === 'note' || byId.get(e.to)?.type === 'note') lines.push(`${a} .. ${b}`)
        continue
      }
      lines.push(`${a}${src} ${arrow} ${dst}${b}${lbl}`)
    }
    lines.push('@enduml')
    blocks.push(lines.join('\n'))
  }

  const lifelines = nodes.filter((n) => n.type === 'lifeline')
  if (lifelines.length > 0) {
    const lines = header('_sequencia')
    for (const l of [...lifelines].sort((p, q) => p.x - q.x)) lines.push(`participant ${puQuote(l.name.trim() || alias.get(l.id)!)} as ${alias.get(l.id)}`)
    const msgs = sequenceMessages(doc)
    const frags = nodes
      .filter((n) => n.type === 'fragment')
      .map((f) => ({ f, top: f.y, bottom: f.y + f.h }))
      .sort((p, q) => p.top - q.top)
    const open: typeof frags = []
    for (const e of msgs) {
      const y = edgeSegment(doc, e)?.a.y ?? 0
      while (open.length && y > open[open.length - 1].bottom) {
        open.pop()
        lines.push('end')
      }
      for (const fr of frags) {
        if (open.includes(fr)) continue
        if (fr.top <= y && y <= fr.bottom && !open.some((o) => o === fr)) {
          open.push(fr)
          lines.push(`${fr.f.props.operator ?? 'alt'}${fr.f.name.trim() ? ` ${fr.f.name.trim()}` : ''}`)
        }
      }
      const text = e.label.trim() ? ` : ${e.label.trim().replace(/\n/g, ' ')}` : ''
      if (e.type === 'lostMessage') lines.push(`${alias.get(e.from)} ->x]${text}`)
      else if (e.type === 'foundMessage') lines.push(`[o-> ${alias.get(e.to)}${text}`)
      else lines.push(`${alias.get(e.from)} ${e.type === 'reply' ? '-->' : '->'} ${alias.get(e.to)}${text}`)
    }
    while (open.pop()) lines.push('end')
    lines.push('@enduml')
    blocks.push(lines.join('\n'))
  }

  const ucNodes = nodes.filter((n) => n.type === 'actor' || n.type === 'usecase' || n.type === 'boundary')
  if (ucNodes.length > 0) {
    const lines = header('_casos')
    lines.push('left to right direction')
    for (const n of ucNodes.filter((x) => x.type === 'actor')) lines.push(`actor ${puQuote(n.name.trim() || alias.get(n.id)!)} as ${alias.get(n.id)}`)
    const inBoundary = new Set<string>()
    for (const b of ucNodes.filter((x) => x.type === 'boundary')) {
      lines.push(`rectangle ${puQuote(b.name.trim() || alias.get(b.id)!)} {`)
      for (const u of ucNodes.filter((x) => x.type === 'usecase' && insideBox(x, b))) {
        inBoundary.add(u.id)
        lines.push(`  usecase ${puQuote(u.name.trim() || alias.get(u.id)!)} as ${alias.get(u.id)}`)
      }
      lines.push('}')
    }
    for (const u of ucNodes.filter((x) => x.type === 'usecase' && !inBoundary.has(x.id))) {
      lines.push(`usecase ${puQuote(u.name.trim() || alias.get(u.id)!)} as ${alias.get(u.id)}`)
    }
    for (const e of edges) {
      if (!['association', 'include', 'extend', 'generalization'].includes(e.type)) continue
      if (!byId.has(e.to) || !(byId.get(e.to)!.type === 'actor' || byId.get(e.to)!.type === 'usecase')) continue
      const a = byId.get(e.from)!
      if (!(a.type === 'actor' || a.type === 'usecase')) continue
      const arrow = e.type === 'include' || e.type === 'extend' ? `..> ` : e.type === 'generalization' ? '--|> ' : '--> '
      const lbl = e.type === 'include' ? ' : <<include>>' : e.type === 'extend' ? ' : <<extend>>' : e.label.trim() ? ` : ${e.label.trim()}` : ''
      lines.push(`${alias.get(e.from)} ${arrow}${alias.get(e.to)}${lbl}`)
    }
    lines.push('@enduml')
    blocks.push(lines.join('\n'))
  }
  blocks.push(...behaviourPlantUml(doc, nodes, edges, alias, header))
  return blocks.join('\n\n') + '\n'
}

/** Actividade e estados (sintaxe de estados, que aceita um grafo qualquer), componentes/implantação e objectos. */
function behaviourPlantUml(doc: DiagramDoc, nodes: DNode[], edges: DEdge[], alias: Map<string, string>, header: (s: string) => string[]): string[] {
  const blocks: string[] = []
  const byId = new Map(nodes.map((n) => [n.id, n]))
  const nm = (n: DNode) => puQuote(n.name.trim() || alias.get(n.id)!)
  const graph = (kinds: ReadonlySet<DNode['type']>, edgeType: DEdge['type'], suffix: string, partitionType?: DNode['type']) => {
    const members = nodes.filter((n) => kinds.has(n.type))
    if (members.length === 0) return
    const lines = header(suffix)
    const containers = nodes.filter((n) => n.type === 'compositeState' || n.type === partitionType)
    const parentOf = (n: DNode) =>
      containers.filter((c) => c.id !== n.id && insideBox(n, c)).sort((a, b) => a.w * a.h - b.w * b.h)[0]
    const pseudo = (n: DNode) => ['initialNode', 'stateInitial', 'activityFinal', 'flowFinal', 'stateFinal'].includes(n.type)
    const declare = (n: DNode, ind: string) => {
      if (pseudo(n)) return
      const a = alias.get(n.id)
      const children = [...members, ...containers].filter((c) => c.id !== n.id && parentOf(c)?.id === n.id)
      if (n.type === 'decisionNode' || n.type === 'choice') lines.push(`${ind}state ${a} <<choice>>`)
      else if (n.type === 'forkNode') {
        const ins = edges.filter((e) => e.type === edgeType && e.to === n.id).length
        lines.push(`${ind}state ${a} <<${ins >= 2 ? 'join' : 'fork'}>>`)
      } else if (n.type === 'history') lines.push(`${ind}state ${a} <<${n.props.deep ? 'history*' : 'history'}>>`)
      else if (children.length) {
        lines.push(`${ind}state ${nm(n)} as ${a} {`)
        for (const c of children) declare(c, `${ind}  `)
        lines.push(`${ind}}`)
      } else {
        lines.push(`${ind}state ${nm(n)} as ${a}${n.type === 'objectNode' ? ' <<object>>' : ''}`)
        for (const act of n.props.attributes ?? []) lines.push(`${ind}${a} : ${act.replace(/\n/g, ' ')}`)
      }
    }
    for (const n of [...containers, ...members].filter((x) => !parentOf(x))) declare(n, '')
    const end = (id: string) => (pseudo(byId.get(id)!) ? '[*]' : alias.get(id)!)
    for (const e of edges) {
      if (e.type !== edgeType) continue
      const guard = e.condition?.trim() ? ` [${e.condition.trim()}]` : ''
      const text = `${e.label.trim()}${guard}`.trim()
      lines.push(`${end(e.from)} --> ${end(e.to)}${text ? ` : ${text.replace(/\n/g, ' ')}` : ''}`)
    }
    lines.push('@enduml')
    blocks.push(lines.join('\n'))
  }
  graph(ACTIVITY_NODES, 'controlFlow', '_actividade', 'partition')
  graph(STATE_NODES, 'transition', '_estados')

  const structural = nodes.filter((n) => ['component', 'port', 'providedInterface', 'requiredInterface', 'deviceNode', 'artifact'].includes(n.type))
  if (structural.length > 0) {
    const lines = header('_componentes')
    const hosts = nodes.filter((n) => n.type === 'deviceNode' || n.type === 'component')
    const hostOf = (n: DNode) =>
      hosts
        .filter((h) => h.id !== n.id && (insideBox(n, h) || (n.type === 'port' && h.type === 'component' && nearBox(n, h))))
        .sort((a, b) => a.w * a.h - b.w * b.h)[0]
    const decl = (n: DNode, ind: string) => {
      const a = alias.get(n.id)
      const kids = structural.filter((c) => hostOf(c)?.id === n.id)
      const kw = { component: 'component', port: 'port', providedInterface: 'interface', requiredInterface: 'interface', deviceNode: 'node', artifact: 'artifact' }[n.type as 'component']
      const st = n.props.stereotype && !['component', 'artifact'].includes(n.props.stereotype) ? ` <<${n.props.stereotype}>>` : ''
      if (kids.length) {
        lines.push(`${ind}${kw} ${nm(n)} as ${a}${st} {`)
        for (const k of kids) decl(k, `${ind}  `)
        lines.push(`${ind}}`)
      } else lines.push(`${ind}${kw} ${nm(n)} as ${a}${st}`)
    }
    for (const n of structural.filter((x) => !hostOf(x))) decl(n, '')
    const ids = new Set(structural.map((n) => n.id))
    for (const e of edges) {
      if (!ids.has(e.from) || !ids.has(e.to)) continue
      const text = e.label.trim() ? ` : ${e.label.trim().replace(/\n/g, ' ')}` : ''
      if (e.type === 'realization') lines.push(`${alias.get(e.from)} - ${alias.get(e.to)}${text}`)
      else if (e.type === 'usage') lines.push(`${alias.get(e.from)} ..> ${alias.get(e.to)} : <<use>>`)
      else if (e.type === 'deploy') lines.push(`${alias.get(e.from)} ..> ${alias.get(e.to)} : <<deploy>>`)
      else if (e.type === 'manifest') lines.push(`${alias.get(e.from)} ..> ${alias.get(e.to)} : <<manifest>>`)
      else if (e.type === 'dependency') lines.push(`${alias.get(e.from)} ..> ${alias.get(e.to)}${text}`)
      else if (e.type === 'association') lines.push(`${alias.get(e.from)} -- ${alias.get(e.to)}${text}`)
    }
    lines.push('@enduml')
    blocks.push(lines.join('\n'))
  }

  const objects = nodes.filter((n) => n.type === 'object')
  if (objects.length > 0) {
    const lines = header('_objectos')
    for (const o of objects) {
      const title = `${o.name.trim() || alias.get(o.id)}${o.props.instanceOf?.trim() ? ` : ${o.props.instanceOf.trim()}` : ''}`
      const slots = o.props.attributes ?? []
      if (slots.length) {
        lines.push(`object ${puQuote(title)} as ${alias.get(o.id)} {`)
        for (const sl of slots) lines.push(`  ${sl.replace(/\n/g, ' ')}`)
        lines.push('}')
      } else lines.push(`object ${puQuote(title)} as ${alias.get(o.id)}`)
    }
    for (const e of edges) if (e.type === 'link') lines.push(`${alias.get(e.from)} -- ${alias.get(e.to)}${e.label.trim() ? ` : ${e.label.trim()}` : ''}`)
    lines.push('@enduml')
    blocks.push(lines.join('\n'))
  }
  void doc
  return blocks
}

function nearBox(n: DNode, host: DNode): boolean {
  const c = center(nodeBox(n))
  const b = nodeBox(host)
  return c.x >= b.x - 10 && c.x <= b.x + b.w + 10 && c.y >= b.y - 10 && c.y <= b.y + b.h + 10
}

// ---------------------------------------------------------------------------
//  BPMN 2.0
// ---------------------------------------------------------------------------

const BPMN_NS =
  'xmlns:bpmn="http://www.omg.org/spec/BPMN/20100524/MODEL" xmlns:bpmndi="http://www.omg.org/spec/BPMN/20100524/DI" xmlns:dc="http://www.omg.org/spec/DD/20100524/DC" xmlns:di="http://www.omg.org/spec/DD/20100524/DI" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:delonix="https://meet.delonix.co.ao/schema/bpmn"'

function bpmnTag(n: DNode): string {
  switch (n.type) {
    case 'startEvent':
      return 'startEvent'
    case 'endEvent':
      return 'endEvent'
    case 'intermediateEvent':
      return n.props.boundary ? 'boundaryEvent' : n.props.throwing ? 'intermediateThrowEvent' : 'intermediateCatchEvent'
    case 'subProcess':
      return n.props.adHoc ? 'adHocSubProcess' : 'subProcess'
    case 'gateway':
      return (
        {
          exclusive: 'exclusiveGateway',
          parallel: 'parallelGateway',
          inclusive: 'inclusiveGateway',
          eventBased: 'eventBasedGateway',
          eventInstantiate: 'eventBasedGateway',
          eventParallel: 'eventBasedGateway',
          complex: 'complexGateway',
        } as const
      )[n.props.gatewayKind ?? 'exclusive']
    case 'task':
      return (
        {
          none: 'task',
          user: 'userTask',
          service: 'serviceTask',
          script: 'scriptTask',
          manual: 'manualTask',
          send: 'sendTask',
          receive: 'receiveTask',
          businessRule: 'businessRuleTask',
          call: 'callActivity',
        } as const
      )[n.props.taskKind ?? 'none']
    default:
      return 'task'
  }
}

const EVENT_DEFINITION: Record<Exclude<DNode['props']['trigger'], undefined | 'none'>, string> = {
  message: 'messageEventDefinition',
  timer: 'timerEventDefinition',
  signal: 'signalEventDefinition',
  error: 'errorEventDefinition',
  escalation: 'escalationEventDefinition',
  compensation: 'compensateEventDefinition',
  conditional: 'conditionalEventDefinition',
  link: 'linkEventDefinition',
  terminate: 'terminateEventDefinition',
}

function eventDefinition(trig: keyof typeof EVENT_DEFINITION, id: string, name: string): string {
  const tag = EVENT_DEFINITION[trig]
  // A condição é obrigatória no esquema; a ligação (link) é emparelhada pelo nome.
  if (trig === 'conditional') return `<bpmn:${tag} id="${id}_def"><bpmn:condition xsi:type="bpmn:tFormalExpression"/></bpmn:${tag}>`
  if (trig === 'link') return `<bpmn:${tag} id="${id}_def" name="${escapeXml(name)}"/>`
  return `<bpmn:${tag} id="${id}_def"/>`
}

export function toBpmn(doc: DiagramDoc): string {
  const byId = new Map(doc.nodes.map((n) => [n.id, n]))
  const pools = doc.nodes.filter((n) => n.type === 'pool')
  const flowNodes = doc.nodes.filter((n) => BPMN_FLOW_NODES.has(n.type))
  const artifacts = doc.nodes.filter((n) => n.type === 'dataObject' || n.type === 'annotation' || n.type === 'dataStore' || n.type === 'group')
  const activities = flowNodes.filter((n) => n.type === 'task' || n.type === 'subProcess')
  const hostOf = (ev: DNode) => (ev.type === 'intermediateEvent' && ev.props.boundary ? attachedActivity(ev, activities) : undefined)
  const edges = doc.edges.filter((e) => EDGE_NOTATION[e.type] === 'bpmn' && byId.has(e.from) && byId.has(e.to))
  const seq = edges.filter((e) => e.type === 'sequenceFlow')

  const processKey = (n: DNode) => poolOf(doc, n)?.id ?? ''
  const processIds = new Map<string, string>()
  for (const p of pools) processIds.set(p.id, xmlId('Process', p.id))
  const hasLoose = [...flowNodes, ...artifacts].some((n) => !poolOf(doc, n))
  if (hasLoose || pools.length === 0) processIds.set('', xmlId('Process', doc.id))

  const out: string[] = []
  out.push('<?xml version="1.0" encoding="UTF-8"?>')
  out.push(`<bpmn:definitions ${BPMN_NS} id="${xmlId('Definitions', doc.id)}" targetNamespace="https://meet.delonix.co.ao/bpmn" exporter="Delonix Meet" exporterVersion="1">`)

  const messageFlows = edges.filter((e) => e.type === 'messageFlow')
  const collabId = xmlId('Collaboration', doc.id)
  const groups = artifacts.filter((a) => a.type === 'group')
  if (pools.length > 0) {
    out.push(`  <bpmn:collaboration id="${collabId}">`)
    for (const p of pools) out.push(`    <bpmn:participant id="${xmlId('Participant', p.id)}" name="${escapeXml(p.name.trim())}" processRef="${processIds.get(p.id)}"/>`)
    for (const e of messageFlows) {
      const ref = (id: string) => (byId.get(id)!.type === 'pool' ? xmlId('Participant', id) : xmlId('N', id))
      out.push(`    <bpmn:messageFlow id="${xmlId('F', e.id)}"${e.label.trim() ? ` name="${escapeXml(e.label.trim())}"` : ''} sourceRef="${ref(e.from)}" targetRef="${ref(e.to)}"/>`)
    }
    out.push('  </bpmn:collaboration>')
  }
  // Um grupo BPMN aponta para um valor de categoria definido fora do processo.
  for (const g of groups) {
    out.push(`  <bpmn:category id="${xmlId('Category', g.id)}"><bpmn:categoryValue id="${xmlId('CategoryValue', g.id)}" value="${escapeXml(g.name.trim())}"/></bpmn:category>`)
  }
  // Grupos de selecção (⌘G) saem como o mesmo artefacto, no processo do primeiro membro.
  const bpmnMembers = new Set([...flowNodes, ...artifacts].map((n) => n.id))
  const selGroups = groupsOf(doc)
    .map((g) => ({ g, members: g.nodes.filter((id) => bpmnMembers.has(id)).map((id) => byId.get(id)!) }))
    .filter((x) => x.members.length > 0)
  for (const { g } of selGroups) {
    out.push(`  <bpmn:category id="${xmlId('SelCategory', g.id)}"><bpmn:categoryValue id="${xmlId('SelCategoryValue', g.id)}" value="${escapeXml(g.name.trim())}"/></bpmn:category>`)
  }

  for (const [key, pid] of processIds) {
    const pool = key ? byId.get(key)! : null
    out.push(`  <bpmn:process id="${pid}" isExecutable="false"${pool ? ` name="${escapeXml(pool.name.trim())}"` : ''}>`)
    const members = flowNodes.filter((n) => processKey(n) === key)
    const lanes = pool?.props.lanes ?? []
    const io = artifacts.filter((a) => processKey(a) === key && a.type === 'dataObject' && (a.props.dataRole === 'input' || a.props.dataRole === 'output'))
    if (io.length > 0) {
      // ioSpecification vem antes do laneSet (tCallableElement → tProcess) e pede os dois conjuntos.
      const ins = io.filter((a) => a.props.dataRole === 'input')
      const outs = io.filter((a) => a.props.dataRole === 'output')
      const coll = (a: DNode) => (a.props.collection ? ' isCollection="true"' : '')
      out.push(`    <bpmn:ioSpecification id="${xmlId('IO', pid)}">`)
      for (const a of ins) out.push(`      <bpmn:dataInput id="${xmlId('N', a.id)}" name="${escapeXml(a.name.trim())}"${coll(a)}/>`)
      for (const a of outs) out.push(`      <bpmn:dataOutput id="${xmlId('N', a.id)}" name="${escapeXml(a.name.trim())}"${coll(a)}/>`)
      out.push(`      <bpmn:inputSet id="${xmlId('InputSet', pid)}">${ins.map((a) => `<bpmn:dataInputRefs>${xmlId('N', a.id)}</bpmn:dataInputRefs>`).join('')}</bpmn:inputSet>`)
      out.push(`      <bpmn:outputSet id="${xmlId('OutputSet', pid)}">${outs.map((a) => `<bpmn:dataOutputRefs>${xmlId('N', a.id)}</bpmn:dataOutputRefs>`).join('')}</bpmn:outputSet>`)
      out.push('    </bpmn:ioSpecification>')
    }
    if (pool && lanes.length > 0) {
      out.push(`    <bpmn:laneSet id="${xmlId('LaneSet', pool.id)}">`)
      for (const lane of lanes) {
        const refs = [...members, ...artifacts.filter((a) => processKey(a) === key)]
          .filter((n) => laneOf(doc, n)?.lane.id === lane.id && BPMN_FLOW_NODES.has(n.type))
          .map((n) => `<bpmn:flowNodeRef>${xmlId('N', n.id)}</bpmn:flowNodeRef>`)
        out.push(`      <bpmn:lane id="${xmlId('Lane', lane.id)}" name="${escapeXml(lane.name.trim())}">${refs.join('')}</bpmn:lane>`)
      }
      out.push('    </bpmn:laneSet>')
    }
    for (const n of members) {
      const tag = bpmnTag(n)
      const id = xmlId('N', n.id)
      const outs = seq.filter((e) => e.from === n.id)
      const ins = seq.filter((e) => e.to === n.id)
      const def = outs.find((e) => e.isDefault)
      const attrs = [`id="${id}"`]
      if (n.name.trim()) attrs.push(`name="${escapeXml(n.name.trim())}"`)
      if (def && (n.type === 'gateway' || n.type === 'task' || n.type === 'subProcess')) attrs.push(`default="${xmlId('F', def.id)}"`)
      const host = hostOf(n)
      if (host) {
        attrs.push(`attachedToRef="${xmlId('N', host.id)}"`)
        if (n.props.nonInterrupting) attrs.push('cancelActivity="false"')
      }
      if (n.type === 'gateway' && (n.props.gatewayKind === 'eventInstantiate' || n.props.gatewayKind === 'eventParallel')) {
        attrs.push(`instantiate="true" eventGatewayType="${n.props.gatewayKind === 'eventParallel' ? 'Parallel' : 'Exclusive'}"`)
      }
      if ((n.type === 'task' || n.type === 'subProcess') && n.props.compensation) attrs.push('isForCompensation="true"')
      const inner: string[] = []
      if (n.type === 'task' && n.props.implementation?.trim()) {
        inner.push(`<bpmn:extensionElements><delonix:executor>${escapeXml(n.props.implementation.trim())}</delonix:executor></bpmn:extensionElements>`)
      }
      for (const e of ins) inner.push(`<bpmn:incoming>${xmlId('F', e.id)}</bpmn:incoming>`)
      for (const e of outs) inner.push(`<bpmn:outgoing>${xmlId('F', e.id)}</bpmn:outgoing>`)
      if (n.type === 'task' && n.props.multiInstance && n.props.multiInstance !== 'none') {
        inner.push(`<bpmn:multiInstanceLoopCharacteristics isSequential="${n.props.multiInstance === 'sequential'}"/>`)
      } else if ((n.type === 'task' || n.type === 'subProcess') && n.props.loop) {
        inner.push(`<bpmn:standardLoopCharacteristics/>`)
      }
      const trig = n.props.trigger
      if ((n.type === 'startEvent' || n.type === 'intermediateEvent' || n.type === 'endEvent') && trig && trig !== 'none') {
        inner.push(eventDefinition(trig, id, n.name.trim()))
      }
      out.push(inner.length ? `    <bpmn:${tag} ${attrs.join(' ')}>${inner.join('')}</bpmn:${tag}>` : `    <bpmn:${tag} ${attrs.join(' ')}/>`)
    }
    for (const e of seq.filter((f) => processKey(byId.get(f.from)!) === key)) {
      const cond = e.condition?.trim()
      const attrs = `id="${xmlId('F', e.id)}"${e.label.trim() ? ` name="${escapeXml(e.label.trim())}"` : ''} sourceRef="${xmlId('N', e.from)}" targetRef="${xmlId('N', e.to)}"`
      out.push(
        cond
          ? `    <bpmn:sequenceFlow ${attrs}><bpmn:conditionExpression xsi:type="bpmn:tFormalExpression">${escapeXml(cond)}</bpmn:conditionExpression></bpmn:sequenceFlow>`
          : `    <bpmn:sequenceFlow ${attrs}/>`,
      )
    }
    for (const a of artifacts.filter((x) => processKey(x) === key)) {
      if (a.type === 'dataObject' && (a.props.dataRole ?? 'none') === 'none') {
        out.push(`    <bpmn:dataObject id="${xmlId('DO', a.id)}"${a.props.collection ? ' isCollection="true"' : ''}/>`)
        out.push(`    <bpmn:dataObjectReference id="${xmlId('N', a.id)}" name="${escapeXml(a.name.trim())}" dataObjectRef="${xmlId('DO', a.id)}"/>`)
      }
      if (a.type === 'dataStore') out.push(`    <bpmn:dataStoreReference id="${xmlId('N', a.id)}" name="${escapeXml(a.name.trim())}"/>`)
    }
    for (const a of artifacts.filter((x) => processKey(x) === key)) {
      if (a.type === 'annotation') {
        out.push(`    <bpmn:textAnnotation id="${xmlId('N', a.id)}"><bpmn:text>${escapeXml((a.props.text ?? a.name).trim())}</bpmn:text></bpmn:textAnnotation>`)
      }
      if (a.type === 'group') out.push(`    <bpmn:group id="${xmlId('N', a.id)}" categoryValueRef="${xmlId('CategoryValue', a.id)}"/>`)
    }
    for (const { g, members } of selGroups.filter((x) => processKey(x.members[0]) === key)) {
      if (members.length) out.push(`    <bpmn:group id="${xmlId('SelGroup', g.id)}" categoryValueRef="${xmlId('SelCategoryValue', g.id)}"/>`)
    }
    for (const e of edges.filter((f) => f.type === 'dataAssociation' && processKey(byId.get(f.from)!) === key)) {
      out.push(`    <bpmn:association id="${xmlId('F', e.id)}" sourceRef="${xmlId('N', e.from)}" targetRef="${xmlId('N', e.to)}"/>`)
    }
    out.push('  </bpmn:process>')
  }

  // Diagrama (DI).
  const plane = pools.length > 0 ? collabId : processIds.get('')!
  out.push(`  <bpmndi:BPMNDiagram id="${xmlId('Diagram', doc.id)}">`)
  out.push(`    <bpmndi:BPMNPlane id="${xmlId('Plane', doc.id)}" bpmnElement="${plane}">`)
  const bounds = (x: number, y: number, w: number, h: number) =>
    `<dc:Bounds x="${Math.round(x)}" y="${Math.round(y)}" width="${Math.round(w)}" height="${Math.round(h)}"/>`
  for (const p of pools) {
    out.push(`      <bpmndi:BPMNShape id="${xmlId('Participant', p.id)}_di" bpmnElement="${xmlId('Participant', p.id)}" isHorizontal="true">${bounds(p.x, p.y, p.w, poolHeight(p))}</bpmndi:BPMNShape>`)
    let top = p.y
    for (const lane of p.props.lanes ?? []) {
      out.push(`      <bpmndi:BPMNShape id="${xmlId('Lane', lane.id)}_di" bpmnElement="${xmlId('Lane', lane.id)}" isHorizontal="true">${bounds(p.x + 30, top, p.w - 30, lane.size)}</bpmndi:BPMNShape>`)
      top += lane.size
    }
  }
  for (const n of [...flowNodes, ...artifacts]) {
    const b = nodeBox(n)
    out.push(`      <bpmndi:BPMNShape id="${xmlId('N', n.id)}_di" bpmnElement="${xmlId('N', n.id)}">${bounds(b.x, b.y, b.w, b.h)}</bpmndi:BPMNShape>`)
  }
  for (const { g } of selGroups) {
    const b = pickBox(doc, { nodes: g.nodes, strokes: [] })
    if (b) out.push(`      <bpmndi:BPMNShape id="${xmlId('SelGroup', g.id)}_di" bpmnElement="${xmlId('SelGroup', g.id)}">${bounds(b.x - GROUP_PAD, b.y - GROUP_PAD, b.w + GROUP_PAD * 2, b.h + GROUP_PAD * 2)}</bpmndi:BPMNShape>`)
  }
  for (const e of edges) {
    const seg = edgeSegment(doc, e)
    if (!seg) continue
    out.push(
      `      <bpmndi:BPMNEdge id="${xmlId('F', e.id)}_di" bpmnElement="${xmlId('F', e.id)}"><di:waypoint x="${Math.round(seg.a.x)}" y="${Math.round(seg.a.y)}"/><di:waypoint x="${Math.round(seg.b.x)}" y="${Math.round(seg.b.y)}"/></bpmndi:BPMNEdge>`,
    )
  }
  out.push('    </bpmndi:BPMNPlane>')
  out.push('  </bpmndi:BPMNDiagram>')
  out.push('</bpmn:definitions>')
  return out.join('\n') + '\n'
}

// ---------------------------------------------------------------------------
//  C4-PlantUML (arquitectura)
// ---------------------------------------------------------------------------

const c4Quote = (s: string) => `"${s.replace(/"/g, "'").replace(/\n/g, ' ').trim()}"`

/**
 * Arquitectura em C4-PlantUML (biblioteca padrão do PlantUML, `<C4/...>`).
 * Os elementos C4 saem com as macros deles; os outros componentes de
 * arquitectura (serviço, base de dados, fila, recursos do catálogo) saem como
 * contentores, com o tipo como tecnologia — o diagrama continua a renderizar.
 * As fronteiras e os nós de implantação aninham pelo desenho.
 */
export function toC4PlantUml(doc: DiagramDoc, typeName: (n: DNode) => string = (n) => n.props.catalog ?? n.type): string {
  const nodes = doc.nodes.filter((n) => NODE_NOTATION[n.type] === 'arch' && n.type !== 'zone')
  const ids = new Set(nodes.map((n) => n.id))
  const alias = (n: DNode) => `e_${n.id.replace(/[^A-Za-z0-9_]/g, '_')}`
  const boxes = nodes.filter((n) => n.type === 'c4Boundary' || n.type === 'c4DeploymentNode' || n.type === 'resourceGroup')
  const parentOf = (n: DNode) => boxes.filter((b) => b.id !== n.id && insideBox(n, b) && b.w * b.h > n.w * n.h).sort((a, b) => a.w * a.h - b.w * b.h)[0]
  const hasDeployment = nodes.some((n) => n.type === 'c4DeploymentNode' || n.type === 'resourceGroup')
  const hasComponent = nodes.some((n) => n.type === 'c4Component' || n.type === 'c4Code')
  const lines: string[] = [`@startuml ${fileBase(doc.title)}_c4`]
  if (hasDeployment) lines.push('!include <C4/C4_Deployment>')
  if (hasComponent || !hasDeployment) lines.push(hasComponent ? '!include <C4/C4_Component>' : '!include <C4/C4_Container>')
  lines.push(`title ${doc.title.replace(/\n/g, ' ')}`)
  const q = (v: string | undefined) => c4Quote(v ?? '')
  const decl = (n: DNode, ind: string) => {
    const a = alias(n)
    const name = q(n.name || typeName(n))
    const desc = n.props.description?.trim()
    const tech = n.props.technology?.trim()
    const ext = n.props.external ? '_Ext' : ''
    const kids = nodes.filter((c) => parentOf(c)?.id === n.id)
    const block = (open: string) => {
      lines.push(`${ind}${open} {`)
      for (const k of kids) decl(k, `${ind}  `)
      lines.push(`${ind}}`)
    }
    switch (n.type) {
      case 'c4Person':
        lines.push(`${ind}Person${ext}(${a}, ${name}${desc ? `, ${q(desc)}` : ''})`)
        break
      case 'c4System':
        lines.push(`${ind}System${ext}(${a}, ${name}${desc ? `, ${q(desc)}` : ''})`)
        break
      case 'c4Container':
      case 'c4Component':
      case 'c4Code': {
        const base = n.type === 'c4Container' ? 'Container' : 'Component'
        const shape = n.props.c4Shape === 'db' ? 'Db' : n.props.c4Shape === 'queue' ? 'Queue' : ''
        const t = n.type === 'c4Code' ? tech || 'code' : tech ?? ''
        lines.push(`${ind}${base}${shape}${ext}(${a}, ${name}, ${q(t)}${desc ? `, ${q(desc)}` : ''})`)
        break
      }
      case 'c4Boundary': {
        const k = n.props.boundaryKind ?? 'system'
        block(`${k === 'container' ? 'Container_Boundary' : k === 'enterprise' ? 'Enterprise_Boundary' : 'System_Boundary'}(${a}, ${name})`)
        break
      }
      case 'c4DeploymentNode':
      case 'resourceGroup':
        block(`Deployment_Node(${a}, ${name}, ${q(tech || (n.type === 'resourceGroup' ? typeName(n) : ''))}${desc ? `, ${q(desc)}` : ''})`)
        break
      default: {
        const shape = n.type === 'database' ? 'Db' : n.type === 'queue' ? 'Queue' : ''
        const t = n.type === 'resource' ? tech || typeName(n) : tech || n.props.stereotype || ''
        lines.push(`${ind}${n.type === 'external' ? 'System_Ext' : `Container${shape}`}(${a}, ${name}${n.type === 'external' ? '' : `, ${q(t)}`}${desc ? `, ${q(desc)}` : ''})`)
      }
    }
  }
  for (const n of nodes.filter((x) => !parentOf(x))) decl(n, '')
  const byId = new Map(nodes.map((n) => [n.id, n]))
  for (const e of doc.edges) {
    if (EDGE_NOTATION[e.type] !== 'arch' || !ids.has(e.from) || !ids.has(e.to)) continue
    const tech = e.technology?.trim()
    const label = e.label.trim() || (e.type === 'async' ? 'async' : '')
    lines.push(`Rel(${alias(byId.get(e.from)!)}, ${alias(byId.get(e.to)!)}, ${q(label)}${tech ? `, ${q(tech)}` : ''})`)
  }
  lines.push('@enduml')
  return lines.join('\n') + '\n'
}

// ---------------------------------------------------------------------------
//  SVG e JSON
// ---------------------------------------------------------------------------

/**
 * Documento SVG autónomo a partir do conteúdo desenhado (já serializado) e da
 * caixa que o enquadra. O papel é branco, sem grelha: é um documento.
 */
export function wrapSvg(inner: string, box: { x: number; y: number; w: number; h: number }, title: string, pad = 24): string {
  const x = Math.floor(box.x - pad)
  const y = Math.floor(box.y - pad)
  const w = Math.ceil(box.w + pad * 2)
  const h = Math.ceil(box.h + pad * 2)
  return [
    `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="${x} ${y} ${w} ${h}" font-family="${escapeXml(FONT)}">`,
    `<title>${escapeXml(title)}</title>`,
    `<rect x="${x}" y="${y}" width="${w}" height="${h}" fill="${INK.surface}"/>`,
    inner,
    '</svg>',
  ].join('\n')
}

/** Caixa para a exportação SVG/PNG (o conteúdo, ou um quadrado vazio). */
export function exportBox(doc: DiagramDoc) {
  return contentBox(doc) ?? { x: 0, y: 0, w: 320, h: 200 }
}

export function toJson(doc: DiagramDoc): string {
  return JSON.stringify({ format: 'delonix-diagram', ...doc }, null, 2) + '\n'
}

/**
 * Lê um modelo exportado em JSON. Recusa o que não for um quadro — um JSON
 * qualquer não pode entrar no IndexedDB e rebentar o editor ao abrir.
 */
export function parseJson(text: string): DiagramDoc | null {
  let raw: unknown
  try {
    raw = JSON.parse(text)
  } catch {
    return null
  }
  if (!raw || typeof raw !== 'object') return null
  const d = raw as Partial<DiagramDoc> & { format?: string }
  if (d.format !== 'delonix-diagram' || d.v !== 1) return null
  if (!Array.isArray(d.nodes) || !Array.isArray(d.edges)) return null
  const okNode = (n: unknown) => {
    const x = n as DNode
    return !!x && typeof x.id === 'string' && typeof x.type === 'string' && x.type in NODE_NOTATION && Number.isFinite(x.x) && Number.isFinite(x.y)
  }
  const okEdge = (e: unknown) => {
    const x = e as DEdge
    return !!x && typeof x.id === 'string' && typeof x.type === 'string' && x.type in EDGE_NOTATION && typeof x.from === 'string' && typeof x.to === 'string'
  }
  if (!d.nodes.every(okNode) || !d.edges.every(okEdge)) return null
  const now = new Date().toISOString()
  return cleanGroups<DiagramDoc>({
    v: 1,
    id: typeof d.id === 'string' ? d.id : '',
    title: typeof d.title === 'string' ? d.title : '',
    notation: d.notation && ['uml', 'bpmn', 'arch', 'flow', 'free'].includes(d.notation) ? d.notation : 'uml',
    roomCode: typeof d.roomCode === 'string' ? d.roomCode : '',
    nodes: d.nodes.map((n) => ({ ...n, name: typeof n.name === 'string' ? n.name : '', props: n.props ?? {} })),
    edges: d.edges.map((e) => ({ ...e, label: typeof e.label === 'string' ? e.label : '' })),
    strokes: Array.isArray(d.strokes) ? d.strokes.filter((s) => s && Array.isArray(s.points)) : [],
    ...(Array.isArray(d.groups)
      ? {
          groups: d.groups
            .filter((g) => g && typeof g.id === 'string' && Array.isArray(g.nodes) && Array.isArray(g.strokes))
            .map((g) => ({ id: g.id, name: typeof g.name === 'string' ? g.name : '', nodes: g.nodes.filter((x) => typeof x === 'string'), strokes: g.strokes.filter((x) => typeof x === 'string') })),
        }
      : {}),
    createdAt: typeof d.createdAt === 'string' ? d.createdAt : now,
    updatedAt: now,
  })
}
