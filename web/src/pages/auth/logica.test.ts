import { describe, expect, it, vi } from 'vitest'
// O erro verdadeiro do `api.ts`, não um imitado: o `api.ts` lê o localStorage
// ao carregar, por isso entra depois do esboço (a bateria corre em node).
vi.stubGlobal('localStorage', { getItem: () => null, setItem: () => {}, removeItem: () => {} })
const { ApiError } = await import('../../api')

import { codigoCompleto, dominioDoEmail, estadoHttp, megabytes, motivoDaRecusa, normalizarCodigo, partesUptime, saudeGlobal } from './logica'

describe('domínio do email para o SSO', () => {
  it('só pergunta quando há um domínio com ponto', () => {
    expect(dominioDoEmail('ana@')).toBeNull()
    expect(dominioDoEmail('ana@empresa')).toBeNull()
    expect(dominioDoEmail('@empresa.ao')).toBeNull()
    expect(dominioDoEmail('ana')).toBeNull()
    expect(dominioDoEmail(' Ana@Empresa.CO.ao ')).toBe('empresa.co.ao')
  })
})

describe('código do desafio de segundo factor', () => {
  it('o TOTP são seis dígitos, e o resto do que se cola cai', () => {
    expect(normalizarCodigo('123 456 7', 'totp')).toBe('123456')
    expect(codigoCompleto('12345', 'totp')).toBe(false)
    expect(codigoCompleto('123456', 'totp')).toBe(true)
  })

  it('o de recuperação aceita minúsculas e sem hífen, e chega no formato do servidor', () => {
    // server/src/mfa.rs `codigos_de_recuperacao`: XXXXX-XXXXX, base32.
    expect(normalizarCodigo('abcde fghij', 'recuperacao')).toBe('ABCDE-FGHIJ')
    expect(normalizarCodigo('ABCDE-FGHIJ', 'recuperacao')).toBe('ABCDE-FGHIJ')
    expect(codigoCompleto('ABCDE-FGHIJ', 'recuperacao')).toBe(true)
    expect(codigoCompleto('ABCDE-FGHI', 'recuperacao')).toBe(false)
  })
})

describe('motivo de uma entrada recusada', () => {
  it('401 é uma só mensagem — não diz se a conta existe', () => {
    expect(motivoDaRecusa(new ApiError(401, null, 'unauthorized'))).toBe('auth.erro.credenciais')
  })
  it('429 e sem ligação têm explicação própria', () => {
    expect(motivoDaRecusa(new ApiError(429, null, 'x'))).toBe('auth.erro.demasiadasTentativas')
    expect(motivoDaRecusa(new TypeError('Failed to fetch'))).toBe('auth.erro.semLigacao')
  })
  it('reconhece o ApiError verdadeiro sem instanceof', () => {
    expect(estadoHttp(new ApiError(503, null, 'x'))).toBe(503)
    expect(estadoHttp(new Error('x'))).toBeNull()
  })
  it('um 400 com texto do servidor passa o texto (ex.: SSO obrigatório)', () => {
    expect(motivoDaRecusa(new ApiError(400, { error: 'x' }, 'x'))).toBeNull()
  })
})

describe('página de estado', () => {
  it('sem resposta é indisponível; base de dados em baixo é degradado', () => {
    expect(saudeGlobal(null)).toBe('indisponivel')
    expect(saudeGlobal({ status: 'degraded', api: true, db: false, uptime_secs: 1, version: '1' })).toBe('degradado')
    expect(saudeGlobal({ status: 'ok', api: true, db: true, uptime_secs: 1, version: '1' })).toBe('operacional')
  })
  it('uptime parte-se em dias, horas e minutos', () => {
    expect(partesUptime(90061)).toEqual({ d: 1, h: 1, m: 1 })
    expect(partesUptime(-5)).toEqual({ d: 0, h: 0, m: 0 })
  })
  it('tamanho em MB com uma casa', () => {
    expect(megabytes(1_572_864)).toBe('1.5')
  })
})
