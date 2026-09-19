import { describe, expect, it } from 'vitest'
import { barras, debito, veredicto } from './qualidadePrevista'

describe('qualidade prevista da pré-entrada', () => {
  it('o veredicto segue o gargalo (o menor dos dois sentidos), pela mediana', () => {
    expect(veredicto([])).toBeNull()
    expect(veredicto([{ downKbps: 48_000, upKbps: 22_000 }, { downKbps: 50_000, upKbps: 30_000 }, { downKbps: 49_000, upKbps: 26_000 }])).toBe('4k')
    expect(veredicto([{ downKbps: 48_000, upKbps: 3_000 }])).toBe('720p')
    // Uma sondagem má isolada não condena a ligação.
    expect(veredicto([{ downKbps: 9_000, upKbps: 9_000 }, { downKbps: 100, upKbps: 100 }, { downKbps: 9_500, upKbps: 9_500 }])).toBe('1080p')
  })
  it('as barras marcam as quebras abaixo de 70 % da mediana', () => {
    const b = barras([{ downKbps: 10, upKbps: 10 }, { downKbps: 10, upKbps: 4 }, { downKbps: 10, upKbps: 10 }])
    expect(b.map((x) => x.quebra)).toEqual([false, true, false])
    expect(b[0].altura).toBe(1)
  })
  it('débitos legíveis', () => {
    expect(debito(48_300, 'pt-PT')).toBe('48 Mbps')
    expect(debito(640, 'pt-PT')).toBe('640 kbps')
  })
})
