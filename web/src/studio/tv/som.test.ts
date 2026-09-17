import { describe, expect, it } from 'vitest'
import { CORRECCAO_NEUTRA, corrigirPixel, equilibrioCinzento, luminanciaMedia, multiplicadores, sanearCorreccao } from './correccao'
import { curvaDoEq, dbParaFader, EQ_INICIAL, faderParaDb, formatarDb, MedidorDeSonoridade, nivelDoBloco, respostaDaBanda } from './som'

const seno = (n: number, sr: number, f: number, amp: number, fase = 0) => {
  const x = new Float32Array(n)
  for (let i = 0; i < n; i++) x[i] = amp * Math.sin((2 * Math.PI * f * i) / sr + fase)
  return x
}

describe('lei do fader', () => {
  it('0 dB a três quartos, +10 no topo, silêncio em baixo', () => {
    expect(faderParaDb(0.75)).toBeCloseTo(0)
    expect(faderParaDb(1)).toBeCloseTo(10)
    expect(faderParaDb(0)).toBe(-Infinity)
  })
  it('ida e volta', () => {
    for (const p of [0.02, 0.1, 0.3, 0.5, 0.6, 0.9]) expect(dbParaFader(faderParaDb(p))).toBeCloseTo(p, 5)
  })
})

describe('sonoridade ao vivo (BS.1770-4)', () => {
  const sr = 48000

  it('um seno de 1 kHz a 0 dBFS num canal mede −3,01 LUFS', () => {
    const m = new MedidorDeSonoridade(sr)
    const bloco = m.amostrasPorBloco
    const total = seno(bloco * 40, sr, 1000, 1)
    for (let i = 0; i < 40; i++) m.alimentar([total.subarray(i * bloco, (i + 1) * bloco)])
    const l = m.ler()
    expect(l.momentanea).toBeCloseTo(-3.01, 1)
    expect(l.curta).toBeCloseTo(-3.01, 1)
    expect(l.integrada).toBeCloseTo(-3.01, 1)
  })

  it('a −20 dBFS mede −23 LUFS, e o bloco partido não muda a leitura', () => {
    const m = new MedidorDeSonoridade(sr)
    const bloco = m.amostrasPorBloco
    const total = seno(bloco * 50, sr, 1000, 0.1)
    for (let i = 0; i < 50; i++) m.alimentar([total.subarray(i * bloco, (i + 1) * bloco)])
    expect(m.ler().integrada).toBeCloseTo(-23.01, 1)
  })

  it('o silêncio fica abaixo da porta absoluta', () => {
    const m = new MedidorDeSonoridade(sr)
    for (let i = 0; i < 10; i++) m.alimentar([new Float32Array(m.amostrasPorBloco)])
    expect(m.ler().integrada).toBe(-Infinity)
  })

  it('o pico REAL apanha o que as amostras escondem (seno a fs/4, fase 45°)', () => {
    const m = new MedidorDeSonoridade(sr)
    const x = seno(m.amostrasPorBloco * 5, sr, sr / 4, 1, Math.PI / 4)
    const amostra = nivelDoBloco(x).picoDb
    for (let i = 0; i < 5; i++) m.alimentar([x.subarray(i * m.amostrasPorBloco, (i + 1) * m.amostrasPorBloco)])
    expect(amostra).toBeCloseTo(-3.01, 1)
    expect(m.ler().picoReal).toBeGreaterThan(-0.6)
    expect(m.ler().picoReal).toBeLessThan(0.6)
  })
})

describe('equalizador', () => {
  it('uma banda plana não mexe na curva', () => {
    expect(curvaDoEq(EQ_INICIAL).every((p) => Math.abs(p.db) < 1e-9)).toBe(true)
  })
  it('um pico de +6 dB a 2,4 kHz dá +6 dB a 2,4 kHz e quase nada a 100 Hz', () => {
    const b = { ...EQ_INICIAL[2], ganhoDb: 6 }
    expect(respostaDaBanda(b, 2400)).toBeCloseTo(6, 1)
    expect(Math.abs(respostaDaBanda(b, 100))).toBeLessThan(0.2)
  })
  it('a prateleira grave de −6 dB dá −6 dB a 20 Hz', () => {
    expect(respostaDaBanda({ ...EQ_INICIAL[0], ganhoDb: -6 }, 20)).toBeCloseTo(-6, 0)
  })
})

describe('formatos', () => {
  it('sinal tipográfico, vírgula da língua e infinito', () => {
    expect(formatarDb(-15.84)).toBe('−15,8')
    expect(formatarDb(1.5, 'en')).toBe('+1.5')
    expect(formatarDb(0)).toBe('0,0')
    expect(formatarDb(-Infinity)).toBe('−∞')
  })
})

describe('correcção de imagem', () => {
  it('a neutra não muda o píxel', () => {
    const p = corrigirPixel([0.3, 0.5, 0.7], CORRECCAO_NEUTRA)
    expect(p[0]).toBeCloseTo(0.3, 2)
    expect(p[1]).toBeCloseTo(0.5, 2)
    expect(p[2]).toBeCloseTo(0.7, 2)
  })
  it('+1 EV duplica a luz', () => {
    const p = corrigirPixel([0.2, 0.2, 0.2], { ...CORRECCAO_NEUTRA, exposicao: 1 })
    expect(p[1]).toBeCloseTo(0.4, 2)
  })
  it('luz de tungsténio (3 200 K) compensa-se a puxar para o azul', () => {
    const [r, , b] = multiplicadores({ ...CORRECCAO_NEUTRA, temperatura: 3200 })
    expect(b).toBeGreaterThan(r)
  })
  it('o contraste afasta do cinzento médio', () => {
    const p = corrigirPixel([0.25, 0.25, 0.25], { ...CORRECCAO_NEUTRA, contraste: 2 })
    expect(p[0]).toBeCloseTo(0, 2)
  })
  it('o equilíbrio cinzento iguala os canais e não mexe numa imagem preta', () => {
    const m = equilibrioCinzento([150, 100, 50])
    expect(150 * m[0]).toBeCloseTo(100 * m[1])
    expect(100 * m[1]).toBeCloseTo(50 * m[2])
    expect(equilibrioCinzento([2, 2, 2])).toEqual([1, 1, 1])
  })
  it('saneia o que vem do armazenamento', () => {
    const c = sanearCorreccao({ exposicao: 9, temperatura: 'x', equilibrio: [5, 1] })
    expect(c.exposicao).toBe(2)
    expect(c.temperatura).toBe(6500)
    expect(c.equilibrio).toEqual([1, 1, 1])
  })
  it('luminância média de RGBA', () => {
    const d = new Uint8ClampedArray(16).fill(255)
    expect(luminanciaMedia(d, 1)).toBeCloseTo(255)
  })
})
