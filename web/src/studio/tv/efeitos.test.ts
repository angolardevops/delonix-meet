/**
 * O modelo dos efeitos sonoros: as teclas e a regra do ducking.
 *
 * Tocar é da mesa de som (dona do `AudioContext`) e não se prova sem browser;
 * o que se prende aqui é o que decide sozinho numa emissão.
 */
import { describe, expect, it } from 'vitest'
import { DUCK_DB, efeitoDaTecla, haVoz, idDoEfeito, indiceDoEfeito, LIMIAR_DE_VOZ_DB, MAX_EFEITOS, teclaDoEfeito } from './efeitos'

describe('as teclas dos efeitos', () => {
  it('são a fila a seguir às sobreposições', () => {
    expect(teclaDoEfeito(0)).toBe(5)
    expect(teclaDoEfeito(MAX_EFEITOS - 1)).toBe(8)
    expect(efeitoDaTecla(5)).toBe(0)
    expect(efeitoDaTecla(8)).toBe(MAX_EFEITOS - 1)
  })

  it('e uma tecla fora da fila não dispara nada', () => {
    // ⌥4 é a última sobreposição: se esta função a reclamasse, carregar na
    // sondagem disparava um genérico no ar.
    expect(efeitoDaTecla(4)).toBeNull()
    expect(efeitoDaTecla(9)).toBeNull()
  })

  it('o id do canal e o índice são a mesma coisa, nos dois sentidos', () => {
    expect(idDoEfeito(2)).toBe('efeito:2')
    expect(indiceDoEfeito('efeito:2')).toBe(2)
    expect(indiceDoEfeito('efeito:9')).toBeNull()
    expect(indiceDoEfeito('mic:omissao')).toBeNull()
  })
})

describe('o ducking segue a voz', () => {
  it('baixa com um microfone acima do limiar', () => {
    expect(haVoz([-20])).toBe(true)
    expect(haVoz([-80, -12])).toBe(true)
  })

  it('e não baixa com ruído de sala, nem com um canal em silêncio absoluto', () => {
    // Um limiar baixo de mais faz a música abanar sozinha entre as palavras,
    // que é pior do que não haver ducking nenhum.
    expect(haVoz([-50])).toBe(false)
    expect(haVoz([LIMIAR_DE_VOZ_DB])).toBe(false)
    expect(haVoz([-Infinity])).toBe(false)
    expect(haVoz([])).toBe(false)
  })

  it('e o degrau é o de rádio: a cama continua a ouvir-se por baixo', () => {
    // Um duck que corta a música deixa um buraco a cada pausa.
    expect(DUCK_DB).toBeLessThan(0)
    expect(DUCK_DB).toBeGreaterThan(-24)
  })
})
