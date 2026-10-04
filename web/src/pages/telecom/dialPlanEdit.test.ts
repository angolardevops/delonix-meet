/**
 * A lógica da edição do plano de marcação e de «Testar número». As formas das
 * recusas são as que o servidor devolveu de facto a 2026-10-03.
 */
import { describe, expect, it, vi } from 'vitest'
import type { DialRule } from '../../api'

// O cliente da API lê a sessão do armazenamento ao carregar: em Node não existe.
vi.stubGlobal('localStorage', { getItem: () => null, setItem: () => {}, removeItem: () => {}, clear: () => {} })

const { ApiError } = await import('../../api')
const { draftsFromRules, isDialable, newDraft, outcomeKey, priceReasonKey, ruleErrorTarget, ruleFieldForCode, rulesFromDrafts, validateDraft, validateDrafts, withAction, RULE_ACTIONS } =
  await import('./dialPlanEdit')
const { errorCode, errorKey, moveItem } = await import('./trunkForm')

const recusa = (code: string, fields: string[]) =>
  new ApiError(400, { code, details: fields.map((field) => ({ field, description: code })), error: 'regra 1: texto do servidor', request_id: 'x' }, 'texto do servidor')

const regras: DialRule[] = [
  { pattern: '9XXXXXXXX', description: 'Móveis nacionais', action: 'external', trunk_id: 't-a', fallback_trunk_id: 't-b', record: true, emergency: false },
  { pattern: '1XX', description: 'Ramais', action: 'extension', trunk_id: null, fallback_trunk_id: null, record: false, emergency: false },
  { pattern: '112', description: 'Emergência', action: 'external', trunk_id: 't-a', fallback_trunk_id: null, record: false, emergency: true },
]

describe('rascunhos ↔ regras', () => {
  it('ida e volta sem alterações devolve o plano tal como veio', () => {
    expect(rulesFromDrafts(draftsFromRules(regras))).toEqual(regras)
  })

  it('cada rascunho tem uma identidade própria, que não depende da posição', () => {
    const d = draftsFromRules(regras)
    expect(new Set(d.map((x) => x.key)).size).toBe(3)
    expect(new Set([...d, newDraft()].map((x) => x.key)).size).toBe(4)
  })

  it('campos em falta na resposta viram valores neutros, não `undefined`', () => {
    const [d] = draftsFromRules([{ pattern: '2X', description: 'x', action: 'block' }])
    expect([d.trunk_id, d.fallback_trunk_id, d.record, d.emergency]).toEqual(['', '', false, false])
  })

  it('só as regras externas levam operadora e reserva; o vazio vai a null', () => {
    const [d] = draftsFromRules([regras[0]])
    const semReserva = rulesFromDrafts([{ ...d, fallback_trunk_id: '' }])[0]
    expect(semReserva.fallback_trunk_id).toBeNull()
    const bloqueio = rulesFromDrafts([{ ...d, action: 'block' }])[0]
    expect([bloqueio.trunk_id, bloqueio.fallback_trunk_id]).toEqual([null, null])
  })

  it('mudar a acção para não-externa larga a operadora e a reserva (o servidor recusá-las-ia)', () => {
    const [d] = draftsFromRules([regras[0]])
    const b = withAction(d, 'room_pin')
    expect([b.action, b.trunk_id, b.fallback_trunk_id]).toEqual(['room_pin', '', ''])
    const e = withAction(d, 'external')
    expect([e.trunk_id, e.fallback_trunk_id]).toEqual(['t-a', 't-b'])
  })

  it('o padrão e a descrição seguem aparados', () => {
    const r = rulesFromDrafts([{ ...newDraft(), pattern: ' 2XX ', description: ' Fixos ', action: 'block' }])[0]
    expect([r.pattern, r.description]).toEqual(['2XX', 'Fixos'])
  })

  it('as quatro acções são as do contrato', () => {
    expect([...RULE_ACTIONS]).toEqual(['external', 'extension', 'room_pin', 'block'])
  })
})

describe('validação local de uma regra', () => {
  const base = () => ({ ...newDraft(), pattern: '9XXXXXXXX', description: 'Móveis', trunk_id: 't-a' })

  it('uma regra completa passa', () => {
    expect(validateDraft(base())).toEqual({})
    expect(validateDraft({ ...base(), action: 'block', trunk_id: '' })).toEqual({})
  })

  it('padrão e descrição são obrigatórios', () => {
    expect(validateDraft({ ...base(), pattern: '  ' }).pattern).toBe('padraoObrigatorio')
    expect(validateDraft({ ...base(), pattern: 'X'.repeat(129) }).pattern).toBe('padraoLongo')
    expect(validateDraft({ ...base(), description: '' }).description).toBe('descricao')
    expect(validateDraft({ ...base(), description: 'd'.repeat(121) }).description).toBe('descricao')
  })

  it('uma regra externa precisa de operadora, e a reserva tem de ser outra', () => {
    expect(validateDraft({ ...base(), trunk_id: '' }).trunk).toBe('operadoraObrigatoria')
    expect(validateDraft({ ...base(), fallback_trunk_id: 't-a' }).fallback).toBe('reservaIgual')
    expect(validateDraft({ ...base(), fallback_trunk_id: 't-b' })).toEqual({})
  })

  it('os erros ficam presos à regra pela chave, não pela posição', () => {
    const boa = base()
    const ma = { ...base(), pattern: '' }
    const erros = validateDrafts([boa, ma])
    expect(Object.keys(erros)).toEqual([ma.key])
    // Reordenar não troca os erros de regra.
    expect(Object.keys(validateDrafts(moveItem([boa, ma], 0, 1)))).toEqual([ma.key])
  })
})

describe('recusas do servidor: a regra apontada em `details[].field`', () => {
  it('`rules[N]` marca a regra N (a contar do zero)', () => {
    const e = recusa('telephony.emergency_cannot_be_blocked', ['rules[1]'])
    expect(errorCode(e)).toBe('telephony.emergency_cannot_be_blocked')
    expect(ruleErrorTarget(e)).toEqual({ index: 1, field: null })
    expect(errorKey(errorCode(e))).toBe('telecom.erro.emergency_cannot_be_blocked')
  })

  it('o padrão inválido vem com dois campos: ignora-se «pattern» solto e usa-se `rules[N].pattern`', () => {
    const e = recusa('telephony.invalid_pattern', ['pattern', 'rules[12].pattern'])
    expect(ruleErrorTarget(e)).toEqual({ index: 12, field: 'pattern' })
  })

  it('uma recusa do plano inteiro não aponta regra nenhuma', () => {
    expect(ruleErrorTarget(recusa('telephony.too_many_rules', []))).toBeNull()
    expect(ruleErrorTarget(recusa('telephony.invalid_rule_action', ['action']))).toBeNull()
    expect(ruleErrorTarget(new Error('rede'))).toBeNull()
  })

  it('cada código leva o foco ao campo certo da regra', () => {
    expect(ruleFieldForCode('telephony.emergency_cannot_be_blocked', null)).toBe('pattern')
    expect(ruleFieldForCode('telephony.invalid_pattern', 'pattern')).toBe('pattern')
    expect(ruleFieldForCode('telephony.rule_requires_trunk', null)).toBe('trunk')
    expect(ruleFieldForCode('telephony.unknown_trunk', null)).toBe('trunk')
    expect(ruleFieldForCode('telephony.fallback_equals_trunk', null)).toBe('fallback')
    expect(ruleFieldForCode('telephony.invalid_rule_description', null)).toBe('description')
    expect(ruleFieldForCode('telephony.emergency_never_recorded', null)).toBeNull()
    expect(ruleFieldForCode(null, null)).toBeNull()
  })

  it('a regra marcada é a que está NAQUELA posição no que se enviou — mesmo depois de reordenar', () => {
    const d = draftsFromRules(regras)
    const enviado = moveItem(d, 0, 1)
    const alvo = ruleErrorTarget(recusa('telephony.rule_requires_trunk', ['rules[1]']))
    expect(enviado[alvo?.index ?? -1].key).toBe(d[0].key)
  })
})

describe('reordenar regras por teclado', () => {
  it('subir e descer trocam com a vizinha e mantêm todas as regras', () => {
    const d = draftsFromRules(regras)
    const subiu = moveItem(d, 2, -1)
    expect(subiu.map((x) => x.pattern)).toEqual(['9XXXXXXXX', '112', '1XX'])
    expect(rulesFromDrafts(subiu).map((r) => r.pattern)).toEqual(['9XXXXXXXX', '112', '1XX'])
    expect(moveItem(d, 0, -1)).toBe(d)
    expect(moveItem(d, 2, 1)).toBe(d)
  })
})

describe('testar número', () => {
  it('os desfechos e as razões de não haver preço que o servidor emite têm chave; os novos saem tal qual', () => {
    for (const o of ['route', 'internal', 'blocked', 'no_match', 'no_available_trunk']) expect(outcomeKey(o)).toBe(`telecom.testar.desfecho.${o}`)
    expect(outcomeKey('queued')).toBeNull()
    expect(priceReasonKey('not_external')).toBe('telecom.testar.semPreco.not_external')
    expect(priceReasonKey('no_price_in_force')).toBe('telecom.testar.semPreco.no_price_in_force')
    expect(priceReasonKey('outra')).toBeNull()
  })

  it('a forma de um número: dígitos, espaços, hífens e um «+» inicial', () => {
    for (const ok of ['112', '923 447 108', '+244 923-447-108', '00271112345678']) expect(isDialable(ok), ok).toBe(true)
    for (const mau of ['', '   ', 'abc', '+', '9+2', '923a', '- -']) expect(isDialable(mau), mau).toBe(false)
  })

  it('há texto para cada desfecho e razão nas quatro línguas, e o ecrã diz que não liga a ninguém', async () => {
    for (const lang of ['pt', 'en', 'fr', 'zh']) {
      const loc = (await import(`../../locales/${lang}/telecom.ts`)).default as unknown as {
        testar: { nota: string; desfecho: Record<string, string>; semPreco: Record<string, string> }
      }
      for (const o of ['route', 'internal', 'blocked', 'no_match', 'no_available_trunk']) expect(loc.testar.desfecho[o], `${lang}.${o}`).toBeTruthy()
      for (const r of ['not_external', 'no_price_in_force']) expect(loc.testar.semPreco[r], `${lang}.${r}`).toBeTruthy()
      expect(loc.testar.nota, lang).toBeTruthy()
    }
    const pt = (await import('../../locales/pt/telecom')).default as unknown as { testar: { nota: string } }
    expect(pt.testar.nota).toContain('Não liga a ninguém')
  })
})
