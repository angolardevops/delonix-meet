import { describe, expect, it } from 'vitest'
import { parseTransition, toPlantUml, toXmi } from './exporters'
import { contentBox, edgeSegment } from './geometry'
import { canConnect, DEdge, DiagramDoc, DNode, emptyDoc, makeNode, NodeProps, NodeType } from './model'
import { applyFix, validate } from './validate'
import { wellFormed } from './xmlBemFormado.test-util'

const N = (id: string, type: NodeType, x: number, y: number, name = id, props: NodeProps = {}): DNode => makeNode(type, x, y, name, props, id)
const E = (id: string, type: DEdge['type'], from: string, to: string, extra: Partial<DEdge> = {}): DEdge => ({ id, type, from, to, label: '', ...extra })
const doc = (nodes: DNode[], edges: DEdge[] = []): DiagramDoc => ({ ...emptyDoc('d1', 'Formas UML', 'uml', '', '2026-09-17T00:00:00Z'), nodes, edges })
const codes = (d: DiagramDoc) => validate(d).map((i) => i.code).sort()

function actividade(): DiagramDoc {
  return doc(
    [
      N('p1', 'partition', 0, 0, 'Anfitrião'),
      N('i', 'initialNode', 90, 40, ''),
      N('a1', 'action', 40, 100, 'Abrir sala'),
      N('d', 'decisionNode', 90, 190, 'Gravar?'),
      N('a2', 'action', 40, 260, 'Gravar'),
      N('f', 'forkNode', 300, 260, ''),
      N('o', 'objectNode', 300, 320, 'Recording'),
      N('fim', 'activityFinal', 320, 400, ''),
    ],
    [
      E('c1', 'controlFlow', 'i', 'a1'),
      E('c2', 'controlFlow', 'a1', 'd'),
      E('c3', 'controlFlow', 'd', 'a2', { condition: 'sim' }),
      E('c4', 'controlFlow', 'd', 'f', { condition: 'não' }),
      E('c5', 'controlFlow', 'a2', 'o'),
      E('c6', 'controlFlow', 'f', 'fim'),
    ],
  )
}

function estados(): DiagramDoc {
  return doc(
    [
      N('s0', 'stateInitial', 0, 0, ''),
      N('comp', 'compositeState', 100, 0, 'Em chamada', {}),
      N('s1', 'state', 130, 60, 'Ligada', { attributes: ['entry / tocar()', 'do / medir()'] }),
      N('h', 'history', 320, 60, '', { deep: true }),
      N('ch', 'choice', 600, 40, 'rede?'),
      N('s2', 'state', 700, 0, 'Suspensa'),
      N('sf', 'stateFinal', 900, 20, ''),
    ],
    [
      E('t1', 'transition', 's0', 'comp'),
      E('t2', 'transition', 'comp', 'ch', { label: 'perda [rtt > 500] / avisar()' }),
      E('t3', 'transition', 'ch', 's2', { label: '[má]' }),
      E('t4', 'transition', 'ch', 'comp', { label: '[boa]' }),
      E('t5', 'transition', 's2', 'sf', { label: 'desligar' }),
    ],
  )
}

function estrutura(): DiagramDoc {
  return doc(
    [
      N('srv', 'deviceNode', 0, 0, 'meet-01', { stereotype: 'device' }),
      N('jar', 'artifact', 20, 60, 'meet-api.jar'),
      N('api', 'component', 400, 0, 'MeetApi'),
      N('pt', 'port', 573, 28, 'http'),
      N('pi', 'providedInterface', 650, 20, 'Rooms'),
      N('ri', 'requiredInterface', 650, 90, 'Storage'),
      N('db', 'deviceNode', 0, 300, 'db-01', { stereotype: 'executionEnvironment' }),
      N('cls', 'class', 900, 0, 'Session', { attributes: ['- id: UUID'] }),
      N('obj', 'object', 900, 200, 's1', { instanceOf: 'Session', attributes: ['id = 42'] }),
      N('obj2', 'object', 1100, 200, 's2', { instanceOf: 'Session' }),
    ],
    [
      E('dp', 'deploy', 'jar', 'srv'),
      E('mf', 'manifest', 'jar', 'api'),
      E('rz', 'realization', 'pt', 'pi'),
      E('us', 'usage', 'api', 'ri'),
      E('cp', 'association', 'srv', 'db', { label: 'TCP/5432' }),
      E('lk', 'link', 'obj', 'obj2'),
    ],
  )
}

describe('UML · ligações novas', () => {
  it('só liga o que a especificação permite', () => {
    const [i, a, fim] = [N('i', 'initialNode', 0, 0), N('a', 'action', 0, 0), N('f', 'activityFinal', 0, 0)]
    expect(canConnect('controlFlow', i, a)).toBe(true)
    expect(canConnect('controlFlow', a, i)).toBe(false)
    expect(canConnect('controlFlow', fim, a)).toBe(false)
    const [s, sf, si] = [N('s', 'state', 0, 0), N('sf', 'stateFinal', 0, 0), N('si', 'stateInitial', 0, 0)]
    expect(canConnect('transition', s, s)).toBe(true)
    expect(canConnect('transition', sf, s)).toBe(false)
    expect(canConnect('transition', s, si)).toBe(false)
    expect(canConnect('controlFlow', s, a)).toBe(false)
    expect(canConnect('deploy', N('x', 'artifact', 0, 0), N('y', 'deviceNode', 0, 0))).toBe(true)
    expect(canConnect('deploy', N('y', 'deviceNode', 0, 0), N('x', 'artifact', 0, 0))).toBe(false)
    const l = N('l', 'lifeline', 0, 0)
    expect(canConnect('lostMessage', l, l)).toBe(true)
    expect(canConnect('lostMessage', l, N('l2', 'lifeline', 0, 0))).toBe(false)
  })

  it('mensagem perdida sai da linha de vida para a direita e entra no enquadramento', () => {
    const l = N('l', 'lifeline', 100, 0, 'A', { length: 200 })
    const e = E('m', 'lostMessage', 'l', 'l', { offset: 50 })
    const seg = edgeSegment({ nodes: [l] }, e)!
    expect(seg.a).toEqual({ x: 163, y: 84 })
    expect(seg.b.x).toBeGreaterThan(seg.a.x)
    const box = contentBox({ nodes: [l], strokes: [], edges: [e] } as unknown as DiagramDoc)!
    expect(box.x + box.w).toBeGreaterThanOrEqual(seg.b.x)
  })

  it('transição: evento, guarda e efeito', () => {
    expect(parseTransition('perda [rtt > 500] / avisar()')).toEqual({ trigger: 'perda', guard: 'rtt > 500', effect: 'avisar()' })
    expect(parseTransition('[boa]')).toEqual({ trigger: '', guard: 'boa', effect: '' })
    expect(parseTransition('desligar')).toEqual({ trigger: 'desligar', guard: '', effect: '' })
  })
})

describe('UML · validação de actividades e estados', () => {
  it('uma actividade e uma máquina de estados coerentes não têm avisos', () => {
    expect(validate(actividade())).toEqual([])
    expect(validate(estados())).toEqual([])
  })

  it('actividade sem inicial, decisão com uma saída e saídas sem guarda', () => {
    const d = actividade()
    d.nodes = d.nodes.filter((n) => n.id !== 'i')
    d.edges = d.edges.filter((e) => e.id !== 'c1' && e.id !== 'c4').map((e) => (e.id === 'c3' ? { ...e, condition: '' } : e))
    expect(codes(d)).toEqual(['umlActividadeSemInicio', 'umlDecisaoSaidas'])
    const d2 = actividade()
    d2.edges = d2.edges.map((e) => (e.id === 'c3' ? { ...e, condition: '' } : e))
    expect(codes(d2)).toEqual(['umlDecisaoGuarda'])
  })

  it('inicial com entrada e final com saída corrigem-se retirando os fluxos', () => {
    const d = actividade()
    d.edges.push(E('bad1', 'controlFlow', 'a2', 'i'))
    // Um fluxo que sai de um final não é ligável: a regra geral também o apanha.
    d.edges.push(E('bad2', 'controlFlow', 'fim', 'a1'))
    const issues = validate(d)
    const inicial = issues.find((i) => i.code === 'umlInicialComEntrada')!
    const final = issues.find((i) => i.code === 'umlFinalComSaida')!
    expect(inicial.fixable && final.fixable).toBe(true)
    const fixed = applyFix(applyFix(d, inicial), final)
    expect(fixed.edges.some((e) => e.id === 'bad1' || e.id === 'bad2')).toBe(false)
  })

  it('estados sem inicial, escolha com uma saída e estado inalcançável', () => {
    const d = estados()
    d.edges = d.edges.filter((e) => e.id !== 't4')
    d.nodes.push(N('solto', 'state', 1200, 300, 'Órfão'))
    expect(codes(d)).toEqual(['umlEscolhaSaidas', 'umlEstadoInalcancavel'])
    const semInicial = estados()
    semInicial.nodes = semInicial.nodes.filter((n) => n.id !== 's0')
    semInicial.edges = semInicial.edges.filter((e) => e.id !== 't1')
    expect(codes(semInicial)).toEqual(['umlEstadosSemInicial'])
  })
})

describe('UML · XMI dos diagramas novos', () => {
  it('actividade: nós tipados, partição, guarda e fluxo de objecto', () => {
    const xmi = toXmi(actividade())
    const r = wellFormed(xmi)
    expect(r.why).toBeUndefined()
    expect(new Set(r.ids).size).toBe(r.ids.length)
    expect(xmi).toContain('<packagedElement xmi:type="uml:Activity" xmi:id="act_d1" name="Formas UML">')
    expect(xmi).toContain('<node xmi:type="uml:InitialNode" xmi:id="an_i" inPartition="ap_p1" outgoing="cf_c1"/>')
    expect(xmi).toContain('xmi:type="uml:DecisionNode" xmi:id="an_d" name="Gravar?"')
    expect(xmi).toContain('<node xmi:type="uml:ForkNode" xmi:id="an_f"')
    expect(xmi).toContain('<node xmi:type="uml:CentralBufferNode" xmi:id="an_o" name="Recording"')
    expect(xmi).toContain('<node xmi:type="uml:ActivityFinalNode" xmi:id="an_fim"')
    expect(xmi).toContain('<guard xmi:type="uml:LiteralString" xmi:id="cf_c3_g" value="sim"/>')
    expect(xmi).toContain('<edge xmi:type="uml:ObjectFlow" xmi:id="cf_c5" source="an_a2" target="an_o">')
    expect(xmi).toMatch(/<group xmi:type="uml:ActivityPartition" xmi:id="ap_p1" name="Anfitrião" node="[^"]*an_a1/)
  })

  it('estados: pseudo-estados, região do composto, actividades internas e transição completa', () => {
    const xmi = toXmi(estados())
    const r = wellFormed(xmi)
    expect(r.why).toBeUndefined()
    expect(new Set(r.ids).size).toBe(r.ids.length)
    expect(xmi).toContain('<subvertex xmi:type="uml:Pseudostate" xmi:id="sv_s0" kind="initial"/>')
    expect(xmi).toContain('<subvertex xmi:type="uml:Pseudostate" xmi:id="sv_ch" name="rede?" kind="choice"/>')
    expect(xmi).toContain('<subvertex xmi:type="uml:FinalState" xmi:id="sv_sf"/>')
    // O estado e o histórico ficam DENTRO da região do composto.
    expect(xmi).toMatch(/xmi:id="sv_comp" name="Em chamada">\s*<region xmi:type="uml:Region" xmi:id="sv_comp_r">\s*<subvertex xmi:type="uml:State" xmi:id="sv_s1" name="Ligada"><entry xmi:type="uml:OpaqueBehavior" xmi:id="sv_s1_b0" name="tocar\(\)"\/><doActivity/)
    expect(xmi).toContain('kind="deepHistory"')
    expect(xmi).toContain('<trigger xmi:type="uml:Trigger" xmi:id="tr_t2_t" name="perda"/><guard xmi:type="uml:Constraint" xmi:id="tr_t2_g"><specification xmi:type="uml:OpaqueExpression" xmi:id="tr_t2_gs"><body>rtt &gt; 500</body></specification></guard><effect xmi:type="uml:OpaqueBehavior" xmi:id="tr_t2_e" name="avisar()"/>')
  })

  it('componentes, implantação e objectos', () => {
    const xmi = toXmi(estrutura())
    const r = wellFormed(xmi)
    expect(r.why).toBeUndefined()
    expect(new Set(r.ids).size).toBe(r.ids.length)
    expect(xmi).toContain('<packagedElement xmi:type="uml:Component" xmi:id="k_api" name="MeetApi"><ownedAttribute xmi:type="uml:Port" xmi:id="k_pt" name="http"/><interfaceRealization xmi:type="uml:InterfaceRealization" xmi:id="r_rz" client="k_api" supplier="k_pi" contract="k_pi"/></packagedElement>')
    expect(xmi).toContain('<packagedElement xmi:type="uml:Usage" xmi:id="d_us" client="k_api" supplier="k_ri"/>')
    expect(xmi).toContain('<packagedElement xmi:type="uml:Device" xmi:id="k_srv" name="meet-01"><deployment xmi:type="uml:Deployment" xmi:id="dp_dp" client="k_srv" supplier="k_jar" deployedArtifact="k_jar"/></packagedElement>')
    expect(xmi).toContain('xmi:type="uml:ExecutionEnvironment" xmi:id="k_db"')
    expect(xmi).toContain('<manifestation xmi:type="uml:Manifestation" xmi:id="mf_mf" client="k_jar" supplier="k_api" utilizedElement="k_api"/>')
    expect(xmi).toContain('<packagedElement xmi:type="uml:CommunicationPath" xmi:id="cp_cp" name="TCP/5432"')
    expect(xmi).not.toContain('xmi:type="uml:Association" xmi:id="as_cp"')
    expect(xmi).toContain('<packagedElement xmi:type="uml:InstanceSpecification" xmi:id="o_obj" name="s1" classifier="c_cls"><slot xmi:type="uml:Slot" xmi:id="o_obj_s0" definingFeature="c_cls_a0">')
  })

  it('sequência: perdida, encontrada e barra de activação', () => {
    const d = doc(
      [N('l1', 'lifeline', 0, 0, 'A'), N('l2', 'lifeline', 200, 0, 'B'), N('act', 'activation', 256, 60)],
      [E('m1', 'message', 'l1', 'l2', { label: 'ping()', offset: 30 }), E('lo', 'lostMessage', 'l2', 'l2', { label: 'eco', offset: 60 }), E('fo', 'foundMessage', 'l1', 'l1', { label: 'alarme', offset: 90 })],
    )
    const xmi = toXmi(d)
    expect(wellFormed(xmi).why).toBeUndefined()
    expect(xmi).toContain('messageKind="lost" sendEvent="msg_lo_s"/>')
    expect(xmi).toContain('messageKind="found" receiveEvent="msg_fo_r"/>')
    expect(xmi).not.toContain('xmi:id="msg_lo_r"')
    expect(xmi).toContain('<fragment xmi:type="uml:BehaviorExecutionSpecification" xmi:id="ex_act" covered="ll_l2" start="ex_act_start" finish="ex_act_finish"/>')
    const pu = toPlantUml(d)
    expect(pu).toContain('E2 ->x] : eco')
    expect(pu).toContain('[o-> E1 : alarme')
  })
})

describe('UML · PlantUML dos diagramas novos', () => {
  it('actividade e estados em sintaxe de estados, com pseudo-estados e guardas', () => {
    const pu = toPlantUml(actividade())
    expect(pu).toContain('@startuml formas-uml_actividade')
    expect(pu).toContain('state E4 <<choice>>')
    expect(pu).toContain('[*] --> E3')
    expect(pu).toContain('E4 --> E5 : [sim]')
    expect(pu).toContain('E6 --> [*]')
    const st = toPlantUml(estados())
    expect(st).toMatch(/state "Em chamada" as E2 \{\n {2}state "Ligada" as E3\n {2}E3 : entry \/ tocar\(\)/)
    expect(st).toContain('state E4 <<history*>>')
    expect(st).toContain('E2 --> E5 : perda [rtt > 500] / avisar()')
  })

  it('componentes dentro dos nós, lollipop, uso, implantação e objectos', () => {
    const pu = toPlantUml(estrutura())
    expect(pu).toMatch(/node "meet-01" as E1 <<device>> \{\n {2}artifact "meet-api.jar" as E2\n\}/)
    expect(pu).toMatch(/component "MeetApi" as E3 \{\n {2}port "http" as E4\n\}/)
    expect(pu).toContain('E4 - E5')
    expect(pu).toContain('E3 ..> E6 : <<use>>')
    expect(pu).toContain('E2 ..> E1 : <<deploy>>')
    expect(pu).toMatch(/object "s1 : Session" as E9 \{\n {2}id = 42\n\}/)
    expect(pu).toContain('E9 -- E10')
  })
})
