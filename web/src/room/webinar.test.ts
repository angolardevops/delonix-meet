import { describe, expect, it } from 'vitest'
import { aAssistir, emPalco } from './webinar'

describe('webinar — quem publica', () => {
  it('em palco: anfitrião, co-anfitrião, orador e emissão', () => {
    for (const r of ['host', 'cohost', 'speaker', 'broadcast'] as const) expect(emPalco(r)).toBe(true)
    expect(emPalco('attendee')).toBe(false)
  })

  it('num webinar, o assistente só assiste; numa reunião normal, ninguém', () => {
    expect(aAssistir(true, 'attendee')).toBe(true)
    expect(aAssistir(true, 'speaker')).toBe(false)
    expect(aAssistir(false, 'attendee')).toBe(false)
  })
})
