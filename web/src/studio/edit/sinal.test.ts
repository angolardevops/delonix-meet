/**
 * Sonoridade e trechos fracos, com sinais cujo valor se conhece de antemão.
 * Referência da norma BS.1770: um seno de 997 Hz a 0 dBFS num canal mede
 * −3,01 LUFS.
 */
import { describe, expect, it } from 'vitest'
import { from } from './sinalTeste'
import {
  aplicarGanho,
  dbParaGanho,
  ganhoParaAlvo,
  limitar,
  lufsIntegrado,
  normalizarPico,
  pico,
  picos,
  rmsPorJanela,
  trechosForaDoMicrofone,
} from './sinal'
import { predefinicao, tamanhoEstimado, tamanhoLegivel, nomeDeFicheiro, tempoEstimado } from '../exports/predefinicoes'

const SR = 48_000

describe('LUFS integrado', () => {
  it('seno de 997 Hz a 0 dBFS mede −3,01 LUFS', () => {
    expect(lufsIntegrado([from.seno(997, 1, 3, SR)], SR)).toBeCloseTo(-3.01, 1)
  })

  it('−20 dB de amplitude são −20 LU', () => {
    expect(lufsIntegrado([from.seno(997, dbParaGanho(-20), 3, SR)], SR)).toBeCloseTo(-23.01, 1)
  })

  it('também a 44,1 kHz', () => {
    expect(lufsIntegrado([from.seno(997, 1, 3, 44_100)], 44_100)).toBeCloseTo(-3.01, 1)
  })

  it('as pausas longas NÃO baixam a medida (porta relativa)', () => {
    const fala = from.seno(997, dbParaGanho(-20), 3, SR)
    const comPausas = from.juntar(fala, new Float32Array(SR * 6), fala)
    expect(lufsIntegrado([comPausas], SR)).toBeCloseTo(-23.01, 0)
  })

  it('silêncio dá −Infinity e o ganho para o alvo é 0', () => {
    const l = lufsIntegrado([new Float32Array(SR * 2)], SR)
    expect(l).toBe(-Infinity)
    expect(ganhoParaAlvo(l, -14)).toBe(0)
  })

  it('o ganho para o alvo é limitado a ±24 dB', () => {
    expect(ganhoParaAlvo(-23, -14)).toBe(9)
    expect(ganhoParaAlvo(-60, -14)).toBe(24)
  })
})

describe('ganho, limitador e normalização', () => {
  it('o limitador segura os picos no tecto', () => {
    const s = from.seno(440, 1, 1, SR)
    aplicarGanho([s], 2)
    limitar([s], SR, -1)
    expect(pico([s])).toBeLessThanOrEqual(dbParaGanho(-1) + 1e-6)
  })

  it('normalizar leva o pico a −1 dBFS', () => {
    const s = from.seno(440, 0.1, 1, SR)
    normalizarPico([s], -1)
    expect(pico([s])).toBeCloseTo(dbParaGanho(-1), 3)
  })
})

describe('onda sonora', () => {
  it('um pico por balde', () => {
    const d = new Float32Array(100)
    d[10] = 0.5
    d[90] = -0.9
    const p = picos([d], 4)
    expect(Array.from(p)).toEqual([0.5, 0, 0, expect.closeTo(0.9, 5)])
  })
})

describe('fala fora do microfone', () => {
  it('encontra o trecho baixo entre fala normal e não confunde com silêncio', () => {
    const forte = from.ruido(0.3, 5, SR)
    const fraco = from.ruido(0.04, 3, SR)
    const calado = new Float32Array(SR * 2)
    const sinal = from.juntar(forte, fraco, forte, calado, forte)
    const janela = 0.05
    const t = trechosForaDoMicrofone(rmsPorJanela([sinal], SR, janela), janela)
    expect(t).toHaveLength(1)
    expect(t[0].inicio).toBeCloseTo(5, 0)
    expect(t[0].fim).toBeCloseTo(8, 0)
    expect(t[0].ganhoDb).toBeGreaterThan(10)
  })

  it('trechos curtos não contam', () => {
    const sinal = from.juntar(from.ruido(0.3, 5, SR), from.ruido(0.04, 0.5, SR), from.ruido(0.3, 5, SR))
    expect(trechosForaDoMicrofone(rmsPorJanela([sinal], SR, 0.05), 0.05)).toEqual([])
  })
})

describe('estimativas de exportação', () => {
  it('tamanho = débito × duração (+ contentor)', () => {
    // 8 Mbps + 128 kbps durante 60 s ≈ 62 MB.
    expect(tamanhoEstimado(predefinicao('web1080'), 60)).toBe(Math.round((8_128_000 * 60) / 8 * 1.02))
    expect(tamanhoLegivel(62_172_000)).toBe('62,2 MB')
    expect(tamanhoLegivel(2_500)).toBe('3 KB')
  })

  it('o tempo usa o ritmo medido quando o há', () => {
    expect(tempoEstimado(predefinicao('web1080'), 60, 60)).toBe(30)
    expect(tempoEstimado(predefinicao('podcast'), 400, null)).toBe(10)
  })

  it('nome de ficheiro sem acentos nem espaços', () => {
    expect(nomeDeFicheiro('Arquitectura de Voz — sessão 3', 'vertical', 'webm')).toBe('Arquitectura-de-Voz-sessao-3-vertical.webm')
  })
})
