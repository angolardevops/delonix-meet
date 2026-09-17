/**
 * O MESMO contrato das listas (`pesquisa.md` §2), cumprido sobre uma colecção
 * que já está INTEIRA no browser — para as listas cujo recurso ainda não tem
 * pesquisa no servidor (webhooks, chaves de API, grupos, organizações, o
 * histórico de chamadas, destinos e exportações do Estúdio).
 *
 * Não é a pesquisa profunda e não finge ser: só procura no que o endpoint de
 * sempre já devolveu, sem corte. Onde o servidor corta (a auditoria com
 * `limit`), NÃO se usa isto — contagens sobre uma amostra seriam falsas.
 *
 * Os períodos contam-se no fuso do browser (o servidor usa o da organização).
 * O `page_token` é a posição, em texto.
 */
import type { Domain, DomainNode, ListEnvelope, ListGroup, ListQuery, SearchSchema, SearchSchemaField } from '../../api'

export interface LocalSource<T> {
  schema: SearchSchema
  /** Valor de um campo do schema (datas em ISO, utilizadores pelo nome). */
  get: (row: T, field: string) => unknown
  /** Texto onde o `q` procura. */
  text: (row: T) => string
  /** Rótulo de um valor de grupo (nome de utilizador, filial…). */
  groupLabel?: (field: string, key: string) => string
  now?: () => Date
}

export function norm(v: string): string {
  return v.normalize('NFD').replace(/[\u0300-\u036f]/g, '').toLowerCase()
}

// ---------------------------------------------------------------------- datas

const pad = (n: number) => String(n).padStart(2, '0')
const startOfDay = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate())
const addDays = (d: Date, n: number) => new Date(d.getFullYear(), d.getMonth(), d.getDate() + n)

/** Semana ISO 8601 (`2026-W38`). */
export function isoWeek(d: Date): string {
  const t = new Date(Date.UTC(d.getFullYear(), d.getMonth(), d.getDate()))
  const day = t.getUTCDay() || 7
  t.setUTCDate(t.getUTCDate() + 4 - day)
  const y = t.getUTCFullYear()
  const w = Math.ceil(((t.getTime() - Date.UTC(y, 0, 1)) / 86_400_000 + 1) / 7)
  return `${y}-W${pad(w)}`
}

/** Chave de grupo de uma data, na forma do contrato. */
export function dateKey(d: Date, g: string): string {
  switch (g) {
    case 'day':
      return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`
    case 'week':
      return isoWeek(d)
    case 'quarter':
      return `${d.getFullYear()}-Q${Math.floor(d.getMonth() / 3) + 1}`
    case 'year':
      return String(d.getFullYear())
    default:
      return `${d.getFullYear()}-${pad(d.getMonth() + 1)}`
  }
}

function mondayOf(d: Date): Date {
  const s = startOfDay(d)
  return addDays(s, -((s.getDay() + 6) % 7))
}

/** `[de, até[` de um período do contrato. `null` = período desconhecido. */
export function periodRange(period: string, now: Date): [Date | null, Date | null] | null {
  const today = startOfDay(now)
  const y = now.getFullYear()
  const m = now.getMonth()
  const q = Math.floor(m / 3) * 3
  switch (period) {
    case 'today':
      return [today, addDays(today, 1)]
    case 'yesterday':
      return [addDays(today, -1), today]
    case 'this_week':
      return [mondayOf(now), addDays(mondayOf(now), 7)]
    case 'last_week':
      return [addDays(mondayOf(now), -7), mondayOf(now)]
    case 'this_month':
      return [new Date(y, m, 1), new Date(y, m + 1, 1)]
    case 'last_month':
      return [new Date(y, m - 1, 1), new Date(y, m, 1)]
    case 'this_quarter':
      return [new Date(y, q, 1), new Date(y, q + 3, 1)]
    case 'last_quarter':
      return [new Date(y, q - 3, 1), new Date(y, q, 1)]
    case 'this_year':
      return [new Date(y, 0, 1), new Date(y + 1, 0, 1)]
    case 'last_year':
      return [new Date(y - 1, 0, 1), new Date(y, 0, 1)]
    case 'last_7_days':
      return [addDays(today, -6), addDays(today, 1)]
    case 'last_30_days':
      return [addDays(today, -29), addDays(today, 1)]
    case 'next_7_days':
      return [today, addDays(today, 7)]
    case 'past':
      return [null, now]
    case 'future':
      return [now, null]
    default:
      return null
  }
}

// ---------------------------------------------------------------- avaliação

function isSet(v: unknown): boolean {
  return !(v === null || v === undefined || (typeof v === 'string' && v.trim() === ''))
}

function cmp(field: SearchSchemaField | undefined, a: unknown, b: unknown): number {
  if (field?.type === 'datetime') return new Date(String(a)).getTime() - new Date(String(b)).getTime()
  if (field?.type === 'number') return Number(a) - Number(b)
  return norm(String(a)).localeCompare(norm(String(b)))
}

function evalCond<T>(row: T, [name, op, value]: [string, string, unknown?], src: LocalSource<T>, now: Date): boolean {
  const field = src.schema.fields.find((f) => f.name === name)
  const v = src.get(row, name)
  const s = (x: unknown) => norm(String(x ?? ''))
  switch (op) {
    case 'is_set':
      return isSet(v)
    case 'is_not_set':
      return !isSet(v)
    case 'contains':
      return s(v).includes(s(value))
    case 'not_contains':
      return !s(v).includes(s(value))
    case 'starts_with':
      return s(v).startsWith(s(value))
    case 'eq':
      return field?.type === 'bool' ? Boolean(v) === (value === true || value === 'true') : isSet(v) && cmp(field, v, value) === 0
    case 'ne':
      return !isSet(v) || cmp(field, v, value) !== 0
    case 'in':
      return Array.isArray(value) && value.some((x) => isSet(v) && cmp(field, v, x) === 0)
    case 'not_in':
      return Array.isArray(value) && !value.some((x) => isSet(v) && cmp(field, v, x) === 0)
    case 'lt':
      return isSet(v) && cmp(field, v, value) < 0
    case 'lte':
      return isSet(v) && cmp(field, v, value) <= 0
    case 'gt':
      return isSet(v) && cmp(field, v, value) > 0
    case 'gte':
      return isSet(v) && cmp(field, v, value) >= 0
    case 'between':
      return Array.isArray(value) && isSet(v) && cmp(field, v, value[0]) >= 0 && cmp(field, v, value[1]) <= 0
    case 'in_period': {
      const r = periodRange(String(value), now)
      if (!r || !isSet(v)) return false
      const t = new Date(String(v)).getTime()
      return (!r[0] || t >= r[0].getTime()) && (!r[1] || t < r[1].getTime())
    }
    default:
      return false
  }
}

export function evalNode<T>(row: T, node: Domain, src: LocalSource<T>, now: Date): boolean {
  if (Array.isArray(node)) {
    if (typeof node[0] === 'string') return evalCond(row, node as [string, string, unknown?], src, now)
    return (node as DomainNode[]).every((n) => evalNode(row, n, src, now))
  }
  const o = node as { and?: DomainNode[]; or?: DomainNode[]; not?: DomainNode }
  if (o.and) return o.and.every((n) => evalNode(row, n, src, now))
  if (o.or) return o.or.some((n) => evalNode(row, n, src, now))
  if (o.not) return !evalNode(row, o.not, src, now)
  return true
}

/** Filtros pré-definidos: mesmo grupo = OU, grupos diferentes = E. */
function presetsMatch<T>(row: T, names: string[], src: LocalSource<T>, now: Date): boolean {
  const byGroup = new Map<string, Domain[]>()
  for (const n of names) {
    const f = src.schema.filters.find((x) => x.name === n)
    if (!f) continue
    byGroup.set(f.group, [...(byGroup.get(f.group) ?? []), f.filter])
  }
  for (const doms of byGroup.values()) if (!doms.some((d) => evalNode(row, d, src, now))) return false
  return true
}

function groupKeyOf<T>(row: T, groupBy: string, src: LocalSource<T>): string | null {
  const [name, gran] = groupBy.split(':')
  const field = src.schema.fields.find((f) => f.name === name)
  const v = src.get(row, name)
  if (!isSet(v)) return null
  if (field?.type === 'datetime') return dateKey(new Date(String(v)), gran ?? 'month')
  if (field?.type === 'bool') return v ? 'true' : 'false'
  return String(v)
}

function groupFilter(groupBy: string, key: string | null, src: LocalSource<unknown>, sample: unknown): Domain {
  const [name, gran] = groupBy.split(':')
  if (key === null) return [name, 'is_not_set']
  const field = src.schema.fields.find((f) => f.name === name)
  if (field?.type === 'bool') return [name, 'eq', key === 'true']
  if (field?.type === 'datetime') {
    // Intervalo do grupo, medido a partir de uma linha dele.
    const d = new Date(String(src.get(sample, name)))
    const from = gran === 'day' ? startOfDay(d) : gran === 'week' ? mondayOf(d) : gran === 'year' ? new Date(d.getFullYear(), 0, 1) : gran === 'quarter' ? new Date(d.getFullYear(), Math.floor(d.getMonth() / 3) * 3, 1) : new Date(d.getFullYear(), d.getMonth(), 1)
    const to = gran === 'day' ? addDays(from, 1) : gran === 'week' ? addDays(from, 7) : gran === 'year' ? new Date(from.getFullYear() + 1, 0, 1) : gran === 'quarter' ? new Date(from.getFullYear(), from.getMonth() + 3, 1) : new Date(from.getFullYear(), from.getMonth() + 1, 1)
    return { and: [[name, 'gte', from.toISOString()], [name, 'lt', to.toISOString()]] }
  }
  return [name, 'eq', key]
}

function orderRows<T>(rows: T[], order: string[], src: LocalSource<T>): T[] {
  if (!order.length) return rows
  return [...rows].sort((a, b) => {
    for (const o of order) {
      const desc = o.startsWith('-')
      const name = desc ? o.slice(1) : o
      const field = src.schema.fields.find((f) => f.name === name)
      const va = src.get(a, name)
      const vb = src.get(b, name)
      if (!isSet(va) && !isSet(vb)) continue
      if (!isSet(va)) return 1
      if (!isSet(vb)) return -1
      const c = cmp(field, va, vb)
      if (c !== 0) return desc ? -c : c
    }
    return 0
  })
}

/** Corre uma `ListQuery` sobre as linhas. Devolve o envelope do contrato. */
export function runLocal<T>(rows: T[], query: ListQuery, src: LocalSource<T>): ListEnvelope<T> {
  const now = src.now?.() ?? new Date()
  const words = norm(query.q ?? '').split(/\s+/).filter(Boolean)
  let hits = rows.filter((r) => {
    if (words.length) {
      const h = norm(src.text(r))
      if (!words.every((w) => h.includes(w))) return false
    }
    if (query.filter && !evalNode(r, query.filter, src, now)) return false
    if (query.filters?.length && !presetsMatch(r, query.filters, src, now)) return false
    return true
  })
  hits = orderRows(hits, query.order_by?.length ? query.order_by : src.schema.default_order, src)
  const size = Math.max(1, Math.min(100, query.page_size ?? 50))
  const start = Math.max(0, Number(query.page_token ?? 0) || 0)
  const env: ListEnvelope<T> = {
    items: hits.slice(start, start + size) as ListEnvelope<T>['items'],
    next_page_token: start + size < hits.length ? String(start + size) : null,
    total: hits.length,
    total_kind: 'exact',
  }
  const [first, ...rest] = query.group_by ?? []
  if (first) {
    const map = new Map<string | null, T[]>()
    for (const r of hits) {
      const k = groupKeyOf(r, first, src)
      map.set(k, [...(map.get(k) ?? []), r])
    }
    const [name] = first.split(':')
    const field = src.schema.fields.find((f) => f.name === name)
    const keys = [...map.keys()].sort((a, b) => (a === null ? 1 : b === null ? -1 : a.localeCompare(b)))
    env.groups = keys.map((key): ListGroup => {
      const members = map.get(key)!
      const aggregates: ListGroup['aggregates'] = {}
      for (const f of src.schema.fields) {
        if (!f.aggregates.length) continue
        const nums = members.map((m) => Number(src.get(m, f.name))).filter((n) => Number.isFinite(n))
        const sum = nums.reduce((a, b) => a + b, 0)
        aggregates[f.name] = {}
        if (f.aggregates.includes('sum')) aggregates[f.name].sum = sum
        if (f.aggregates.includes('avg')) aggregates[f.name].avg = nums.length ? sum / nums.length : 0
      }
      const label =
        key === null
          ? null
          : field?.type === 'enum'
            ? (field.options?.find((o) => o.value === key)?.label ?? key)
            : (src.groupLabel?.(name, key) ?? key)
      return {
        key,
        label,
        count: members.length,
        aggregates,
        filter: groupFilter(first, key, src as LocalSource<unknown>, members[0]),
        group_by: rest,
      }
    })
    env.next_groups_page_token = null
  }
  return env
}
