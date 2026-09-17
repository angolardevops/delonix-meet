import { describe, expect, it } from 'vitest'
import { contactBody, countSms } from './smsCount'

// Os mesmos casos que o servidor decide em `sms_codec.rs`: se o contador da
// consola divergir, quem escreve vê «1 parte» e paga duas.
describe('countSms (espelho do sms_codec)', () => {
  it('ASCII cabe em GSM-7: 160 numa parte, 161 em duas de 153', () => {
    expect(countSms('a'.repeat(160))).toEqual({ encoding: 'gsm7', units: 160, segments: 1 })
    expect(countSms('a'.repeat(161))).toEqual({ encoding: 'gsm7', units: 161, segments: 2 })
    expect(countSms('a'.repeat(306)).segments).toBe(2)
    expect(countSms('a'.repeat(307)).segments).toBe(3)
  })

  it('os caracteres da extensão custam dois septetos e não se partem', () => {
    expect(countSms('€').units).toBe(2)
    expect(countSms('a'.repeat(159) + '€')).toEqual({ encoding: 'gsm7', units: 161, segments: 2 })
    // 152 + escape: o € não cabe no fim da primeira parte e passa inteiro.
    expect(countSms('a'.repeat(152) + '€' + 'a'.repeat(10)).segments).toBe(2)
  })

  it('«ç», «ã» e «õ» não estão na tabela: UCS-2, 70 numa parte, 67 por parte depois', () => {
    expect(countSms('Reunião')).toMatchObject({ encoding: 'ucs2', segments: 1 })
    expect(countSms('ç'.repeat(70)).segments).toBe(1)
    expect(countSms('ç'.repeat(71)).segments).toBe(2)
    // O e2e do gateway: «Reunião de coordenação: » + 60 «ç» vai em 2 partes.
    expect(countSms('Reunião de coordenação: ' + 'ç'.repeat(60)).segments).toBe(2)
  })

  it('um emoji são duas unidades que ficam na mesma parte', () => {
    expect(countSms('😀')).toEqual({ encoding: 'ucs2', units: 2, segments: 1 })
    expect(countSms('a'.repeat(66) + '😀').segments).toBe(1)
    expect(countSms('a'.repeat(69) + '😀').segments).toBe(2)
  })

  it('vazio não conta partes', () => {
    expect(countSms('')).toEqual({ encoding: 'gsm7', units: 0, segments: 0 })
  })
})

describe('contactBody (espelho do sms::contact_body)', () => {
  it('nomeia quem envia e corta o nome aos 40', () => {
    expect(contactBody(' ana ', ' ola ')).toBe('ana (Delonix Meet): ola')
    expect(contactBody('x'.repeat(100), 'b').startsWith('x'.repeat(40) + ' ')).toBe(true)
  })
})
