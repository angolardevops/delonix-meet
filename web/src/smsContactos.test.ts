import { describe, expect, it, vi } from 'vitest'

// O `api.ts` lê o localStorage no topo do módulo (ver api.guardas.test.ts).
vi.stubGlobal('localStorage', { getItem: () => null, setItem: () => {}, removeItem: () => {}, clear: () => {} })
const { smsErrorCode } = await import('./api')

// Os códigos `sms.*` vão no início do texto de `error` (ADR-0005 §Contactos).
// A UI decide a mensagem por eles — nunca pelo texto por extenso, que muda.
describe('smsErrorCode', () => {
  it('lê o código estável do início do erro', () => {
    expect(smsErrorCode('sms.recipient_opted_out: a pessoa desligou estes SMS no perfil')).toBe('sms.recipient_opted_out')
    expect(smsErrorCode('sms.recipient_no_phone: a pessoa não tem telemóvel')).toBe('sms.recipient_no_phone')
  })

  it('não inventa código num erro sem ele', () => {
    expect(smsErrorCode('forbidden')).toBeNull()
    expect(smsErrorCode('sem rota: a ligação à Movicel não está contratada')).toBeNull()
    expect(smsErrorCode('erro sms.recipient_opted_out a meio')).toBeNull()
  })
})
