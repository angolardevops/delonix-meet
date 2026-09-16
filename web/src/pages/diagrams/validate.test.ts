import { describe, expect, it } from 'vitest'
import { DEdge, DiagramDoc, DNode, emptyDoc, isValidMultiplicity, makeNode, normalizeMultiplicity, NodeProps, NodeType, parseMember } from './model'
import { applyFix, fixAll, validate } from './validate'

const N = (id: string, type: NodeType, x: number, y: number, name = id, props: NodeProps = {}): DNode => makeNode(type, x, y, name, props, id)
const E = (id: string, type: DEdge['type'], from: string, to: string, extra: Partial<DEdge> = {}): DEdge => ({ id, type, from, to, label: '', ...extra })
const doc = (notation: DiagramDoc['notation'], nodes: DNode[], edges: DEdge[] = []): DiagramDoc => ({
  ...emptyDoc('d1', 'Teste', notation, '', '2026-09-16T00:00:00Z'),
  nodes,
  edges,
})
const codes = (d: DiagramDoc) => validate(d).map((i) => i.code).sort()

describe('sintaxe UML', () => {
  it('lê atributos e operações com visibilidade', () => {
    expect(parseMember('− language: Locale')).toMatchObject({ visibility: '-', name: 'language', type: 'Locale', isOperation: false })
    expect(parseMember('+ publishStream(): Stream')).toMatchObject({ visibility: '+', name: 'publishStream', type: 'Stream', isOperation: true })
    expect(parseMember('start(a: Int, b)')).toMatchObject({ visibility: '', name: 'start', params: 'a: Int, b', type: '' })
    expect(parseMember('# count = 3')).toMatchObject({ name: 'count', type: '' })
  })

  it('multiplicidades: válidas, inválidas e a forma canónica', () => {
    for (const ok of ['1', '*', '0..1', '1..*', '2..5', '']) expect(isValidMultiplicity(ok)).toBe(true)
    for (const bad of ['5..2', '*..1', 'a', '1..', '1...3']) expect(isValidMultiplicity(bad)).toBe(false)
    expect(normalizeMultiplicity('5..2')).toBe('2..5')
    expect(normalizeMultiplicity(' 1 .. 1 ')).toBe('1')
    expect(normalizeMultiplicity('0..n')).toBe('0..*')
    expect(normalizeMultiplicity('1...3')).toBe('1..3')
    expect(normalizeMultiplicity('abc')).toBeNull()
  })
})

describe('validação UML', () => {
  it('um modelo coerente não tem avisos', () => {
    const d = doc(
      'uml',
      [
        N('s', 'class', 0, 0, 'Session', { attributes: ['- id: UUID'], operations: ['+ start(): void'] }),
        N('r', 'class', 300, 0, 'Recording', { attributes: ['- sizeBytes: Long'] }),
        N('t', 'interface', 300, 200, 'StreamTarget'),
        N('k', 'class', 300, 400, 'RtmpTarget'),
      ],
      [E('e1', 'composition', 's', 'r', { srcMult: '1', dstMult: '0..*', label: 'grava' }), E('e2', 'realization', 'k', 't')],
    )
    expect(validate(d)).toEqual([])
  })

  it('nome vazio, nome duplicado no mesmo pacote e atributo sem tipo', () => {
    const d = doc('uml', [
      N('a', 'class', 0, 0, ''),
      N('b', 'class', 0, 0, 'X', { package: 'meet' }),
      N('c', 'class', 0, 0, 'X', { package: 'meet' }),
      N('d', 'class', 0, 0, 'X', { package: 'outro', attributes: ['- semTipo'] }),
    ])
    expect(codes(d)).toEqual(['umlAtributoSemTipo', 'umlNomeDuplicado', 'umlNomeVazio'])
  })

  it('ciclo de herança é erro e não tem correcção automática', () => {
    const d = doc('uml', [N('a', 'class', 0, 0), N('b', 'class', 0, 0), N('c', 'class', 0, 0)], [
      E('1', 'generalization', 'a', 'b'),
      E('2', 'generalization', 'b', 'c'),
      E('3', 'generalization', 'c', 'a'),
    ])
    const cyc = validate(d).filter((i) => i.code === 'umlHerancaCiclo')
    expect(cyc).toHaveLength(1)
    expect(cyc[0].fixable).toBe(false)
    expect(applyFix(d, cyc[0])).toBe(d)
  })

  it('herança de classe para interface corrige-se para realização', () => {
    const d = doc('uml', [N('a', 'class', 0, 0), N('i', 'interface', 0, 200)], [E('g', 'generalization', 'a', 'i')])
    const [is] = validate(d)
    expect(is).toMatchObject({ code: 'umlHerancaParaInterface', fixable: true })
    const fixed = applyFix(d, is)
    expect(fixed.edges[0].type).toBe('realization')
    expect(validate(fixed)).toEqual([])
  })

  it('realização para uma classe corrige-se para herança', () => {
    const d = doc('uml', [N('a', 'class', 0, 0), N('b', 'class', 0, 200)], [E('r', 'realization', 'a', 'b')])
    const { doc: fixed, fixed: n } = fixAll(d)
    expect(n).toBe(1)
    expect(fixed.edges[0].type).toBe('generalization')
  })

  it('multiplicidade inválida: corrige quando há forma canónica, avisa quando não há', () => {
    const d = doc('uml', [N('a', 'class', 0, 0), N('b', 'class', 300, 0)], [
      E('x', 'association', 'a', 'b', { srcMult: '5..2', dstMult: 'muitos' }),
    ])
    const issues = validate(d).filter((i) => i.code === 'umlMultiplicidade')
    expect(issues.map((i) => i.fixable)).toEqual([true, false])
    const { doc: fixed } = fixAll(d)
    expect(fixed.edges[0]).toMatchObject({ srcMult: '2..5', dstMult: 'muitos' })
  })

  it('ligação que a notação não admite: muda para a por omissão ou sai', () => {
    const d = doc('uml', [N('a', 'class', 0, 0), N('b', 'class', 300, 0), N('l', 'lifeline', 0, 400), N('u', 'usecase', 0, 600)], [
      E('m', 'message', 'a', 'b'),
      E('i', 'include', 'l', 'u'),
    ])
    expect(codes(d)).toEqual(['ligacaoInvalida', 'ligacaoInvalida'])
    const { doc: fixed } = fixAll(d)
    expect(fixed.edges).toEqual([expect.objectContaining({ id: 'm', type: 'association' })])
  })

  it('aresta duplicada sai; herança para si próprio sai; aresta órfã sai', () => {
    const d = doc('uml', [N('a', 'class', 0, 0), N('b', 'class', 300, 0)], [
      E('1', 'association', 'a', 'b'),
      E('2', 'association', 'a', 'b'),
      E('3', 'generalization', 'a', 'a'),
      E('4', 'dependency', 'a', 'fantasma'),
    ])
    const { doc: fixed } = fixAll(d)
    expect(fixed.edges.map((e) => e.id)).toEqual(['1'])
  })

  it('actor sem casos de uso e mensagem sem nome avisam', () => {
    const d = doc('uml', [N('act', 'actor', 0, 0, 'Anfitrião'), N('l1', 'lifeline', 0, 0), N('l2', 'lifeline', 200, 0)], [E('m', 'message', 'l1', 'l2')])
    expect(codes(d)).toEqual(['umlActorIsolado', 'umlMensagemSemNome'])
  })

  it('só valida a notação pedida', () => {
    const d = doc('uml', [N('a', 'class', 0, 0, ''), N('t', 'task', 0, 0, '')])
    expect(validate(d, 'uml').map((i) => i.code)).toEqual(['umlNomeVazio'])
    expect(validate(d, 'bpmn').map((i) => i.code)).toContain('bpmnTarefaSemNome')
  })
})

describe('validação BPMN', () => {
  /** O processo do template, simplificado: início → verificar → gateway → (reunião | estúdio) → fim. */
  function aula(): DiagramDoc {
    return doc(
      'bpmn',
      [
        N('pool', 'pool', 0, 0, 'Delonix Meet', { lanes: [{ id: 'l1', name: 'Formadora', size: 200 }, { id: 'l2', name: 'Técnico', size: 200 }] }),
        N('s', 'startEvent', 60, 80, 'Aula agendada', { trigger: 'message' }),
        N('v', 'task', 140, 70, 'Verificar dispositivos', { taskKind: 'user' }),
        N('g', 'gateway', 300, 75, 'Tipo de sessão?', { gatewayKind: 'exclusive' }),
        N('m', 'task', 400, 20, 'Iniciar reunião', { taskKind: 'service' }),
        N('e', 'task', 400, 120, 'Abrir estúdio', { taskKind: 'service', implementation: 'meet.studio.open' }),
        N('end', 'endEvent', 600, 80, ''),
      ],
      [
        E('f1', 'sequenceFlow', 's', 'v'),
        E('f2', 'sequenceFlow', 'v', 'g'),
        E('f3', 'sequenceFlow', 'g', 'm', { label: 'reunião' }),
        E('f4', 'sequenceFlow', 'g', 'e', { label: 'emissão' }),
        E('f5', 'sequenceFlow', 'm', 'end'),
        E('f6', 'sequenceFlow', 'e', 'end'),
      ],
    )
  }

  it('gateway sem saída por omissão: o aviso do template, e a correcção marca a primeira sem condição', () => {
    const d = aula()
    const issues = validate(d)
    expect(issues.map((i) => i.code)).toEqual(['bpmnGatewaySemOmissao'])
    const fixed = applyFix(d, issues[0])
    expect(fixed.edges.find((e) => e.id === 'f3')?.isDefault).toBe(true)
    expect(validate(fixed)).toEqual([])
  })

  it('se todas as saídas têm condição, não há escolha determinística e não se corrige', () => {
    const d = aula()
    d.edges = d.edges.map((e) => (e.id === 'f3' || e.id === 'f4' ? { ...e, condition: `tipo == '${e.label}'` } : e))
    const [is] = validate(d)
    expect(is).toMatchObject({ code: 'bpmnGatewaySemOmissao', fixable: false })
  })

  it('sem início e sem fim: acrescenta os eventos quando há um único candidato', () => {
    const d = doc('bpmn', [N('a', 'task', 100, 100, 'A'), N('b', 'task', 300, 100, 'B')], [E('f', 'sequenceFlow', 'a', 'b')])
    expect(codes(d)).toEqual(['bpmnSemEntrada', 'bpmnSemFim', 'bpmnSemInicio', 'bpmnSemSaida'])
    const { doc: fixed } = fixAll(d, { start: 'Início', end: 'Fim' })
    expect(fixed.nodes.map((n) => n.type)).toEqual(['task', 'task', 'startEvent', 'endEvent'])
    expect(fixed.nodes[2].name).toBe('Início')
    expect(validate(fixed)).toEqual([])
    // Determinístico: a mesma entrada dá o mesmo modelo.
    expect(fixAll(d, { start: 'Início', end: 'Fim' }).doc).toEqual(fixed)
  })

  it('dois candidatos a início: avisa sem inventar', () => {
    const d = doc('bpmn', [N('a', 'task', 0, 0, 'A'), N('b', 'task', 300, 0, 'B'), N('end', 'endEvent', 600, 0)], [
      E('1', 'sequenceFlow', 'a', 'end'),
      E('2', 'sequenceFlow', 'b', 'end'),
    ])
    const semInicio = validate(d).find((i) => i.code === 'bpmnSemInicio')
    expect(semInicio?.fixable).toBe(false)
  })

  it('início com entrada e fim com saída perdem essas setas', () => {
    const d = doc('bpmn', [N('s', 'startEvent', 0, 0), N('t', 'task', 100, 0, 'T'), N('e', 'endEvent', 300, 0)], [
      E('1', 'sequenceFlow', 's', 't'),
      E('2', 'sequenceFlow', 't', 'e'),
      E('3', 'sequenceFlow', 'e', 's'),
    ])
    expect(codes(d)).toEqual(['bpmnFimComSaida', 'bpmnInicioComEntrada'])
    expect(fixAll(d).doc.edges.map((e) => e.id)).toEqual(['1', '2'])
  })

  it('condição à saída de gateway paralela, omissão com condição e várias omissões', () => {
    const d = doc(
      'bpmn',
      [N('s', 'startEvent', 0, 0), N('p', 'gateway', 100, 0, 'P', { gatewayKind: 'parallel' }), N('a', 'task', 200, 0, 'A'), N('b', 'task', 200, 100, 'B'), N('x', 'gateway', 300, 0, 'X'), N('e', 'endEvent', 400, 0)],
      [
        E('1', 'sequenceFlow', 's', 'p'),
        E('2', 'sequenceFlow', 'p', 'a', { condition: 'x > 1' }),
        E('3', 'sequenceFlow', 'p', 'b'),
        E('4', 'sequenceFlow', 'a', 'x'),
        E('5', 'sequenceFlow', 'b', 'x'),
        E('6', 'sequenceFlow', 'x', 'e', { isDefault: true, condition: 'ok' }),
        E('7', 'sequenceFlow', 'x', 'e', { isDefault: true }),
      ],
    )
    expect(codes(d)).toEqual(['bpmnCondicaoEmParalelo', 'bpmnOmissaoComCondicao', 'bpmnVariasOmissao'])
    const { doc: fixed } = fixAll(d)
    expect(validate(fixed)).toEqual([])
    expect(fixed.edges.find((e) => e.id === '6')).toMatchObject({ isDefault: true, condition: '' })
    expect(fixed.edges.find((e) => e.id === '7')?.isDefault).toBe(false)
  })

  it('fluxo de sequência entre piscinas passa a fluxo de mensagem, e o contrário', () => {
    const d = doc(
      'bpmn',
      [
        N('p1', 'pool', 0, 0, 'A'),
        N('p2', 'pool', 0, 400, 'B'),
        N('s1', 'startEvent', 60, 100),
        N('t1', 'task', 160, 90, 'T1'),
        N('e1', 'endEvent', 400, 100),
        N('s2', 'startEvent', 60, 500),
        N('t2', 'task', 160, 490, 'T2'),
        N('e2', 'endEvent', 400, 500),
      ],
      [
        E('a', 'sequenceFlow', 's1', 't1'),
        E('b', 'sequenceFlow', 't1', 'e1'),
        E('c', 'sequenceFlow', 's2', 't2'),
        E('d', 'sequenceFlow', 't2', 'e2'),
        E('x', 'sequenceFlow', 't1', 't2'),
        E('y', 'messageFlow', 's1', 'e1'),
      ],
    )
    expect(codes(d)).toEqual(['bpmnFluxoEntrePiscinas', 'bpmnMensagemMesmaPiscina'])
    const { doc: fixed } = fixAll(d)
    expect(fixed.edges.find((e) => e.id === 'x')?.type).toBe('messageFlow')
    expect(fixed.edges.find((e) => e.id === 'y')?.type).toBe('sequenceFlow')
  })

  it('elemento fora da piscina e gateway de eventos seguida de tarefa', () => {
    const d = doc(
      'bpmn',
      [N('p', 'pool', 0, 0, 'A'), N('s', 'startEvent', 60, 100), N('g', 'gateway', 150, 90, 'G', { gatewayKind: 'eventBased' }), N('t', 'task', 250, 90, 'T'), N('e', 'endEvent', 900, 900)],
      [E('1', 'sequenceFlow', 's', 'g'), E('2', 'sequenceFlow', 'g', 't'), E('3', 'sequenceFlow', 't', 'e')],
    )
    // O fim solto forma um processo à parte — e esse não tem início.
    expect(codes(d)).toEqual(['bpmnForaDaPiscina', 'bpmnGatewayEventos', 'bpmnSemFim', 'bpmnSemInicio'])
  })
})

describe('validação de arquitectura e fluxograma', () => {
  it('arquitectura: componente sem nome e isolado', () => {
    const d = doc('arch', [N('a', 'service', 0, 0, ''), N('b', 'database', 200, 0, 'pg'), N('c', 'queue', 400, 0, 'q')], [E('1', 'sync', 'a', 'b')])
    expect(codes(d)).toEqual(['archIsolado', 'archSemNome'])
  })

  it('fluxograma: decisão com uma saída, saída sem etiqueta e nó inalcançável', () => {
    const d = doc(
      'flow',
      [N('s', 'terminator', 0, 0, 'Início'), N('d', 'decision', 0, 100, 'OK?'), N('p', 'process', 0, 200, 'Faz'), N('o', 'process', 300, 200, 'Solto')],
      [E('1', 'flow', 's', 'd'), E('2', 'flow', 'd', 'p')],
    )
    expect(codes(d)).toEqual(['flowDecisaoEtiquetas', 'flowDecisaoSaidas', 'flowInalcancavel'])
  })

  it('fluxograma sem terminador de início', () => {
    const d = doc('flow', [N('p', 'process', 0, 0, 'A'), N('q', 'process', 0, 100, 'B')], [E('1', 'flow', 'p', 'q'), E('2', 'flow', 'q', 'p')])
    expect(codes(d)).toEqual(['flowSemInicio'])
  })
})
