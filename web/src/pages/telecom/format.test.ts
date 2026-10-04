import { describe, expect, it } from 'vitest'
import type { SipRegistration, SipSettings, Trunk } from '../../api'
import type { Async } from '../../components/AsyncSection'
import {
  currencyLabel,
  enumKey,
  formatDecimal,
  formatMoney,
  formatRatio,
  measured,
  nextToken,
  pageMode,
  reasonKey,
  sbcTone,
  sortTrunks,
  trunkCounts,
  trunkTone,
} from './format'

describe('dinheiro: decimal em texto, formatado como texto', () => {
  it('agrupa os milhares e usa o separador decimal da língua', () => {
    expect(formatDecimal('184620.0000', 'en-GB')).toBe('184,620.00')
    expect(formatDecimal('184620.5', 'de-DE')).toBe('184.620,50')
  })

  it('tira os zeros à direita, mas nunca abaixo de duas casas', () => {
    expect(formatDecimal('12.5000', 'en-GB')).toBe('12.50')
    expect(formatDecimal('12', 'en-GB')).toBe('12.00')
    expect(formatDecimal('0.0000', 'en-GB')).toBe('0.00')
  })

  it('não arredonda: um preço por minuto com quatro casas fica com as quatro', () => {
    expect(formatDecimal('0.0375', 'en-GB')).toBe('0.0375')
    expect(formatDecimal('1.2345', 'en-GB')).toBe('1.2345')
  })

  it('um valor que não cabe num double não perde dígitos', () => {
    // 2^53 + 1: como number seria 9007199254740992.
    expect(formatDecimal('9007199254740993.10', 'en-GB')).toBe('9,007,199,254,740,993.10')
    // A soma que em vírgula flutuante dá 0.30000000000000004 nunca é feita.
    expect(formatDecimal('0.30', 'en-GB')).toBe('0.30')
  })

  it('negativos e zeros à esquerda', () => {
    expect(formatDecimal('-1500.00', 'en-GB')).toBe('-1,500.00')
    expect(formatDecimal('007.10', 'en-GB')).toBe('7.10')
  })

  it('o que não é um decimal simples devolve null, em vez de um número inventado', () => {
    expect(formatDecimal('', 'en-GB')).toBeNull()
    expect(formatDecimal('1e3', 'en-GB')).toBeNull()
    expect(formatDecimal('1,5', 'en-GB')).toBeNull()
    expect(formatDecimal('NaN', 'en-GB')).toBeNull()
  })

  it('o kwanza escreve-se Kz; as outras moedas ficam pelo código', () => {
    expect(currencyLabel('AOA')).toBe('Kz')
    expect(currencyLabel('USD')).toBe('USD')
    expect(formatMoney({ amount: '184620.0000', currency: 'AOA' }, 'en-GB')).toBe('184,620.00 Kz')
    expect(formatMoney({ amount: '0.0375', currency: 'USD' }, 'en-GB')).toBe('0.0375 USD')
  })

  it('um montante ilegível mostra-se tal qual, com a moeda que o servidor mandou', () => {
    expect(formatMoney({ amount: 'abc', currency: 'AOA' }, 'en-GB')).toBe('abc AOA')
  })
})

describe('medições: null não é zero', () => {
  it('zero é uma medição e fica zero', () => {
    expect(measured(0)).toBe(0)
    expect(measured(6.2)).toBe(6.2)
  })

  it('null, omitido e não-finito são «sem medição»', () => {
    expect(measured(null)).toBeNull()
    expect(measured(undefined)).toBeNull()
    expect(measured(Number.NaN)).toBeNull()
  })

  it('o ASR é uma fracção e mostra-se em percentagem', () => {
    expect(formatRatio(0.62, 'en-GB')).toBe('62%')
    expect(formatRatio(0, 'en-GB')).toBe('0%')
    expect(formatRatio(1, 'en-GB')).toBe('100%')
  })
})

describe('razões e valores enumerados', () => {
  it('uma razão conhecida tem chave de tradução', () => {
    expect(reasonKey('no_calls_in_window')).toBe('telecom.razao.no_calls_in_window')
    expect(reasonKey('sip_not_configured')).toBe('telecom.razao.sip_not_configured')
    expect(reasonKey('missing_exchange_rate')).toBe('telecom.razao.missing_exchange_rate')
  })

  it('uma razão nova do servidor não tem chave — mostra-se tal qual', () => {
    expect(reasonKey('carrier_on_fire')).toBeNull()
    expect(reasonKey('connection refused (os error 111)')).toBeNull()
  })

  it('um valor enumerado novo não é «corrigido» para um conhecido', () => {
    expect(enumKey('tronco', 'up')).toBe('telecom.tronco.up')
    expect(enumKey('tronco', 'maintenance')).toBeNull()
    expect(enumKey('desfecho', 'answered')).toBe('telecom.desfecho.answered')
    expect(enumKey('grupo_que_nao_existe', 'up')).toBeNull()
  })

  it('todas as razões e valores conhecidos existem no dicionário de origem', async () => {
    const pt = (await import('../../locales/pt/telecom')).default as unknown as Record<string, Record<string, string>>
    for (const code of ['sip_not_configured', 'settings_missing', 'not_configured', 'media_server_unreachable', 'sbc_not_configured', 'sbc_unreachable', 'trunk_down', 'trunk_degraded', 'registration_failed', 'not_loaded_on_media_server', 'options_ping_failed', 'low_asr', 'registering', 'no_measurement', 'sip_status_unavailable', 'no_calls_in_window', 'missing_exchange_rate', 'inbound_not_billed', 'no_trunk', 'no_price_in_force']) {
      expect(reasonKey(code), code).not.toBeNull()
      expect(pt.razao[code], code).toBeTruthy()
    }
    const grupos: Record<string, string[]> = {
      sbc: ['healthy', 'degraded', 'down', 'not_configured'],
      tronco: ['up', 'degraded', 'down', 'unknown'],
      accao: ['external', 'extension', 'room_pin', 'block'],
      desfecho: ['answered', 'busy', 'no_answer', 'failed', 'forwarded', 'waiting_room', 'wrong_pin'],
      sentido: ['inbound', 'outbound'],
      srtp: ['mandatory', 'optional', 'off'],
    }
    for (const [g, vs] of Object.entries(grupos)) {
      for (const v of vs) {
        expect(enumKey(g, v), `${g}.${v}`).toBe(`telecom.${g}.${v}`)
        expect(pt[g][v], `${g}.${v}`).toBeTruthy()
      }
    }
    for (const s of ['up', 'degraded', 'down', 'unknown']) {
      expect(pt.contagem[`${s}_one`], s).toBeTruthy()
      expect(pt.contagem[`${s}_other`], s).toBeTruthy()
    }
  })
})

describe('estados', () => {
  it('o tom acompanha o estado; um estado desconhecido é neutro, não «bom»', () => {
    expect(sbcTone('healthy')).toBe('success')
    expect(sbcTone('degraded')).toBe('warning')
    expect(sbcTone('down')).toBe('record')
    expect(sbcTone('not_configured')).toBe('neutral')
    expect(sbcTone('whatever')).toBe('neutral')
    expect(trunkTone('up')).toBe('success')
    expect(trunkTone('unknown')).toBe('neutral')
  })

  it('a contagem por estado só lista os estados que têm operadoras', () => {
    expect(trunkCounts({ total: 4, up: 3, degraded: 1, down: 0, unknown: 0 })).toEqual([
      { state: 'up', n: 3 },
      { state: 'degraded', n: 1 },
    ])
    expect(trunkCounts({ total: 0, up: 0, degraded: 0, down: 0, unknown: 0 })).toEqual([])
  })

  it('as operadoras saem pela posição, sem alterar a lista recebida', () => {
    const k = (id: string, position: number, name = id) => ({ id, position, name }) as Trunk
    const input = [k('c', 2), k('a', 0), k('b', 1)]
    expect(sortTrunks(input).map((x) => x.id)).toEqual(['a', 'b', 'c'])
    expect(input.map((x) => x.id)).toEqual(['c', 'a', 'b'])
  })
})

describe('paginação: o servidor OMITE o cursor quando não há mais páginas', () => {
  it('campo omitido — a forma real da resposta — é fim de lista', () => {
    expect(nextToken(JSON.parse('{"items":[]}'))).toBeNull()
  })

  it('null e vazio também são fim de lista', () => {
    expect(nextToken({ next_page_token: null })).toBeNull()
    expect(nextToken({ next_page_token: '' })).toBeNull()
  })

  it('um cursor é devolvido tal qual', () => {
    expect(nextToken({ next_page_token: 'eyJwIjoxfQ' })).toBe('eyJwIjoxfQ')
  })
})

describe('modo da página: não configurado não é uma página de zeros, e um erro não é «vazio»', () => {
  // As formas reais de uma organização sem telefonia (servidor de validação, 2026-10-03).
  const settingsOff: SipSettings = { configured: false, domain: null, sbc_host: null, transport: null, srtp: null, codecs: [], username: null, password_configured: false, updated_at: null }
  const registrationOff = {
    state: 'not_configured',
    reasons: ['sip_not_configured'],
    channels: { in_use: null, max: 0 },
    trunks: { total: 0, up: 0, degraded: 0, down: 0, unknown: 0 },
    quality: { jitter_ms: null, loss_pct: null, mos: null, calls: 0, window_hours: 24, reason: 'no_calls_in_window' },
    codecs_configured: [],
    codecs_offered: [],
    measured_at: '2026-10-03T14:33:25.510566518Z',
  } as SipRegistration
  type Sip = Async<{ settings: SipSettings; registration: SipRegistration }>
  const sipOff: Sip = { s: 'ready', d: { settings: settingsOff, registration: registrationOff } }
  const sipOn: Sip = { s: 'ready', d: { settings: { ...settingsOff, configured: true }, registration: { ...registrationOff, state: 'healthy', reasons: [] } } }
  const noTrunks: Async<{ items: Trunk[] }> = { s: 'ready', d: { items: [] } }
  const oneTrunk: Async<{ items: Trunk[] }> = { s: 'ready', d: { items: [{ id: 't1' } as Trunk] } }

  it('sem definições SIP e sem operadoras: não configurado', () => {
    expect(pageMode(sipOff, noTrunks)).toBe('not_configured')
  })

  it('enquanto qualquer dos dois carrega, a página carrega', () => {
    expect(pageMode({ s: 'loading' }, noTrunks)).toBe('loading')
    expect(pageMode(sipOff, { s: 'loading' })).toBe('loading')
  })

  it('configurada mostra-se, mesmo sem operadoras', () => {
    expect(pageMode(sipOn, noTrunks)).toBe('show')
  })

  it('operadoras sem definições SIP mostram-se: há dados para ver', () => {
    expect(pageMode(sipOff, oneTrunk)).toBe('show')
  })

  it('um erro de rede nunca passa por «não configurado»', () => {
    expect(pageMode({ s: 'error', msg: 'rede' }, noTrunks)).toBe('show')
    expect(pageMode(sipOff, { s: 'error', msg: 'rede' })).toBe('show')
    expect(pageMode({ s: 'error', msg: 'rede' }, { s: 'error', msg: 'rede' })).toBe('show')
  })

  it('a qualidade de quem não mediu é null em todas as métricas, com razão', () => {
    const q = registrationOff.quality
    expect([measured(q.jitter_ms), measured(q.loss_pct), measured(q.mos)]).toEqual([null, null, null])
    expect(reasonKey(q.reason ?? '')).toBe('telecom.razao.no_calls_in_window')
    expect(measured(registrationOff.channels.in_use)).toBeNull()
  })
})
