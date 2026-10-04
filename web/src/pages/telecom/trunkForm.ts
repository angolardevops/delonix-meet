/**
 * Lógica do formulário de uma operadora (tronco SIP), dos preços e do câmbio —
 * pura, sem React, para se poder testar.
 *
 * O servidor é quem manda: o que aqui se valida é só para poupar uma viagem e
 * para avisar cedo (SRTP exige TLS). Uma recusa do servidor lê-se pelo CÓDIGO
 * (`telephony.*`) e pelo campo de `details[]`, nunca pelo texto.
 *
 * Dinheiro e taxas são decimais em TEXTO: valida-se a forma, e não passam por
 * `number` em sítio nenhum.
 */
import { ApiError } from '../../api'
import type { CreateTrunkReq, Money, SipTransport, SrtpMode, Trunk, TrunkScope, UpdateTrunkReq } from '../../api'

// ------------------------------------------------------------------ o formulário

export type TrunkField =
  | 'name'
  | 'short_code'
  | 'host'
  | 'port'
  | 'transport'
  | 'srtp'
  | 'scope'
  | 'prefixes'
  | 'max_channels'
  | 'username'
  | 'password'
  | 'price'

/** Tudo em texto, como está nos campos; só se converte ao montar o pedido. */
export interface TrunkForm {
  name: string
  short_code: string
  host: string
  port: string
  transport: string
  srtp: string
  scope: string
  prefixes: string
  max_channels: string
  register: boolean
  username: string
  /** Só de escrita. Vazio ao editar = manter a que existe. */
  password: string
  enabled: boolean
  /** Só ao criar: o preço inicial é opcional. */
  price_amount: string
  price_currency: string
}

export const TRANSPORTS: readonly SipTransport[] = ['tls', 'tcp', 'udp']
export const SRTP_MODES: readonly SrtpMode[] = ['mandatory', 'optional', 'off']
export const SCOPES: readonly TrunkScope[] = ['national', 'international']
/** Lista fechada no servidor (`money.rs`): acrescentar uma moeda é uma decisão de lá. */
export const CURRENCIES = ['AOA', 'USD'] as const
/** O câmbio é de uma moeda estrangeira PARA kwanzas: o kwanza não entra. */
export const RATE_CURRENCIES = ['USD'] as const

export const MAX_NAME = 80
export const MAX_PREFIXES = 20
export const MAX_PREFIX_LEN = 8
export const MAX_CHANNELS = 10000

export function emptyTrunkForm(): TrunkForm {
  return {
    name: '',
    short_code: '',
    host: '',
    port: '5061',
    transport: 'tls',
    srtp: 'mandatory',
    scope: 'national',
    prefixes: '',
    max_channels: '',
    register: false,
    username: '',
    password: '',
    enabled: true,
    price_amount: '',
    price_currency: 'AOA',
  }
}

export function formFromTrunk(k: Trunk): TrunkForm {
  return {
    name: k.name,
    short_code: k.short_code,
    host: k.host,
    port: String(k.port),
    transport: k.transport,
    srtp: k.srtp,
    scope: k.scope,
    prefixes: k.prefixes.join(', '),
    max_channels: String(k.max_channels),
    register: k.register,
    username: k.username,
    password: '',
    enabled: k.enabled,
    price_amount: '',
    price_currency: 'AOA',
  }
}

// ------------------------------------------------------------------ prefixos

/** Separa o que se escreveu num campo: vírgulas, ponto e vírgula, espaços ou «·». */
export function splitPrefixes(text: string): string[] {
  const out: string[] = []
  for (const p of text.split(/[\s,;·]+/)) if (p !== '' && !out.includes(p)) out.push(p)
  return out
}

/** A mesma regra do servidor: dígitos, com «+» opcional no início, até 8 caracteres. */
export function isPrefix(p: string): boolean {
  return p.length <= MAX_PREFIX_LEN && /^(\+\d*|\d+)$/.test(p)
}

export type PrefixProblem = { key: 'prefixosDemais' } | { key: 'prefixoInvalido'; prefix: string } | null

export function prefixProblem(text: string): PrefixProblem {
  const list = splitPrefixes(text)
  if (list.length > MAX_PREFIXES) return { key: 'prefixosDemais' }
  const mau = list.find((p) => !isPrefix(p))
  return mau === undefined ? null : { key: 'prefixoInvalido', prefix: mau }
}

// ------------------------------------------------------------------ dinheiro

/** Um preço: decimal positivo, até 4 casas, vírgula ou ponto. Só a FORMA. */
export function isAmountText(s: string): boolean {
  return /^\d{1,15}([.,]\d{1,4})?$/.test(s.trim())
}

/** Uma taxa «Kz por unidade»: até 9 dígitos e 6 casas, e maior do que zero. */
export function isRateText(s: string): boolean {
  const t = s.trim()
  return /^\d{1,9}([.,]\d{1,6})?$/.test(t) && /[1-9]/.test(t)
}

/** O texto como o servidor o guarda: ponto decimal. Não mexe em mais nada. */
export function decimalText(s: string): string {
  return s.trim().replace(',', '.')
}

export function moneyFrom(amount: string, currency: string): Money {
  return { amount: decimalText(amount), currency }
}

/**
 * Um `datetime-local` (hora de quem escreve) como instante RFC 3339, que é o
 * que o servidor aceita em `valid_from`. Vazio = «a partir de agora» (omite-se);
 * um texto que não é data devolve null.
 */
export function localToIso(value: string): string | undefined | null {
  if (value.trim() === '') return undefined
  const d = new Date(value)
  return Number.isNaN(d.getTime()) ? null : d.toISOString()
}

// ------------------------------------------------------------------ validação

/** SRTP diferente de `off` só com TLS: as chaves SDES viajam na sinalização. */
export function srtpNeedsTls(transport: string, srtp: string): boolean {
  return srtp !== 'off' && transport !== 'tls'
}

/** Sinalização ou media em claro: só por rede privada (regra de casa da interligação). */
export function isCleartext(transport: string, srtp: string): boolean {
  return transport !== 'tls' || srtp === 'off'
}

function intIn(text: string, min: number, max: number): boolean {
  if (!/^\d{1,6}$/.test(text.trim())) return false
  const n = Number.parseInt(text.trim(), 10)
  return n >= min && n <= max
}

/** Chaves de `telecom.form.erro.*` por campo; vazio = pode seguir. */
export type TrunkFormErrors = Partial<Record<TrunkField, { key: string; vars?: Record<string, string | number> }>>

export function validateTrunkForm(f: TrunkForm, mode: 'create' | 'edit'): TrunkFormErrors {
  const e: TrunkFormErrors = {}
  const name = f.name.trim()
  if (name === '' || [...name].length > MAX_NAME) e.name = { key: 'nome', vars: { max: MAX_NAME } }
  if (!/^[A-Za-z0-9]{2,4}$/.test(f.short_code.trim())) e.short_code = { key: 'sigla' }
  const host = f.host.trim()
  if (host === '') e.host = { key: 'hostObrigatorio' }
  else if (host.length > 253 || !/^[A-Za-z0-9.\-:[\]]+$/.test(host) || host.startsWith('-') || host.includes('..')) e.host = { key: 'host' }
  if (!intIn(f.port, 1, 65535)) e.port = { key: 'porta' }
  if (srtpNeedsTls(f.transport, f.srtp)) e.srtp = { key: 'srtpExigeTls' }
  const p = prefixProblem(f.prefixes)
  if (p) e.prefixes = p.key === 'prefixosDemais' ? { key: 'prefixosDemais', vars: { max: MAX_PREFIXES } } : { key: 'prefixoInvalido', vars: { prefixo: p.prefix } }
  if (f.max_channels.trim() === '') e.max_channels = { key: 'canaisObrigatorio' }
  else if (!intIn(f.max_channels, 1, MAX_CHANNELS)) e.max_channels = { key: 'canais', vars: { max: MAX_CHANNELS } }
  if (f.username.length > 128) e.username = { key: 'utilizador' }
  if (f.password.length > 256) e.password = { key: 'password' }
  if (mode === 'create' && f.price_amount.trim() !== '' && !isAmountText(f.price_amount)) e.price = { key: 'preco' }
  return e
}

// ------------------------------------------------------------------ pedidos

interface Normalized {
  name: string
  short_code: string
  host: string
  port: number
  transport: string
  srtp: string
  scope: string
  prefixes: string[]
  max_channels: number
  register: boolean
  username: string
  enabled: boolean
}

function normalize(f: TrunkForm): Normalized {
  return {
    name: f.name.trim(),
    short_code: f.short_code.trim().toUpperCase(),
    host: f.host.trim().toLowerCase(),
    port: Number.parseInt(f.port.trim(), 10),
    transport: f.transport,
    srtp: f.srtp,
    scope: f.scope,
    prefixes: splitPrefixes(f.prefixes),
    max_channels: Number.parseInt(f.max_channels.trim(), 10),
    register: f.register,
    username: f.username.trim(),
    enabled: f.enabled,
  }
}

/** O corpo de `createTrunk`. A password e o preço só vão se foram escritos. */
export function createBody(f: TrunkForm): CreateTrunkReq {
  const n = normalize(f)
  const body: CreateTrunkReq = {
    name: n.name,
    short_code: n.short_code,
    host: n.host,
    max_channels: n.max_channels,
    port: n.port,
    transport: n.transport as SipTransport,
    srtp: n.srtp as SrtpMode,
    scope: n.scope as TrunkScope,
    prefixes: n.prefixes,
    enabled: n.enabled,
    // Sempre explícito: por omissão o servidor liga o registo.
    register: n.register,
    username: n.username,
  }
  if (f.password !== '') body.password = f.password
  if (f.price_amount.trim() !== '') body.price_per_min = moneyFrom(f.price_amount, f.price_currency)
  return body
}

const sameList = (a: string[], b: string[]) => a.length === b.length && a.every((x, i) => x === b[i])

/**
 * O corpo de `updateTrunk`: SÓ o que mudou. A password vazia não vai — vazio
 * é «manter» (R214: a que existe nunca veio, por isso não há com que comparar).
 * Um objecto vazio quer dizer que não há nada para gravar.
 */
export function patchBody(original: Trunk, f: TrunkForm): UpdateTrunkReq {
  const n = normalize(f)
  const body: UpdateTrunkReq = {}
  if (n.name !== original.name) body.name = n.name
  if (n.short_code !== original.short_code) body.short_code = n.short_code
  if (n.host !== original.host) body.host = n.host
  if (n.port !== original.port) body.port = n.port
  if (n.transport !== original.transport) body.transport = n.transport as SipTransport
  if (n.srtp !== original.srtp) body.srtp = n.srtp as SrtpMode
  if (n.scope !== original.scope) body.scope = n.scope as TrunkScope
  if (!sameList(n.prefixes, original.prefixes)) body.prefixes = n.prefixes
  if (n.max_channels !== original.max_channels) body.max_channels = n.max_channels
  if (n.register !== original.register) body.register = n.register
  if (n.username !== original.username) body.username = n.username
  if (n.enabled !== original.enabled) body.enabled = n.enabled
  if (f.password !== '') body.password = f.password
  return body
}

// ------------------------------------------------------------------ recusas

/** O código de erro de uma recusa da API (qualquer espaço de nomes), ou null. */
export function errorCode(err: unknown): string | null {
  if (!(err instanceof ApiError)) return null
  const code = (err.body as { code?: unknown } | null)?.code
  return typeof code === 'string' ? code : null
}

/** Os campos que o servidor apontou em `details[]`. */
export function errorFields(err: unknown): string[] {
  if (!(err instanceof ApiError)) return []
  const details = (err.body as { details?: unknown } | null)?.details
  if (!Array.isArray(details)) return []
  return details.map((d) => (d as { field?: unknown } | null)?.field).filter((f): f is string => typeof f === 'string')
}

/** Os códigos que a consola sabe dizer na língua de quem lê (`telecom.erro.*`). */
const ERROR_CODES = new Set([
  'telephony.srtp_requires_tls',
  'telephony.trunk_host_refused',
  'telephony.trunk_name_taken',
  'telephony.trunk_in_use',
  'telephony.invalid_trunk_name',
  'telephony.invalid_short_code',
  'telephony.invalid_trunk_host',
  'telephony.invalid_port',
  'telephony.invalid_transport',
  'telephony.invalid_srtp',
  'telephony.invalid_scope',
  'telephony.invalid_prefixes',
  'telephony.invalid_max_channels',
  'telephony.invalid_username',
  'telephony.invalid_password',
  'telephony.invalid_trunk_order',
  'telephony.invalid_amount',
  'telephony.invalid_currency',
  'telephony.invalid_rate',
  'telephony.price_backdated',
  'telephony.price_exists',
  'telephony.rate_exists',
  'telephony.emergency_cannot_be_blocked',
  'telephony.emergency_must_be_external',
  'telephony.emergency_never_recorded',
  'telephony.emergency_rule_without_emergency_number',
  'telephony.rule_requires_trunk',
  'telephony.rule_trunk_not_allowed',
  'telephony.fallback_equals_trunk',
  'telephony.unknown_trunk',
  'telephony.invalid_pattern',
  'telephony.invalid_rule_description',
  'telephony.invalid_rule_action',
  'telephony.too_many_rules',
  'telephony.invalid_number',
  'secrets.encryption_unconfigured',
])

/** A chave de tradução de um código de erro, ou null se a consola não o conhece. */
export function errorKey(code: string | null): string | null {
  if (!code || !ERROR_CODES.has(code)) return null
  return `telecom.erro.${code.slice(code.indexOf('.') + 1)}`
}

const TRUNK_FIELDS: Record<string, TrunkField> = {
  name: 'name',
  short_code: 'short_code',
  host: 'host',
  port: 'port',
  transport: 'transport',
  srtp: 'srtp',
  scope: 'scope',
  prefixes: 'prefixes',
  max_channels: 'max_channels',
  username: 'username',
  password: 'password',
  currency: 'price',
  price_per_min: 'price',
}

/** Recusas que não trazem `details[]` mas que têm um campo óbvio. */
const FIELD_BY_CODE: Record<string, TrunkField> = {
  'telephony.trunk_name_taken': 'name',
  'telephony.invalid_amount': 'price',
  'telephony.invalid_currency': 'price',
  'secrets.encryption_unconfigured': 'password',
}

/**
 * O campo do formulário de operadora a que uma recusa do servidor pertence:
 * primeiro o que ele aponta em `details[].field`, depois o que o código diz.
 * Null = o erro é do pedido inteiro e mostra-se no fundo do formulário.
 */
export function trunkErrorField(err: unknown): TrunkField | null {
  for (const f of errorFields(err)) if (TRUNK_FIELDS[f]) return TRUNK_FIELDS[f]
  const code = errorCode(err)
  return (code && FIELD_BY_CODE[code]) || null
}

/** A que campo pertence uma recusa do servidor ao criar um preço ou uma taxa de câmbio. */
export function dateOrAmountField(code: string | null): 'amount' | 'valid_from' | null {
  if (code === 'telephony.price_backdated' || code === 'telephony.price_exists' || code === 'telephony.rate_exists') return 'valid_from'
  if (code === 'telephony.invalid_amount' || code === 'telephony.invalid_rate') return 'amount'
  return null
}

// ------------------------------------------------------------------ ordem

/**
 * A lista com o elemento `index` um lugar acima (-1) ou abaixo (+1). Fora dos
 * limites devolve a MESMA lista — quem chama sabe então que não há nada a gravar.
 */
export function moveItem<T>(list: T[], index: number, delta: -1 | 1): T[] {
  const to = index + delta
  if (index < 0 || index >= list.length || to < 0 || to >= list.length) return list
  const out = [...list]
  ;[out[index], out[to]] = [out[to], out[index]]
  return out
}

/** Arrastar: tira de `from` e põe em `to`. Posições inválidas ou iguais não mexem. */
export function moveTo<T>(list: T[], from: number, to: number): T[] {
  if (from === to || from < 0 || to < 0 || from >= list.length || to >= list.length) return list
  const out = [...list]
  const [x] = out.splice(from, 1)
  out.splice(to, 0, x)
  return out
}
