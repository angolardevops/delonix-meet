/**
 * Lógica da edição do plano de marcação — pura, sem React, para se poder testar.
 *
 * O plano grava-se INTEIRO (`putDialPlan` substitui tudo) e a ordem das regras
 * é a ordem em que casam. O servidor valida tudo; aqui só se apanha cedo o que
 * é óbvio, e traduz-se uma recusa para a regra que ele aponta
 * (`details[].field = "rules[N]"`, com N a contar do zero).
 */
import type { DialRule, DialRuleAction } from '../../api'
import { errorFields } from './trunkForm'

export const RULE_ACTIONS: readonly DialRuleAction[] = ['external', 'extension', 'room_pin', 'block']
export const MAX_RULES = 100
export const MAX_DESCRIPTION = 120
export const MAX_PATTERN = 128

/** Uma regra enquanto se edita: os selects usam '' para «nenhuma». */
export interface RuleDraft {
  /** Identidade estável enquanto o diálogo está aberto (a posição muda ao reordenar). */
  key: string
  pattern: string
  description: string
  action: string
  trunk_id: string
  fallback_trunk_id: string
  record: boolean
  /** Não se edita aqui: uma regra de emergência mantém-se como o servidor a deu. */
  emergency: boolean
}

let seq = 0
const nextKey = () => `r${++seq}`

export function draftsFromRules(rules: DialRule[]): RuleDraft[] {
  return rules.map((r) => ({
    key: nextKey(),
    pattern: r.pattern,
    description: r.description,
    action: r.action,
    trunk_id: r.trunk_id ?? '',
    fallback_trunk_id: r.fallback_trunk_id ?? '',
    record: r.record === true,
    emergency: r.emergency === true,
  }))
}

export function newDraft(): RuleDraft {
  return { key: nextKey(), pattern: '', description: '', action: 'external', trunk_id: '', fallback_trunk_id: '', record: false, emergency: false }
}

/**
 * Mudar a acção: só as regras externas escolhem operadora, por isso as outras
 * largam a operadora e a reserva (o servidor recusa-as: `rule_trunk_not_allowed`).
 */
export function withAction(d: RuleDraft, action: string): RuleDraft {
  return action === 'external' ? { ...d, action } : { ...d, action, trunk_id: '', fallback_trunk_id: '' }
}

/** O que vai no `PUT`: operadora e reserva só nas regras externas, vazio como null. */
export function rulesFromDrafts(drafts: RuleDraft[]): DialRule[] {
  return drafts.map((d) => {
    const external = d.action === 'external'
    return {
      pattern: d.pattern.trim(),
      description: d.description.trim(),
      action: d.action,
      trunk_id: external && d.trunk_id ? d.trunk_id : null,
      fallback_trunk_id: external && d.fallback_trunk_id ? d.fallback_trunk_id : null,
      record: d.record,
      emergency: d.emergency,
    }
  })
}

export type RuleField = 'pattern' | 'description' | 'trunk' | 'fallback'
/** Chaves de `telecom.planoEd.erro.*` por campo de uma regra. */
export type RuleErrors = Partial<Record<RuleField, string>>

export function validateDraft(d: RuleDraft): RuleErrors {
  const e: RuleErrors = {}
  const p = d.pattern.trim()
  if (p === '') e.pattern = 'padraoObrigatorio'
  else if (p.length > MAX_PATTERN) e.pattern = 'padraoLongo'
  const desc = d.description.trim()
  if (desc === '' || [...desc].length > MAX_DESCRIPTION) e.description = 'descricao'
  if (d.action === 'external') {
    if (!d.trunk_id) e.trunk = 'operadoraObrigatoria'
    else if (d.fallback_trunk_id && d.fallback_trunk_id === d.trunk_id) e.fallback = 'reservaIgual'
  }
  return e
}

/** Os erros locais por chave de regra; vazio = pode gravar. */
export function validateDrafts(drafts: RuleDraft[]): Record<string, RuleErrors> {
  const out: Record<string, RuleErrors> = {}
  for (const d of drafts) {
    const e = validateDraft(d)
    if (Object.keys(e).length > 0) out[d.key] = e
  }
  return out
}

/**
 * A regra que o servidor apontou: `rules[N]` ou `rules[N].pattern` em
 * `details[].field`. Devolve o índice (a contar do zero, como ele o manda) e o
 * sub-campo, se houver. Null = a recusa é do plano inteiro.
 */
export function ruleErrorTarget(err: unknown): { index: number; field: string | null } | null {
  for (const f of errorFields(err)) {
    const m = /^rules\[(\d+)\](?:\.([a-z_]+))?$/.exec(f)
    if (m) return { index: Number.parseInt(m[1], 10), field: m[2] ?? null }
  }
  return null
}

/** O campo de uma regra a que um código de erro pertence, para pôr lá o foco. */
export function ruleFieldForCode(code: string | null, sub: string | null): RuleField | null {
  if (sub === 'pattern') return 'pattern'
  switch (code) {
    case 'telephony.invalid_pattern':
    case 'telephony.emergency_cannot_be_blocked':
    case 'telephony.emergency_rule_without_emergency_number':
      return 'pattern'
    case 'telephony.invalid_rule_description':
      return 'description'
    case 'telephony.rule_requires_trunk':
    case 'telephony.unknown_trunk':
    case 'telephony.rule_trunk_not_allowed':
      return 'trunk'
    case 'telephony.fallback_equals_trunk':
      return 'fallback'
    default:
      return null
  }
}

// ------------------------------------------------------------------ testar número

const OUTCOMES = new Set(['route', 'internal', 'blocked', 'no_match', 'no_available_trunk'])

/** A chave de tradução do desfecho de um teste, ou null se for um valor novo. */
export function outcomeKey(outcome: string): string | null {
  return OUTCOMES.has(outcome) ? `telecom.testar.desfecho.${outcome}` : null
}

const PRICE_REASONS = new Set(['not_external', 'no_price_in_force'])

/** Porque é que não há preço estimado; null se a consola não conhece a razão. */
export function priceReasonKey(reason: string): string | null {
  return PRICE_REASONS.has(reason) ? `telecom.testar.semPreco.${reason}` : null
}

/** A forma que o servidor aceita num número: dígitos, espaços, hífens e um «+» inicial. */
export function isDialable(text: string): boolean {
  const t = text.trim()
  return t !== '' && /^\+?[\d\s-]+$/.test(t) && /\d/.test(t)
}
