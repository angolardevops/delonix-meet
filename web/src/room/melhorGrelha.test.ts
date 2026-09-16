import { describe, expect, it } from 'vitest'
import { melhorGrelha } from './useLayout'

describe('grelha que enche o palco', () => {
  it('12 retratos no palco do template (1400×740) dão 4 colunas × 3 linhas', () => {
    expect(melhorGrelha(1400, 740, 12)).toEqual({ cols: 4, rows: 3, w: 342, h: 240 })
  })
  it('quatro pessoas dão 2×2 e as células enchem a área', () => {
    const g = melhorGrelha(1400, 740, 4)
    expect(g).toMatchObject({ cols: 2, rows: 2 })
    expect(g.w * 2 + 10).toBe(1400)
  })
  it('uma pessoa ocupa tudo', () => {
    expect(melhorGrelha(800, 600, 1)).toEqual({ cols: 1, rows: 1, w: 800, h: 600 })
  })
})
