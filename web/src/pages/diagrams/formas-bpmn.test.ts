import { describe, expect, it } from 'vitest'
import { toBpmn } from './exporters'
import { DEdge, DiagramDoc, DNode, emptyDoc, makeNode, NodeProps, NodeType, TRIGGERS } from './model'
import { applyFix, validate } from './validate'
import { wellFormed } from './xmlBemFormado.test-util'

const N = (id: string, type: NodeType, x: number, y: number, name = id, props: NodeProps = {}): DNode => makeNode(type, x, y, name, props, id)
const E = (id: string, type: DEdge['type'], from: string, to: string, extra: Partial<DEdge> = {}): DEdge => ({ id, type, from, to, label: '', ...extra })
const doc = (nodes: DNode[], edges: DEdge[] = []): DiagramDoc => ({ ...emptyDoc('d1', 'Encomenda', 'bpmn', '', '2026-09-17T00:00:00Z'), nodes, edges })
const codes = (d: DiagramDoc) => validate(d).map((i) => i.code).sort()

/** Processo com os elementos novos todos a funcionar juntos. */
function processo(): DiagramDoc {
  return doc(
    [
      N('s', 'startEvent', 0, 100, 'Pedido', { trigger: 'timer' }),
      N('in', 'dataObject', 0, 0, 'Pedido', { dataRole: 'input' }),
      N('out', 'dataObject', 900, 0, 'Recibo', { dataRole: 'output', collection: true }),
      N('t', 'task', 80, 86, 'Validar', { taskKind: 'businessRule', loop: true }),
      // Evento de fronteira na borda de baixo da tarefa (tarefa 130×64 em y 86 → base 150).
      N('b', 'intermediateEvent', 180, 132, 'Prazo', { trigger: 'timer', boundary: true, nonInterrupting: true }),
      N('comp', 'task', 180, 230, 'Desfazer', { compensation: true }),
      N('g', 'gateway', 260, 93, 'Tipo?', { gatewayKind: 'complex' }),
      N('call', 'task', 360, 30, 'Faturar', { taskKind: 'call' }),
      N('ad', 'subProcess', 360, 150, 'Rever', { adHoc: true }),
      N('ge', 'gateway', 560, 93, '', { gatewayKind: 'eventInstantiate' }),
      N('m', 'intermediateEvent', 660, 40, 'Pago', { trigger: 'message' }),
      N('c', 'intermediateEvent', 660, 160, 'Stock', { trigger: 'conditional' }),
      N('lt', 'intermediateEvent', 740, 40, 'L1', { trigger: 'link', throwing: true }),
      N('lc', 'intermediateEvent', 740, 160, 'L1', { trigger: 'link' }),
      N('esc', 'intermediateEvent', 800, 160, 'Escalar', { trigger: 'escalation', throwing: true }),
      N('e1', 'endEvent', 880, 160, 'Cancelada', { trigger: 'terminate' }),
      N('e2', 'endEvent', 880, 300, 'Falhou', { trigger: 'error' }),
      N('ds', 'dataStore', 400, 300, 'ERP'),
      N('grp', 'group', 340, 0, 'Financeiro'),
    ],
    [
      E('f1', 'sequenceFlow', 's', 't'),
      E('f2', 'sequenceFlow', 't', 'g'),
      E('f3', 'sequenceFlow', 'g', 'call', { condition: 'fatura' }),
      E('f4', 'sequenceFlow', 'g', 'ad', { isDefault: true }),
      E('f5', 'sequenceFlow', 'call', 'ge'),
      E('f6', 'sequenceFlow', 'ad', 'ge'),
      E('f7', 'sequenceFlow', 'ge', 'm'),
      E('f8', 'sequenceFlow', 'ge', 'c'),
      E('f9', 'sequenceFlow', 'm', 'lt'),
      E('f10', 'sequenceFlow', 'lc', 'esc'),
      E('f11', 'sequenceFlow', 'esc', 'e1'),
      E('f12', 'sequenceFlow', 'c', 'e2'),
      E('f13', 'sequenceFlow', 'b', 'e2'),
      E('a1', 'dataAssociation', 'ds', 'call'),
    ],
  )
}

describe('BPMN · gatilhos permitidos', () => {
  it('cada tipo de evento aceita só os gatilhos da especificação', () => {
    expect(TRIGGERS.startEvent).not.toContain('error')
    expect(TRIGGERS.startEvent).not.toContain('terminate')
    expect(TRIGGERS.endEvent).toContain('terminate')
    expect(TRIGGERS.endEvent).not.toContain('timer')
    expect(TRIGGERS.intermediateEvent).not.toContain('none')
    expect(TRIGGERS.intermediateEvent).toContain('link')
  })
})

describe('BPMN · validação dos elementos novos', () => {
  it('o processo completo não tem problemas', () => {
    expect(validate(processo())).toEqual([])
  })

  it('evento de fronteira solto e com entrada (esta corrige-se)', () => {
    const d = processo()
    d.nodes = d.nodes.map((n) => (n.id === 'b' ? { ...n, x: 600, y: 400 } : n))
    d.edges.push(E('bad', 'sequenceFlow', 'call', 'b'))
    const got = codes(d)
    expect(got).toContain('bpmnFronteiraSolta')
    expect(got).toContain('bpmnFronteiraComEntrada')
    const fix = validate(d).find((i) => i.code === 'bpmnFronteiraComEntrada')!
    expect(applyFix(d, fix).edges.some((e) => e.id === 'bad')).toBe(false)
  })

  it('gateway que nem divide nem junta, gateway de eventos com uma saída e ligação sem par', () => {
    const d = processo()
    d.edges = d.edges.filter((e) => e.id !== 'f8' && e.id !== 'f4' && e.id !== 'f6')
    d.nodes = d.nodes.map((n) => (n.id === 'lc' ? { ...n, name: 'L2' } : n))
    const got = codes(d)
    expect(got).toContain('bpmnGatewayInutil')
    expect(got).toContain('bpmnGatewayEventosSaidas')
    expect(got.filter((c) => c === 'bpmnLigacaoSemPar')).toHaveLength(2)
  })

  it('um gateway de eventos não leva a um evento que lança', () => {
    const d = processo()
    d.nodes = d.nodes.map((n) => (n.id === 'm' ? { ...n, props: { ...n.props, throwing: true } } : n))
    expect(codes(d)).toContain('bpmnGatewayEventos')
  })
})

describe('BPMN · exportação dos elementos novos', () => {
  const xml = toBpmn(processo())

  it('é XML bem formado com ids únicos', () => {
    const r = wellFormed(xml)
    expect(r.why).toBeUndefined()
    expect(new Set(r.ids).size).toBe(r.ids.length)
  })

  it('eventos: fronteira não interruptiva, lançar/apanhar e definições tipadas', () => {
    expect(xml).toMatch(/<bpmn:boundaryEvent id="N_b" name="Prazo" attachedToRef="N_t" cancelActivity="false">.*<bpmn:timerEventDefinition id="N_b_def"\/><\/bpmn:boundaryEvent>/)
    expect(xml).toMatch(/<bpmn:intermediateThrowEvent id="N_lt" name="L1">.*<bpmn:linkEventDefinition id="N_lt_def" name="L1"\/>/)
    expect(xml).toMatch(/<bpmn:intermediateCatchEvent id="N_lc" name="L1">.*<bpmn:linkEventDefinition id="N_lc_def" name="L1"\/>/)
    expect(xml).toContain('<bpmn:conditionalEventDefinition id="N_c_def"><bpmn:condition xsi:type="bpmn:tFormalExpression"/></bpmn:conditionalEventDefinition>')
    expect(xml).toMatch(/<bpmn:intermediateThrowEvent id="N_esc" name="Escalar">.*<bpmn:escalationEventDefinition id="N_esc_def"\/>/)
    expect(xml).toContain('<bpmn:terminateEventDefinition id="N_e1_def"/>')
    expect(xml).toContain('<bpmn:errorEventDefinition id="N_e2_def"/>')
  })

  it('actividades: regra de negócio com ciclo, chamada, ad-hoc e compensação', () => {
    expect(xml).toMatch(/<bpmn:businessRuleTask id="N_t" name="Validar">.*<bpmn:standardLoopCharacteristics\/><\/bpmn:businessRuleTask>/)
    expect(xml).toContain('<bpmn:callActivity id="N_call" name="Faturar">')
    expect(xml).toContain('<bpmn:adHocSubProcess id="N_ad" name="Rever">')
    expect(xml).toContain('<bpmn:task id="N_comp" name="Desfazer" isForCompensation="true"/>')
  })

  it('gateways: complexo com omissão e eventos de arranque', () => {
    expect(xml).toContain('<bpmn:complexGateway id="N_g" name="Tipo?" default="F_f4">')
    expect(xml).toMatch(/<bpmn:eventBasedGateway id="N_ge" instantiate="true" eventGatewayType="Exclusive">/)
  })

  it('dados: ioSpecification antes do resto, armazém, colecção e grupo com categoria', () => {
    const proc = xml.slice(xml.indexOf('<bpmn:process'))
    expect(proc).toMatch(/^<bpmn:process id="Process_d1" isExecutable="false">\n\s*<bpmn:ioSpecification id="IO_Process_d1">/)
    expect(xml).toContain('<bpmn:dataInput id="N_in" name="Pedido"/>')
    expect(xml).toContain('<bpmn:dataOutput id="N_out" name="Recibo" isCollection="true"/>')
    expect(xml).toContain('<bpmn:inputSet id="InputSet_Process_d1"><bpmn:dataInputRefs>N_in</bpmn:dataInputRefs></bpmn:inputSet>')
    expect(xml).toContain('<bpmn:dataStoreReference id="N_ds" name="ERP"/>')
    expect(xml).toContain('<bpmn:category id="Category_grp"><bpmn:categoryValue id="CategoryValue_grp" value="Financeiro"/></bpmn:category>')
    expect(xml).toContain('<bpmn:group id="N_grp" categoryValueRef="CategoryValue_grp"/>')
    // Tudo o que é desenhado tem forma no DI.
    for (const id of ['N_b', 'N_ds', 'N_grp', 'N_in', 'N_out']) expect(xml).toContain(`bpmnElement="${id}"`)
  })
})
