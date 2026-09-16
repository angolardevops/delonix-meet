import { describe, expect, it } from 'vitest'
import { escapeXml, fileBase, parseJson, toBpmn, toJson, toPlantUml, toXmi, wrapSvg } from './exporters'
import { anchorPoint, contentBox, fitView, laneOf, nodeBox } from './geometry'
import { DEdge, DiagramDoc, DNode, emptyDoc, makeNode, NodeProps, NodeType } from './model'

const N = (id: string, type: NodeType, x: number, y: number, name = id, props: NodeProps = {}): DNode => makeNode(type, x, y, name, props, id)
const E = (id: string, type: DEdge['type'], from: string, to: string, extra: Partial<DEdge> = {}): DEdge => ({ id, type, from, to, label: '', ...extra })

/**
 * Verificação de XML bem formado sem DOM: etiquetas equilibradas, atributos
 * entre aspas, entidades conhecidas e ids únicos. Não substitui um parser,
 * mas apanha exactamente o que um exportador por concatenação costuma partir.
 */
function wellFormed(xml: string): { ok: boolean; why?: string; ids: string[] } {
  const body = xml.replace(/^<\?xml[^?]*\?>\s*/, '')
  const stack: string[] = []
  const ids: string[] = []
  const tag = /<(\/?)([A-Za-z_][\w:.-]*)((?:\s+[\w:.-]+="[^"<]*")*)\s*(\/?)>|([^<]+)/gy
  let m: RegExpExecArray | null
  let pos = 0
  while (pos < body.length) {
    tag.lastIndex = pos
    m = tag.exec(body)
    if (!m) return { ok: false, why: `lixo em ${pos}: ${body.slice(pos, pos + 40)}`, ids }
    pos = tag.lastIndex
    if (m[5] !== undefined) {
      if (/&(?!amp;|lt;|gt;|quot;|apos;)/.test(m[5])) return { ok: false, why: `entidade em texto: ${m[5].slice(0, 40)}`, ids }
      continue
    }
    const [, close, name, attrs, selfClose] = m
    if (/&(?!amp;|lt;|gt;|quot;|apos;)/.test(attrs)) return { ok: false, why: `entidade em atributo de ${name}`, ids }
    for (const a of attrs.matchAll(/\s(?:xmi:)?id="([^"]*)"/g)) ids.push(a[1])
    if (close) {
      const top = stack.pop()
      if (top !== name) return { ok: false, why: `fecha ${name}, aberto ${top}`, ids }
    } else if (!selfClose) stack.push(name)
  }
  if (stack.length) return { ok: false, why: `por fechar: ${stack.join(',')}`, ids }
  return { ok: true, ids }
}

function modeloUml(): DiagramDoc {
  return {
    ...emptyDoc('m1', 'Modelo de domínio <Meet> & "gravação"', 'uml', '', '2026-09-16T00:00:00Z'),
    nodes: [
      N('s', 'class', 0, 0, 'Session', { stereotype: 'entity', package: 'meet.core', attributes: ['− id: UUID', '− roomCode: String'], operations: ['+ start(): void', '+ publishStream(dest: StreamTarget): Stream'] }),
      N('r', 'class', 300, 0, 'Recording', { package: 'meet.media', attributes: ['− sizeBytes: Long'] }),
      N('t', 'interface', 300, 250, 'StreamTarget', { operations: ['+ connect(): Health'] }),
      N('k', 'class', 300, 400, 'RtmpTarget'),
      N('en', 'enum', 600, 0, 'SessionType', { attributes: ['MEETING', 'WEBINAR'] }),
      N('nt', 'note', 600, 200, '', { text: 'A dobragem gera um Transcript por idioma' }),
      N('l1', 'lifeline', 0, 600, 'Participant'),
      N('l2', 'lifeline', 200, 600, 'MeetGateway'),
      N('f', 'fragment', -20, 670, 'sso', { operator: 'opt' }),
      N('a', 'actor', 700, 600, 'Anfitrião'),
      N('b', 'boundary', 800, 580, 'Delonix Meet'),
      N('u1', 'usecase', 850, 620, 'Emitir para vários canais'),
      N('u2', 'usecase', 850, 700, 'Autenticar'),
    ],
    edges: [
      E('e1', 'composition', 's', 'r', { srcMult: '1', dstMult: '0..*', label: 'grava' }),
      E('e2', 'realization', 'k', 't'),
      E('e3', 'anchor', 'nt', 'r'),
      E('m1', 'message', 'l1', 'l2', { label: 'join(roomCode, sso)', offset: 60 }),
      E('m2', 'reply', 'l2', 'l1', { label: 'token', offset: 100 }),
      E('m0', 'message', 'l1', 'l2', { label: 'hello()', offset: 20 }),
      E('ua', 'association', 'a', 'u1'),
      E('ui', 'include', 'u1', 'u2'),
      E('dep', 'dependency', 's', 't'),
    ],
  }
}

describe('XMI', () => {
  const xmi = toXmi(modeloUml())

  it('é XML bem formado, com ids únicos, e escapa o que a pessoa escreveu', () => {
    const r = wellFormed(xmi)
    expect(r.why).toBeUndefined()
    expect(new Set(r.ids).size).toBe(r.ids.length)
    expect(xmi).toContain('name="Modelo de domínio &lt;Meet&gt; &amp; &quot;gravação&quot;"')
  })

  it('classes dentro dos pacotes, atributos com visibilidade e tipos referenciados', () => {
    expect(xmi).toMatch(/<packagedElement xmi:type="uml:Package" xmi:id="[^"]+" name="meet.core">\s*<packagedElement xmi:type="uml:Class" xmi:id="c_s" name="Session">/)
    expect(xmi).toContain('<ownedAttribute xmi:type="uml:Property" xmi:id="c_s_a0" name="id" visibility="private" type="dt_1"/>')
    expect(xmi).toContain('<packagedElement xmi:type="uml:DataType" xmi:id="dt_1" name="UUID"/>')
    // Um tipo que é uma classe do modelo referencia a classe, não um DataType.
    expect(xmi).toContain('name="dest" direction="in" type="c_t"')
    expect(xmi).toContain('<packagedElement xmi:type="uml:Interface" xmi:id="c_t" name="StreamTarget">')
    expect(xmi).toContain('<ownedLiteral xmi:type="uml:EnumerationLiteral" xmi:id="c_en_l1" name="WEBINAR"/>')
  })

  it('composição: agregação no extremo da parte, multiplicidades nos dois', () => {
    expect(xmi).toContain('<ownedEnd xmi:type="uml:Property" xmi:id="as_e1_dst" type="c_r" association="as_e1" aggregation="composite">')
    expect(xmi).toContain('<upperValue xmi:type="uml:LiteralUnlimitedNatural" xmi:id="as_e1_dst_hi" value="*"/>')
    expect(xmi).toContain('<lowerValue xmi:type="uml:LiteralInteger" xmi:id="as_e1_src_lo" value="1"/>')
  })

  it('realização, dependência, nota, casos de uso e interacção', () => {
    expect(xmi).toContain('<interfaceRealization xmi:type="uml:InterfaceRealization" xmi:id="r_e2" client="c_k" supplier="c_t" contract="c_t"/>')
    expect(xmi).toContain('<packagedElement xmi:type="uml:Dependency" xmi:id="d_dep" client="c_s" supplier="c_t"/>')
    expect(xmi).toContain('body="A dobragem gera um Transcript por idioma"><annotatedElement xmi:idref="c_r"/>')
    expect(xmi).toContain('<packagedElement xmi:type="uml:UseCase" xmi:id="u_u1" name="Emitir para vários canais" subject="u_b"><include xmi:type="uml:Include" xmi:id="i_ui" addition="u_u2"/>')
    expect(xmi).toContain('<packagedElement xmi:type="uml:Actor" xmi:id="u_a" name="Anfitrião"/>')
    // Mensagens pela ordem vertical, não pela ordem de criação.
    const order = [...xmi.matchAll(/<message xmi:type="uml:Message" xmi:id="msg_(\w+)"/g)].map((m) => m[1])
    expect(order).toEqual(['m0', 'm1', 'm2'])
    expect(xmi).toContain('messageSort="reply"')
    expect(xmi).toContain('interactionOperator="opt" covered="ll_l1 ll_l2"')
  })
})

describe('PlantUML', () => {
  const pu = toPlantUml(modeloUml())

  it('três blocos: classes, sequência e casos de uso', () => {
    expect(pu.match(/@startuml/g)).toHaveLength(3)
    expect(pu.match(/@enduml/g)).toHaveLength(3)
  })

  it('classes com estereótipo, membros e setas UML', () => {
    expect(pu).toContain('package "meet.core" {')
    expect(pu).toMatch(/class "Session" as E1 <<entity>> \{\n {4}-id : UUID\n {4}-roomCode : String\n {4}\+start\(\) : void/)
    expect(pu).toContain('E1 "1" *-- "0..*" E2 : grava')
    expect(pu).toContain('E4 ..|> E3')
    expect(pu).toContain('enum "SessionType" as E5 {')
    expect(pu).toContain('note "A dobragem gera um Transcript por idioma" as E6')
  })

  it('sequência ordenada, com o fragmento à volta das mensagens que cobre', () => {
    const seq = pu.split('@enduml')[1]
    const lines = seq.split('\n').map((l) => l.trim()).filter(Boolean)
    const i0 = lines.indexOf('E7 -> E8 : hello()')
    const iOpt = lines.indexOf('opt sso')
    const i1 = lines.indexOf('E7 -> E8 : join(roomCode, sso)')
    const i2 = lines.indexOf('E8 --> E7 : token')
    expect([i0, iOpt, i1, i2].every((i) => i >= 0)).toBe(true)
    expect(i0 < iOpt && iOpt < i1 && i1 < i2).toBe(true)
    expect(lines.filter((l) => l === 'end')).toHaveLength(1)
  })

  it('casos de uso com fronteira e include', () => {
    expect(pu).toContain('actor "Anfitrião" as E10')
    expect(pu).toMatch(/rectangle "Delonix Meet" \{\n {2}usecase "Emitir para vários canais" as E12/)
    expect(pu).toContain('E12 ..> E13 : <<include>>')
    expect(pu).toContain('E10 --> E12')
  })
})

describe('BPMN 2.0', () => {
  function processo(): DiagramDoc {
    return {
      ...emptyDoc('b1', 'Aula aberta com emissão', 'bpmn', '', '2026-09-16T00:00:00Z'),
      nodes: [
        N('pool', 'pool', 0, 0, 'Delonix Meet', { lanes: [{ id: 'la', name: 'Formadora', size: 180 }, { id: 'lb', name: 'Técnico', size: 160 }] }),
        N('s', 'startEvent', 60, 60, 'Aula agendada', { trigger: 'message' }),
        N('v', 'task', 140, 50, 'Verificar dispositivos', { taskKind: 'user' }),
        N('g', 'gateway', 300, 55, 'Tipo de sessão?'),
        N('e', 'task', 400, 40, 'Emitir para 4 destinos', { taskKind: 'service', implementation: 'meet.studio.open', multiInstance: 'parallel' }),
        N('w', 'task', 400, 220, 'Vigiar saúde dos canais'),
        N('tm', 'intermediateEvent', 600, 60, 'Fim do tempo', { trigger: 'timer' }),
        N('end', 'endEvent', 700, 60, ''),
        N('ext', 'pool', 0, 500, 'YouTube'),
        N('ann', 'annotation', 900, 100, '', { text: 'Emissão a 4K' }),
      ],
      edges: [
        E('f1', 'sequenceFlow', 's', 'v'),
        E('f2', 'sequenceFlow', 'v', 'g'),
        E('f3', 'sequenceFlow', 'g', 'e', { label: 'emissão', condition: "tipo == 'emissao' && x < 3" }),
        E('f4', 'sequenceFlow', 'g', 'w', { isDefault: true }),
        E('f5', 'sequenceFlow', 'e', 'tm'),
        E('f6', 'sequenceFlow', 'tm', 'end'),
        E('f7', 'sequenceFlow', 'w', 'end'),
        E('mf', 'messageFlow', 'e', 'ext', { label: 'RTMP' }),
      ],
    }
  }
  const xml = toBpmn(processo())

  it('é XML bem formado e com ids únicos', () => {
    const r = wellFormed(xml)
    expect(r.why).toBeUndefined()
    expect(new Set(r.ids).size).toBe(r.ids.length)
  })

  it('colaboração com participantes e fluxo de mensagem para a outra piscina', () => {
    expect(xml).toContain('<bpmn:participant id="Participant_pool" name="Delonix Meet" processRef="Process_pool"/>')
    expect(xml).toContain('<bpmn:messageFlow id="F_mf" name="RTMP" sourceRef="N_e" targetRef="Participant_ext"/>')
  })

  it('pistas com as referências dos elementos que lá estão', () => {
    expect(xml).toMatch(/<bpmn:lane id="Lane_la" name="Formadora">(<bpmn:flowNodeRef>N_\w+<\/bpmn:flowNodeRef>)+<\/bpmn:lane>/)
    expect(xml).toContain('<bpmn:flowNodeRef>N_w</bpmn:flowNodeRef></bpmn:lane>')
    const formadora = /<bpmn:lane id="Lane_la"[^]*?<\/bpmn:lane>/.exec(xml)![0]
    expect(formadora).not.toContain('N_w<')
  })

  it('tipos de tarefa, gateway por omissão, multi-instância, executor, condição e eventos', () => {
    expect(xml).toContain('<bpmn:userTask id="N_v" name="Verificar dispositivos">')
    expect(xml).toContain('<bpmn:exclusiveGateway id="N_g" name="Tipo de sessão?" default="F_f4">')
    expect(xml).toContain('<delonix:executor>meet.studio.open</delonix:executor>')
    expect(xml).toContain('<bpmn:multiInstanceLoopCharacteristics isSequential="false"/>')
    expect(xml).toContain('<bpmn:conditionExpression xsi:type="bpmn:tFormalExpression">tipo == &apos;emissao&apos; &amp;&amp; x &lt; 3</bpmn:conditionExpression>')
    expect(xml).toContain('<bpmn:messageEventDefinition id="N_s_def"/>')
    expect(xml).toContain('<bpmn:intermediateCatchEvent id="N_tm" name="Fim do tempo">')
    expect(xml).toContain('<bpmn:timerEventDefinition id="N_tm_def"/>')
    expect(xml).toContain('<bpmn:textAnnotation id="N_ann"><bpmn:text>Emissão a 4K</bpmn:text></bpmn:textAnnotation>')
  })

  it('diagrama DI: formas com Bounds e arestas com dois pontos', () => {
    expect(xml).toContain('<bpmndi:BPMNPlane id="Plane_b1" bpmnElement="Collaboration_b1">')
    expect(xml).toContain('<bpmndi:BPMNShape id="Participant_pool_di" bpmnElement="Participant_pool" isHorizontal="true"><dc:Bounds x="0" y="0" width="760" height="340"/>')
    expect(xml).toContain('<bpmndi:BPMNShape id="Lane_lb_di" bpmnElement="Lane_lb" isHorizontal="true"><dc:Bounds x="30" y="180" width="730" height="160"/>')
    expect(xml.match(/<bpmndi:BPMNEdge /g)).toHaveLength(8)
    expect(xml).toMatch(/<bpmndi:BPMNEdge id="F_f1_di" bpmnElement="F_f1"><di:waypoint x="\d+" y="\d+"\/><di:waypoint x="\d+" y="\d+"\/><\/bpmndi:BPMNEdge>/)
  })

  it('sem piscinas: um só processo e o plano aponta para ele', () => {
    const d: DiagramDoc = { ...emptyDoc('x', 'X', 'bpmn'), nodes: [N('s', 'startEvent', 0, 0), N('e', 'endEvent', 100, 0)], edges: [E('f', 'sequenceFlow', 's', 'e')] }
    const out = toBpmn(d)
    expect(wellFormed(out).ok).toBe(true)
    expect(out).not.toContain('bpmn:collaboration')
    expect(out).toContain('bpmnElement="Process_x"')
  })
})

describe('SVG, JSON e nomes', () => {
  it('SVG autónomo com viewBox à volta do conteúdo e título escapado', () => {
    const svg = wrapSvg('<rect x="0" y="0" width="10" height="10"/>', { x: 10, y: 20, w: 100, h: 50 }, 'A & B', 10)
    expect(svg).toContain('viewBox="0 10 120 70"')
    expect(svg).toContain('<title>A &amp; B</title>')
    expect(wellFormed(svg).ok).toBe(true)
  })

  it('JSON vai e volta; um JSON qualquer é recusado', () => {
    const d = modeloUml()
    const back = parseJson(toJson(d))!
    expect(back.nodes).toEqual(d.nodes)
    expect(back.edges).toEqual(d.edges)
    expect(parseJson('{"nodes": []}')).toBeNull()
    expect(parseJson('isto não é json')).toBeNull()
    expect(parseJson(JSON.stringify({ format: 'delonix-diagram', v: 1, nodes: [{ id: 'x', type: 'bomba', x: 0, y: 0 }], edges: [] }))).toBeNull()
  })

  it('nome de ficheiro sem acentos nem símbolos', () => {
    expect(fileBase('Processo de aula aberta · emissão!')).toBe('processo-de-aula-aberta-emissao')
    expect(fileBase('···')).toBe('diagrama')
    expect(escapeXml(`<a href='x'>&</a>`)).toBe('&lt;a href=&apos;x&apos;&gt;&amp;&lt;/a&gt;')
  })
})

describe('geometria', () => {
  it('a seta sai do contorno certo em rectângulo, elipse e losango', () => {
    const r = N('r', 'process', 0, 0)
    expect(anchorPoint(r, { x: 1000, y: 30 })).toEqual({ x: 150, y: 30 })
    const e = N('e', 'startEvent', 0, 0)
    const p = anchorPoint(e, { x: 18, y: 1000 })
    expect(p.x).toBeCloseTo(18)
    expect(p.y).toBeCloseTo(36)
    const d = N('d', 'gateway', 0, 0)
    const q = anchorPoint(d, { x: 1000, y: 25 })
    expect(q).toEqual({ x: 50, y: 25 })
  })

  it('a altura de uma classe cresce com os membros; a caixa do conteúdo e o enquadramento', () => {
    const c0 = N('c', 'class', 0, 0, 'A')
    const c1 = N('c', 'class', 0, 0, 'A', { attributes: ['- a: X', '- b: Y', '- c: Z'] })
    expect(nodeBox(c1).h).toBeGreaterThan(nodeBox(c0).h)
    const box = contentBox({ nodes: [N('a', 'process', 100, 100), N('b', 'process', 400, 300)], strokes: [] })!
    expect(box).toEqual({ x: 100, y: 100, w: 450, h: 260 })
    const v = fitView(box, 1000, 600)
    expect(v.k).toBeGreaterThan(0.2)
    expect(v.x + box.x * v.k).toBeGreaterThanOrEqual(0)
  })

  it('pista de um elemento pela posição vertical', () => {
    const pool = N('p', 'pool', 0, 0, 'P', { lanes: [{ id: 'a', name: 'A', size: 100 }, { id: 'b', name: 'B', size: 100 }] })
    const doc = { nodes: [pool, N('t', 'task', 100, 120)] }
    expect(laneOf(doc, doc.nodes[1])?.lane.id).toBe('b')
  })
})
