import { describe, expect, it } from 'vitest'
import { estimateSms } from './smsSegments'

describe('estimateSms — GSM 03.38', () => {
  it('corpo vazio são zero segmentos', () => {
    expect(estimateSms('')).toMatchObject({ encoding: 'gsm7', units: 0, segments: 0 })
  })

  it('160 caracteres ASCII cabem numa parte', () => {
    expect(estimateSms('a'.repeat(160))).toMatchObject({ encoding: 'gsm7', units: 160, segments: 1, perSegment: 160 })
  })

  it('161 passam a duas partes de 153', () => {
    expect(estimateSms('a'.repeat(161))).toMatchObject({ encoding: 'gsm7', segments: 2, perSegment: 153 })
    expect(estimateSms('a'.repeat(306)).segments).toBe(2)
    expect(estimateSms('a'.repeat(307)).segments).toBe(3)
  })

  it('€ é da tabela de extensão e conta 2 septetos', () => {
    expect(estimateSms('€')).toMatchObject({ encoding: 'gsm7', units: 2 })
    // 159 + 2 = 161 → já não cabe numa parte.
    expect(estimateSms('a'.repeat(159) + '€').segments).toBe(2)
    expect(estimateSms('a'.repeat(158) + '€').segments).toBe(1)
  })

  it('os restantes da extensão também contam 2', () => {
    expect(estimateSms('^{}\\[~]|').units).toBe(16)
  })

  it('Ç maiúsculo está na tabela básica (0x09)', () => {
    expect(estimateSms('ÇÉ')).toMatchObject({ encoding: 'gsm7', units: 2 })
  })

  it('ç minúsculo NÃO está na tabela básica → UCS-2', () => {
    expect(estimateSms('Serviço').encoding).toBe('ucs2')
  })

  it('à, é, ü estão na básica; ã e õ não', () => {
    expect(estimateSms('à é ü').encoding).toBe('gsm7')
    expect(estimateSms('atenção').encoding).toBe('ucs2')
    expect(estimateSms('põe').encoding).toBe('ucs2')
  })

  it('UCS-2: 70 numa parte, 67 por parte a seguir', () => {
    expect(estimateSms('ã'.repeat(70))).toMatchObject({ encoding: 'ucs2', units: 70, segments: 1, perSegment: 70 })
    expect(estimateSms('ã'.repeat(71))).toMatchObject({ segments: 2, perSegment: 67 })
    expect(estimateSms('ã'.repeat(134)).segments).toBe(2)
    expect(estimateSms('ã'.repeat(135)).segments).toBe(3)
  })

  it('emoji conta em unidades UTF-16 (fora do BMP são 2)', () => {
    const e = estimateSms('ok \u{1F600}')
    expect(e.encoding).toBe('ucs2')
    expect(e.units).toBe(5)
    expect(estimateSms('\u{1F600}'.repeat(35)).segments).toBe(1)
    expect(estimateSms('\u{1F600}'.repeat(36)).segments).toBe(2)
  })

  it('quebras de linha LF e CR são GSM-7 básico', () => {
    expect(estimateSms('a\nb\r')).toMatchObject({ encoding: 'gsm7', units: 4 })
  })
})
