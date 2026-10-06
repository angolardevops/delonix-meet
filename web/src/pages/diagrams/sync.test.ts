import { describe, expect, it } from 'vitest'
import { compara, planeia } from './sync'

const d = (id: string, updatedAt: string) => ({ id, updatedAt })

describe('sincronização de diagramas', () => {
  it('quem só existe num lado vai para a fila desse lado', () => {
    expect(compara(d('a', '2026-10-06T10:00:00.000Z'), undefined)).toBe('so-local')
    expect(compara(undefined, d('a', '2026-10-06T10:00:00.000Z'))).toBe('so-servidor')
    expect(compara(undefined, undefined)).toBe('igual')
  })

  it('ganha o mais recente, e o empate não mexe em nada', () => {
    const velho = d('a', '2026-10-06T10:00:00.000Z')
    const novo = d('a', '2026-10-06T11:00:00.000Z')
    expect(compara(novo, velho)).toBe('enviar')
    expect(compara(velho, novo)).toBe('descarregar')
    // Tratar o empate como «o servidor ganha» punha um GET e um PUT em cada
    // abertura de cada diagrama.
    expect(compara(velho, { ...velho })).toBe('igual')
  })

  it('o mesmo instante escrito de outra maneira é um empate, não uma gravação', () => {
    // É isto que o servidor devolve: o Postgres guarda microssegundos e o
    // browser não. Sem o arredondamento ao milissegundo, cada gravação ficava
    // eternamente «por enviar».
    expect(compara(d('a', '2026-10-06T10:00:00.000Z'), d('a', '2026-10-06T10:00:00.000Z'))).toBe('igual')
    // E o fuso não decide: a comparação é por instante, não por texto.
    expect(compara(d('a', '2026-10-06T12:00:00.000+02:00'), d('a', '2026-10-06T10:00:00.000Z'))).toBe('igual')
  })

  it('uma data ilegível não decide nada — não apaga nem escreve por cima', () => {
    expect(compara(d('a', 'ontem'), d('a', '2026-10-06T10:00:00.000Z'))).toBe('igual')
    expect(compara(d('a', '2026-10-06T10:00:00.000Z'), d('a', ''))).toBe('igual')
  })

  it('o plano de uma lista mostra o melhor de cada lado e separa as duas filas', () => {
    const p = planeia(
      [d('igual', '2026-10-06T10:00:00.000Z'), d('meu-novo', '2026-10-06T12:00:00.000Z'), d('so-aqui', '2026-10-05T09:00:00.000Z')],
      [d('igual', '2026-10-06T10:00:00.000Z'), d('meu-novo', '2026-10-06T08:00:00.000Z'), d('so-la', '2026-10-06T13:00:00.000Z')],
    )
    expect(p.enviar.sort()).toEqual(['meu-novo', 'so-aqui'])
    expect(p.descarregar).toEqual(['so-la'])
    // A lista já mostra o que só está no servidor: é o que faz «abri noutro
    // computador» parecer imediato.
    expect(p.lista.map((x) => x.id)).toEqual(['so-la', 'meu-novo', 'igual', 'so-aqui'])
    // E o que ganhou é o do lado mais recente, não o local por ser local.
    expect(p.lista.find((x) => x.id === 'meu-novo')!.updatedAt).toBe('2026-10-06T12:00:00.000Z')
  })

  it('sem servidor (offline) tudo fica por enviar e nada se perde da lista', () => {
    const locais = [d('a', '2026-10-06T10:00:00.000Z'), d('b', '2026-10-06T11:00:00.000Z')]
    const p = planeia(locais, [])
    expect(p.enviar.sort()).toEqual(['a', 'b'])
    expect(p.descarregar).toEqual([])
    expect(p.lista.map((x) => x.id)).toEqual(['b', 'a'])
  })
})
