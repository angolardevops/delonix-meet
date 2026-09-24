import { describe, expect, it } from 'vitest'
import { contagemDosDestinos, estadoDoCartao } from './destinosLocais'

describe('estado de um cartão de destino', () => {
  it('sem chave nunca está no ar, mesmo com a emissão a decorrer', () => {
    expect(estadoDoCartao('no-ar', false)).toBe('sem-chave')
    expect(estadoDoCartao('erro', false)).toBe('sem-chave')
  })

  it('com chave segue a fase da emissão', () => {
    expect(estadoDoCartao('parado', true)).toBe('pronto')
    expect(estadoDoCartao('a-ligar', true)).toBe('a-ligar')
    expect(estadoDoCartao('no-ar', true)).toBe('no-ar')
    expect(estadoDoCartao('erro', true)).toBe('erro')
  })

  it('a contagem do cabeçalho soa aos cartões', () => {
    const ds = [{ chave: 'a' }, { chave: ' ' }, { chave: 'b' }]
    expect(contagemDosDestinos(ds, 'no-ar')).toEqual({ 'sem-chave': 1, pronto: 0, 'a-ligar': 0, 'no-ar': 2, erro: 0 })
    expect(contagemDosDestinos(ds, 'parado')).toEqual({ 'sem-chave': 1, pronto: 2, 'a-ligar': 0, 'no-ar': 0, erro: 0 })
  })
})
