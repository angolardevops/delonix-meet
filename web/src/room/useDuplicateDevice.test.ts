import { describe, expect, it } from 'vitest'
import { decididas, enfileirar, lembrar, resolver } from './dispositivos'

const mem = () => {
  const m = new Map<string, string>()
  return { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v) }
}
const a = (id: string, canHangup = true) => ({ phoneId: id, canHangup })

describe('dispositivos duplicados · fila de perguntas', () => {
  it('duas pernas ficam em fila e resolver uma não fecha a outra', () => {
    let f = enfileirar([], a('p1'), new Set())
    f = enfileirar(f, a('p2'), new Set())
    expect(f.map((x) => x.phoneId)).toEqual(['p1', 'p2'])
    f = resolver(f, 'p1')
    expect(f.map((x) => x.phoneId)).toEqual(['p2'])
    // resolver uma perna desconhecida não mexe em nada
    expect(resolver(f, 'outra')).toEqual(f)
  })
  it('o mesmo aviso duas vezes não duplica a pergunta, só a actualiza', () => {
    const f = enfileirar(enfileirar([], a('p1', false), new Set()), a('p1', true), new Set())
    expect(f).toEqual([a('p1', true)])
  })
  it('o que já se decidiu nesta sala não volta a perguntar (F5, reconexão)', () => {
    const s = mem()
    expect(decididas('sala', s).size).toBe(0)
    lembrar('sala', 'p1', s)
    const d = decididas('sala', s)
    expect(enfileirar([], a('p1'), d)).toEqual([])
    expect(enfileirar([], a('p2'), d)).toEqual([a('p2')])
    // outra sala não herda a decisão
    expect(decididas('outra', s).size).toBe(0)
  })
  it('sem storage ou com lixo, pergunta-se outra vez e nada rebenta', () => {
    expect(decididas('sala', null).size).toBe(0)
    expect(decididas('sala', { getItem: () => '{não é json' }).size).toBe(0)
    expect(() => lembrar('sala', 'p1', null)).not.toThrow()
  })
})
