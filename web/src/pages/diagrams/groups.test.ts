import { describe, expect, it } from 'vitest'
import { parseJson, toBpmn, toJson, toPlantUml, toXmi } from './exporters'
import { nodeBox } from './geometry'
import {
  alignPick,
  cleanGroups,
  distributePick,
  expandPick,
  groupPick,
  nextGroupName,
  pickIsOneGroup,
  togglePick,
  ungroupPick,
  unitsOf,
} from './groups'
import { DiagramDoc, DNode, emptyDoc, makeNode, NodeProps, NodeType } from './model'
import { wellFormed } from './xmlBemFormado.test-util'

const N = (id: string, type: NodeType, x: number, y: number, name = id, props: NodeProps = {}): DNode => makeNode(type, x, y, name, props, id)

function uml(): DiagramDoc {
  return {
    ...emptyDoc('g1', 'Núcleo', 'uml', '', '2026-09-17T00:00:00Z'),
    nodes: [N('s', 'class', 0, 40, 'Session'), N('r', 'class', 300, 48, 'Recording'), N('t', 'class', 520, 60, 'Transcript'), N('x', 'class', 0, 400, 'Solta')],
    strokes: [{ id: 'k1', points: [10, 10, 60, 20], color: '#000', width: 2 }],
  }
}

describe('grupos de selecção · operações', () => {
  it('agrupar três classes cria um grupo com nome; agrupar o mesmo grupo outra vez não faz nada', () => {
    const r = groupPick(uml(), { nodes: ['s', 'r', 't'], strokes: [] }, 'Núcleo de media', 'G')!
    expect(r.doc.groups).toEqual([{ id: 'G', name: 'Núcleo de media', nodes: ['s', 'r', 't'], strokes: [] }])
    expect(pickIsOneGroup(r.doc, { nodes: ['t', 's', 'r'], strokes: [] })?.id).toBe('G')
    expect(groupPick(r.doc, { nodes: ['s', 'r', 't'], strokes: [] }, 'outra')).toBeNull()
    expect(groupPick(uml(), { nodes: ['s'], strokes: [] }, 'só um')).toBeNull()
  })

  it('escolher um membro escolhe o grupo inteiro; ⇧clique junta e tira o grupo como um todo', () => {
    const d = groupPick(uml(), { nodes: ['s', 'r'], strokes: ['k1'] }, 'A', 'G')!.doc
    expect(expandPick(d, { nodes: ['r'], strokes: [] })).toEqual({ nodes: ['r', 's'], strokes: ['k1'] })
    const junta = togglePick(d, { nodes: ['x'], strokes: [] }, 'node', 's')
    expect(junta.nodes.sort()).toEqual(['r', 's', 'x'])
    expect(junta.strokes).toEqual(['k1'])
    expect(togglePick(d, junta, 'stroke', 'k1')).toEqual({ nodes: ['x'], strokes: [] })
  })

  it('agrupar parte de um grupo tira-a de lá; o grupo que fica com um membro desaparece', () => {
    const d = groupPick(uml(), { nodes: ['s', 'r'], strokes: [] }, 'A', 'A')!.doc
    const e = groupPick(d, { nodes: ['r', 't'], strokes: [] }, 'B', 'B')!.doc
    expect(e.groups).toEqual([{ id: 'B', name: 'B', nodes: ['r', 't'], strokes: [] }])
  })

  it('desagrupar desfaz só os grupos tocados; sem grupo devolve null', () => {
    let d = groupPick(uml(), { nodes: ['s', 'r'], strokes: [] }, 'A', 'A')!.doc
    d = groupPick(d, { nodes: ['t', 'x'], strokes: [] }, 'B', 'B')!.doc
    expect(ungroupPick(d, { nodes: ['r'], strokes: [] })!.groups!.map((g) => g.id)).toEqual(['B'])
    expect(ungroupPick(uml(), { nodes: ['s'], strokes: [] })).toBeNull()
  })

  it('apagar um membro limpa o grupo, e um grupo com menos de dois desaparece', () => {
    const d = groupPick(uml(), { nodes: ['s', 'r'], strokes: [] }, 'A', 'A')!.doc
    expect(cleanGroups({ ...d, nodes: d.nodes.filter((n) => n.id !== 's') }).groups).toEqual([])
    expect(cleanGroups(d)).toBe(d)
  })

  it('nome livre', () => {
    const d = groupPick(uml(), { nodes: ['s', 'r'], strokes: [] }, 'Grupo 1', 'A')!.doc
    expect(nextGroupName(d, (n) => `Grupo ${n}`)).toBe('Grupo 2')
  })

  it('alinhar à esquerda trata o grupo como um bloco (não o desmancha)', () => {
    const d = groupPick(uml(), { nodes: ['r', 't'], strokes: [] }, 'A', 'A')!.doc
    const pick = { nodes: ['s', 'r', 't'], strokes: [] }
    expect(unitsOf(d, pick).map((u) => u.id).sort()).toEqual(['g:A', 'n:s'])
    const a = alignPick(d, pick, 'left')
    const x = (id: string) => a.nodes.find((n) => n.id === id)!.x
    expect(x('s')).toBe(0)
    expect(x('r')).toBe(0)
    // O grupo mexeu-se inteiro: a distância entre r e t mantém-se.
    expect(x('t') - x('r')).toBe(220)
  })

  it('distribuir na horizontal iguala os espaços entre bordas', () => {
    const d = { ...uml(), nodes: [N('a', 'class', 0, 0), N('b', 'class', 180, 0), N('c', 'class', 600, 0)] }
    const r = distributePick(d, { nodes: ['a', 'b', 'c'], strokes: [] }, 'x')
    const [a, b, c] = ['a', 'b', 'c'].map((id) => nodeBox(r.nodes.find((n) => n.id === id)!))
    expect(Math.abs(b.x - (a.x + a.w) - (c.x - (b.x + b.w)))).toBeLessThanOrEqual(1)
  })
})

describe('grupos de selecção · exportações', () => {
  const agrupado = () => groupPick(uml(), { nodes: ['s', 'r', 't'], strokes: [] }, 'Núcleo <media>', 'G')!.doc

  it('XMI: extensão da ferramenta com os membros por xmi:idref, e o modelo UML igual', () => {
    const sem = toXmi(uml())
    const com = toXmi(agrupado())
    expect(wellFormed(com).why).toBeUndefined()
    expect(com).toContain('<xmi:Extension extender="Delonix Meet">')
    expect(com).toContain('<group xmi:id="grp_G" name="Núcleo &lt;media&gt;"><member xmi:idref="c_s"/><member xmi:idref="c_r"/><member xmi:idref="c_t"/></group>')
    // Nada de pacote novo: o que está dentro de <uml:Model> não muda.
    const modelo = (x: string) => x.slice(x.indexOf('<uml:Model'), x.indexOf('</uml:Model>'))
    expect(modelo(com)).toBe(modelo(sem))
    expect(sem).not.toContain('xmi:Extension')
  })

  it('PlantUML: together { … } com as classes do grupo', () => {
    const p = toPlantUml(agrupado())
    expect(p).toContain("' group Núcleo <media>: Session, Recording, Transcript")
    expect(p).toMatch(/\ntogether \{\n {2}class "Session" as E\d+ \{\n {2}\}\n {2}class "Recording"[\s\S]*?\n\}\nclass "Solta"/)
    // Membros em pacotes diferentes: comentário sempre; together só dentro do mesmo pacote.
    const d = agrupado()
    d.nodes = d.nodes.map((n) => (n.id === 's' ? { ...n, props: { ...n.props, package: 'core' } } : n.id === 'r' || n.id === 't' ? { ...n, props: { ...n.props, package: 'media' } } : n))
    const q = toPlantUml(d)
    expect(q).toMatch(/package "media" \{\n {2}together \{\n {4}class "Recording"[\s\S]*?class "Transcript"[\s\S]*?\n {2}\}\n\}/)
    expect(q).toMatch(/package "core" \{\n {2}class "Session"/)
  })

  it('BPMN: bpmn:group com categoria e forma no diagrama', () => {
    const base: DiagramDoc = {
      ...emptyDoc('b1', 'Arranque', 'bpmn', '', '2026-09-17T00:00:00Z'),
      nodes: [N('ini', 'startEvent', 0, 20, 'Início'), N('t1', 'task', 80, 6, 'Iniciar reunião'), N('t2', 'task', 80, 62, 'Abrir estúdio'), N('t3', 'task', 240, 34, 'Emitir para 4 destinos')],
    }
    const d = groupPick(base, { nodes: ['t1', 't2', 't3'], strokes: [] }, 'Arranque da emissão', 'G')!.doc
    const x = toBpmn(d)
    const r = wellFormed(x)
    expect(r.why).toBeUndefined()
    expect(new Set(r.ids).size).toBe(r.ids.length)
    expect(x).toContain('<bpmn:category id="SelCategory_G"><bpmn:categoryValue id="SelCategoryValue_G" value="Arranque da emissão"/></bpmn:category>')
    expect(x).toContain('<bpmn:group id="SelGroup_G" categoryValueRef="SelCategoryValue_G"/>')
    expect(x).toMatch(/<bpmndi:BPMNShape id="SelGroup_G_di" bpmnElement="SelGroup_G"><dc:Bounds x="68" y="-6" width="\d+" height="\d+"\/><\/bpmndi:BPMNShape>/)
    // O grupo vai para dentro do processo, antes de ele fechar.
    expect(x.indexOf('<bpmn:group id="SelGroup_G"')).toBeLessThan(x.indexOf('</bpmn:process>'))
  })

  it('JSON: os grupos voltam ao importar, e referências mortas caem', () => {
    const d = agrupado()
    expect(parseJson(toJson(d))!.groups).toEqual(d.groups)
    const lixo = JSON.parse(toJson(d))
    lixo.groups.push({ id: 'Z', name: 'fantasma', nodes: ['nao-existe', 's'], strokes: [] }, { id: 7 })
    expect(parseJson(JSON.stringify(lixo))!.groups!.map((g) => g.id)).toEqual(['G'])
  })
})
