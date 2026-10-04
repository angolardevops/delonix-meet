/**
 * Assistente «Ligar operadora móvel»: o que cada escolha pré-preenche.
 *
 * Só entra aqui o que é PADRÃO DA INDÚSTRIA numa interligação SIP com uma
 * operadora — TLS na 5061, SRTP obrigatório, âmbito nacional, interligação por
 * IP (sem registo). O que só o contrato de interligação diz fica VAZIO: o
 * endereço do SBC, o limite de canais, as credenciais e o preço. Nenhum preset
 * traz host, IP, porta contratada ou credencial de operadora nenhuma — não os
 * conhecemos, e um endereço inventado é uma chamada que sai para o sítio errado.
 *
 * Os prefixos são uma SUGESTÃO a confirmar com a operadora (a numeração muda,
 * e com portabilidade o prefixo deixa de dizer a rede): o ecrã marca-os assim.
 */
import { emptyTrunkForm } from './trunkForm'
import type { TrunkForm } from './trunkForm'

export type OperatorId = 'unitel' | 'africell' | 'movicel' | 'other'

export interface OperatorPreset {
  id: OperatorId
  /** Nome próprio da operadora (não se traduz); vazio em «Outra operadora». */
  name: string
  short_code: string
  /** Prefixos móveis nacionais SUGERIDOS — nunca um facto; vazio se não há certeza. */
  suggestedPrefixes: string[]
}

export const OPERATORS: readonly OperatorPreset[] = [
  { id: 'unitel', name: 'Unitel', short_code: 'UNI', suggestedPrefixes: ['92', '93', '94'] },
  { id: 'africell', name: 'Africell', short_code: 'AFR', suggestedPrefixes: ['95'] },
  { id: 'movicel', name: 'Movicel', short_code: 'MOV', suggestedPrefixes: ['91', '99'] },
  { id: 'other', name: '', short_code: '', suggestedPrefixes: [] },
]

/** Como a interligação é protegida: o habitual, ou o que resta quando a operadora só dá UDP. */
export type LinkSecurity = 'tls_srtp' | 'udp_plain'

/** O que se pede à operadora antes de preencher — chaves de `telecom.assistente.pedir.*`. */
export const ASK_OPERATOR = ['sbc', 'origem', 'autenticacao', 'codecs', 'dtmf', 'numeros', 'identidade', 'canais'] as const

export function operatorPreset(id: OperatorId): OperatorPreset {
  return OPERATORS.find((o) => o.id === id) ?? OPERATORS[OPERATORS.length - 1]
}

/**
 * O formulário de partida para uma operadora. Host, canais, utilizador,
 * password e preço saem SEMPRE vazios.
 */
export function presetForm(id: OperatorId, security: LinkSecurity = 'tls_srtp'): TrunkForm {
  const o = operatorPreset(id)
  const base = emptyTrunkForm()
  return {
    ...base,
    name: o.name,
    short_code: o.short_code,
    scope: 'national',
    register: false,
    prefixes: o.suggestedPrefixes.join(', '),
    ...(security === 'udp_plain' ? { transport: 'udp', srtp: 'off', port: '5060' } : { transport: 'tls', srtp: 'mandatory', port: '5061' }),
  }
}
