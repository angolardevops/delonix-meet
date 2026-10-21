import { describe, expect, it } from 'vitest'
import { formatBytes } from './format'

describe('formatBytes', () => {
  /**
   * O DEFEITO QUE ISTO GUARDA: havia três formatadores e o mesmo ficheiro
   * aparecia com 1,5 GB, 1 GB e 1,4 GB em três ecrãs. O que este teste exige
   * não é um texto bonito — é que exista UMA resposta.
   */
  it('a base é 1024 e o rótulo di-lo', () => {
    expect(formatBytes(1_500_000_000, 'pt')).toBe('1,4 GiB')
    expect(formatBytes(1024 ** 3, 'pt')).toBe('1 GiB')
    expect(formatBytes(10 * 1024 ** 3, 'pt')).toBe('10 GiB')
  })

  it('decimais só a partir de MiB', () => {
    // «820 KiB», não «820,0 KiB» — uma casa num valor destes é ruído.
    expect(formatBytes(840_000, 'pt')).toBe('820 KiB')
    expect(formatBytes(62_172_000, 'pt')).toBe('59,3 MiB')
  })

  it('os pequenos e o zero', () => {
    // Um ficheiro vazio é um facto, não a ausência de um.
    expect(formatBytes(0, 'pt')).toBe('0 B')
    expect(formatBytes(1, 'pt')).toBe('1 B')
    expect(formatBytes(2_500, 'pt')).toBe('2 KiB')
  })

  it('a língua manda no separador decimal', () => {
    expect(formatBytes(1_500_000_000, 'en')).toBe('1.4 GiB')
    expect(formatBytes(1_500_000_000, 'pt')).toBe('1,4 GiB')
  })

  it('o que não é um tamanho não finge ser', () => {
    expect(formatBytes(Number.NaN, 'pt')).toBe('—')
    expect(formatBytes(-1, 'pt')).toBe('—')
    expect(formatBytes(Number.POSITIVE_INFINITY, 'pt')).toBe('—')
  })
})
