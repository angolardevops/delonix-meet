/**
 * A transcrição do servidor entra no projecto ALINHADA com o que ficou na linha
 * de tempo: um trecho cortado não reaparece nas legendas, e o que vem depois de
 * um corte chega mais cedo exactamente o que o corte tirou.
 */
import { describe, expect, it } from 'vitest'
import { editar, novoProjecto } from '../edit/projecto'
import type { Fonte, Projecto } from '../edit/projecto'
import { cuesDoServidor, fonteDaBiblioteca, segmentosDasCues } from './servidor'

function daBiblioteca(duracao = 60): Projecto {
  const f: Fonte = { id: 'f1', nome: 'aula.webm', tipo: 'av', origem: 'completo', duracao, largura: 1280, altura: 720, bytes: 1, criadaEm: 0, gravacao: 'rec-1' }
  let p = novoProjecto('Aula', 0)
  p = editar(p, { tipo: 'fonte', fonte: f }, 1)
  return editar(p, { tipo: 'inserir', fonteId: 'f1', faixa: 'V1', inicio: 0 }, 2)
}
const seg = (a: number, b: number, text: string) => ({ start_ms: a, end_ms: b, text, confidence: null })

describe('transcrição do servidor', () => {
  it('encontra a fonte que veio da biblioteca', () => {
    expect(fonteDaBiblioteca(daBiblioteca())).toEqual({ fonteId: 'f1', gravacao: 'rec-1' })
    expect(fonteDaBiblioteca(novoProjecto('x', 0))).toBeNull()
  })

  it('sem cortes, os tempos são os da gravação', () => {
    const p = daBiblioteca()
    const cues = cuesDoServidor(p, 'f1', [seg(0, 4000, 'Bom dia a todos.'), seg(5000, 9000, 'Hoje falamos de SIP.')], 'Ana')
    expect(cues.map((c) => c.texto)).toEqual(['Bom dia a todos.', 'Hoje falamos de SIP.'])
    expect(cues[0].inicio).toBeCloseTo(0)
    expect(cues[1].inicio).toBeCloseTo(5)
    expect(cues[1].fim).toBeCloseTo(9)
    expect(cues[0].orador).toBe('Ana')
  })

  it('um trecho cortado não reaparece e o seguinte chega mais cedo', () => {
    let p = daBiblioteca()
    p = editar(p, { tipo: 'cortar-intervalos', intervalos: [{ inicio: 4.5, fim: 9.5 }] }, 3)
    const cues = cuesDoServidor(p, 'f1', [seg(0, 4000, 'Fica.'), seg(5000, 9000, 'Foi cortado.'), seg(10000, 12000, 'Depois do corte.')])
    expect(cues.map((c) => c.texto)).toEqual(['Fica.', 'Depois do corte.'])
    expect(cues[1].inicio).toBeCloseTo(5)
  })

  it('ida e volta para os pedidos de IA em milissegundos', () => {
    const cues = cuesDoServidor(daBiblioteca(), 'f1', [seg(1500, 3000, 'Olá.')])
    expect(segmentosDasCues(cues)).toEqual([{ start_ms: 1500, end_ms: 3000, text: 'Olá.', confidence: null }])
  })
})
