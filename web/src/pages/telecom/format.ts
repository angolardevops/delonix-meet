/**
 * Lógica de apresentação do ecrã «Telefonia» — pura, sem React, para se poder
 * testar. Três regras atravessam tudo o que aqui está:
 *
 *  1. Dinheiro chega como decimal em TEXTO e é formatado como texto. Nunca
 *     passa por um número de vírgula flutuante: 0.1 + 0.2 não é 0.3, e um
 *     total de facturação não pode depender disso.
 *  2. Um valor que o servidor não mediu vem a null, e null não é zero. Quem
 *     mostra decide entre «0» (mediu e deu zero) e «sem medição».
 *  3. Um código que a consola não conhece mostra-se tal qual. Não se
 *     «corrige» para um conhecido nem se esconde.
 */
import type { Money, SipRegistration, SipSettings, Trunk } from '../../api'
import type { Async } from '../../components/AsyncSection'
import type { BadgeTone } from '../../ui/kit'

// ---------------------------------------------------------------- dinheiro

const DECIMAL = /^(-?)(\d+)(?:\.(\d+))?$/

/** Casas decimais: tira os zeros à direita, mas nunca abaixo de duas. */
function trimFraction(frac: string): string {
  const semZeros = frac.replace(/0+$/, '')
  return semZeros.length >= 2 ? semZeros : semZeros.padEnd(2, '0')
}

function decimalSeparator(locale: string): string {
  try {
    return new Intl.NumberFormat(locale).formatToParts(1.5).find((p) => p.type === 'decimal')?.value ?? '.'
  } catch {
    return '.'
  }
}

function groupInteger(int: string, locale: string): string {
  try {
    return new Intl.NumberFormat(locale, { maximumFractionDigits: 0 }).format(BigInt(int))
  } catch {
    return int
  }
}

/**
 * Um decimal em texto, com os separadores da língua. Devolve null quando o
 * texto não é um decimal simples — quem chama mostra então o original.
 */
export function formatDecimal(amount: string, locale: string): string | null {
  const m = DECIMAL.exec(amount.trim())
  if (!m) return null
  const [, sinal, int, frac = ''] = m
  const inteiro = groupInteger(int.replace(/^0+(?=\d)/, ''), locale)
  return `${sinal}${inteiro}${decimalSeparator(locale)}${trimFraction(frac)}`
}

/** O kwanza escreve-se «Kz»; as outras moedas ficam pelo código ISO. */
export function currencyLabel(currency: string): string {
  return currency === 'AOA' ? 'Kz' : currency
}

export function formatMoney(m: Money, locale: string): string {
  const n = formatDecimal(m.amount, locale)
  return n === null ? `${m.amount} ${m.currency}` : `${n} ${currencyLabel(m.currency)}`
}

// ---------------------------------------------------------------- medições

/** O valor medido, ou null se o servidor não mediu. Zero é uma medição. */
export function measured(v: number | null | undefined): number | null {
  return typeof v === 'number' && Number.isFinite(v) ? v : null
}

export function formatNumber(v: number, locale: string, digits = 0): string {
  return v.toLocaleString(locale, { minimumFractionDigits: digits, maximumFractionDigits: digits })
}

/** Uma fracção 0..1 (o ASR) como percentagem inteira. */
export function formatRatio(v: number, locale: string): string {
  return `${formatNumber(v * 100, locale)}%`
}

// ------------------------------------------------------------------ razões

/** Os códigos de razão que o servidor emite hoje e que a consola sabe dizer. */
const REASONS = new Set([
  'sip_not_configured',
  'settings_missing',
  'not_configured',
  'media_server_unreachable',
  'sbc_not_configured',
  'sbc_unreachable',
  'trunk_down',
  'trunk_degraded',
  'registration_failed',
  'not_loaded_on_media_server',
  'options_ping_failed',
  'low_asr',
  'registering',
  'no_measurement',
  'sip_status_unavailable',
  'no_calls_in_window',
  'missing_exchange_rate',
  'inbound_not_billed',
  'no_trunk',
  'no_price_in_force',
])

/** A chave de tradução de uma razão, ou null se a consola não a conhece. */
export function reasonKey(code: string): string | null {
  return REASONS.has(code) ? `telecom.razao.${code}` : null
}

const KNOWN: Record<string, readonly string[]> = {
  sbc: ['healthy', 'degraded', 'down', 'not_configured'],
  tronco: ['up', 'degraded', 'down', 'unknown'],
  accao: ['external', 'extension', 'room_pin', 'block'],
  desfecho: ['answered', 'busy', 'no_answer', 'failed', 'forwarded', 'waiting_room', 'wrong_pin'],
  sentido: ['inbound', 'outbound'],
  srtp: ['mandatory', 'optional', 'off'],
}

/** A chave de tradução de um valor enumerado, ou null se for um valor novo. */
export function enumKey(grupo: keyof typeof KNOWN | string, valor: string): string | null {
  return KNOWN[grupo]?.includes(valor) ? `telecom.${grupo}.${valor}` : null
}

// ------------------------------------------------------------------ estados

export function sbcTone(state: string): BadgeTone {
  if (state === 'healthy') return 'success'
  if (state === 'degraded') return 'warning'
  if (state === 'down') return 'record'
  return 'neutral'
}

export function trunkTone(state: string): BadgeTone {
  if (state === 'up') return 'success'
  if (state === 'degraded') return 'warning'
  if (state === 'down') return 'record'
  return 'neutral'
}

/** Só os estados que têm operadoras, pela ordem em que interessam a quem opera. */
export function trunkCounts(t: SipRegistration['trunks']): { state: string; n: number }[] {
  return (['up', 'degraded', 'down', 'unknown'] as const).map((state) => ({ state, n: t[state] })).filter((x) => x.n > 0)
}

/** A ordem É o encaminhamento: quem mostra não a pode baralhar. */
export function sortTrunks(items: Trunk[]): Trunk[] {
  return [...items].sort((a, b) => a.position - b.position || a.name.localeCompare(b.name))
}

// --------------------------------------------------------------- paginação

/**
 * O cursor da página seguinte. O servidor OMITE o campo quando não há mais
 * páginas (não o manda a null), e um cursor vazio também não é cursor.
 */
export function nextToken(page: { next_page_token?: string | null }): string | null {
  const t = page.next_page_token
  return typeof t === 'string' && t !== '' ? t : null
}

// -------------------------------------------------------- o modo da página

export type PageMode = 'loading' | 'not_configured' | 'show'

/**
 * O que a página mostra. Sem definições SIP e sem operadoras não há telefonia:
 * diz-se isso, em vez de seis cartões de zeros. Um erro NÃO é «não
 * configurado» — cai em «show» e o cartão respectivo mostra o erro.
 */
export function pageMode(
  sip: Async<{ settings: SipSettings; registration: SipRegistration }>,
  trunks: Async<{ items: Trunk[] }>,
): PageMode {
  if (sip.s === 'loading' || trunks.s === 'loading') return 'loading'
  if (sip.s === 'ready' && trunks.s === 'ready' && !sip.d.settings.configured && trunks.d.items.length === 0) {
    return 'not_configured'
  }
  return 'show'
}
