/**
 * Estado do painel de pesquisa estilo Odoo, e a tradução dele para o contrato
 * das listas (`docs/reference/pesquisa.md` §2): `q`, `filter` (domínio),
 * `filters` (pré-definidos), `group_by`, `order_by`, `page_size`.
 *
 * Cada FACETA da caixa de pesquisa é uma parte do estado:
 *   - «Pesquisar: x»                → `q` (texto livre nos campos do `text_search`)
 *   - «Pesquisar Título por: x ou y» → termo: `{or: [[title, contains, x], …]}`
 *   - «Filtro: As minhas ou …»      → nomes pré-definidos de UM grupo do schema
 *   - «Filtro: Duração > 60 e …»    → filtro personalizado (condições com E/OU)
 *   - «Agrupar por: Autor > Mês»    → `group_by`, por ordem
 *
 * Tudo é serializável na query do hash (`#/recordings?f=mine&g=uploader`), para
 * partilhar o link e para o botão voltar.
 */
import type { Domain, DomainNode, ListQuery, SavedSearchQuery, SearchOperator, SearchSchema, SearchSchemaField } from '../../api'

export interface CustomCondition {
  field: string
  op: SearchOperator
  value?: unknown
}

export interface CustomFilter {
  join: 'and' | 'or'
  conds: CustomCondition[]
}

export interface SearchTerm {
  field: string
  values: string[]
}

export interface SearchState {
  /** Valores de texto livre; vão juntos em `q` (o servidor junta termos com E). */
  q: string[]
  terms: SearchTerm[]
  filters: string[]
  custom: CustomFilter[]
  groupBy: string[]
  orderBy: string[]
  pageSize: number
}

export const DEFAULT_PAGE_SIZE = 50
export const MAX_PAGE_SIZE = 100
export const MAX_GROUP_BY = 3

export const EMPTY_SEARCH: SearchState = { q: [], terms: [], filters: [], custom: [], groupBy: [], orderBy: [], pageSize: DEFAULT_PAGE_SIZE }

export function isEmptySearch(s: SearchState): boolean {
  return !s.q.length && !s.terms.length && !s.filters.length && !s.custom.length && !s.groupBy.length && !s.orderBy.length
}

/** Operadores que não levam valor. */
export const NO_VALUE_OPS: SearchOperator[] = ['is_set', 'is_not_set']

export function fieldOf(schema: Pick<SearchSchema, 'fields'>, name: string): SearchSchemaField | undefined {
  return schema.fields.find((f) => f.name === name)
}

/** O nó de um termo «Pesquisar <campo> por»: contém (texto), igual/em (enum/bool). */
export function termNode(field: SearchSchemaField | undefined, term: SearchTerm): DomainNode {
  const vals = term.values
  if (field?.type === 'enum') return vals.length === 1 ? [term.field, 'eq', vals[0]] : [term.field, 'in', vals]
  const op: SearchOperator = field?.operators.includes('contains') || !field ? 'contains' : 'eq'
  const conds = vals.map((v) => [term.field, op, v] as DomainNode)
  return conds.length === 1 ? conds[0] : { or: conds }
}

export function conditionNode(c: CustomCondition): DomainNode {
  return NO_VALUE_OPS.includes(c.op) ? [c.field, c.op] : [c.field, c.op, c.value]
}

export function customNode(f: CustomFilter): DomainNode {
  const conds = f.conds.map(conditionNode)
  return conds.length === 1 ? conds[0] : f.join === 'or' ? { or: conds } : { and: conds }
}

/** O domínio do estado: lista no topo = E, um nó por faceta. */
export function stateDomain(state: SearchState, schema: Pick<SearchSchema, 'fields'>): DomainNode[] {
  return [...state.terms.map((t) => termNode(fieldOf(schema, t.field), t)), ...state.custom.filter((c) => c.conds.length).map(customNode)]
}

export function toListQuery(state: SearchState, schema: Pick<SearchSchema, 'fields'>): ListQuery {
  const filter = stateDomain(state, schema)
  return {
    q: state.q.join(' ').trim() || undefined,
    filter: filter.length ? filter : undefined,
    filters: state.filters.length ? state.filters : undefined,
    group_by: state.groupBy.length ? state.groupBy : undefined,
    order_by: state.orderBy.length ? state.orderBy : undefined,
    page_size: state.pageSize,
  }
}

export function toSavedQuery(state: SearchState, schema: Pick<SearchSchema, 'fields'>): SavedSearchQuery {
  const q = toListQuery(state, schema)
  return { q: q.q ?? '', filter: q.filter ?? [], filters: state.filters, group_by: state.groupBy, order_by: state.orderBy }
}

function isCond(n: unknown): n is [string, SearchOperator, unknown?] {
  return Array.isArray(n) && typeof n[0] === 'string' && typeof n[1] === 'string' && n.length <= 3
}

/**
 * Reconstrói as facetas a partir de um domínio guardado (favorito, link antigo).
 * Um nó que tem a forma de um termo volta a ser termo; o resto vira filtro
 * personalizado — nunca se perde uma condição.
 */
export function domainToFacets(filter: Domain | null | undefined, schema: Pick<SearchSchema, 'fields'>): Pick<SearchState, 'terms' | 'custom'> {
  const terms: SearchTerm[] = []
  const custom: CustomFilter[] = []
  if (!filter) return { terms, custom }
  const nodes: unknown[] = isCond(filter) ? [filter] : Array.isArray(filter) ? filter : [filter]
  for (const n of nodes) {
    const asTerm = nodeAsTerm(n, schema)
    if (asTerm) {
      terms.push(asTerm)
      continue
    }
    const c = nodeAsCustom(n)
    if (c) custom.push(c)
  }
  return { terms, custom }
}

function nodeAsTerm(n: unknown, schema: Pick<SearchSchema, 'fields'>): SearchTerm | null {
  if (isCond(n)) {
    const f = fieldOf(schema, n[0])
    if (!f) return null
    if (f.type === 'enum' && n[1] === 'eq' && typeof n[2] === 'string') return { field: n[0], values: [n[2]] }
    if (f.type === 'enum' && n[1] === 'in' && Array.isArray(n[2])) return { field: n[0], values: n[2].map(String) }
    if (f.type === 'text' && n[1] === 'contains' && typeof n[2] === 'string') return { field: n[0], values: [n[2]] }
    return null
  }
  const or = (n as { or?: unknown[] })?.or
  if (Array.isArray(or) && or.length > 1 && or.every((c) => isCond(c) && c[1] === 'contains' && typeof c[2] === 'string')) {
    const field = (or[0] as [string])[0]
    if (or.every((c) => (c as [string])[0] === field) && fieldOf(schema, field)?.type === 'text') {
      return { field, values: or.map((c) => String((c as [string, string, string])[2])) }
    }
  }
  return null
}

function nodeAsCustom(n: unknown): CustomFilter | null {
  if (isCond(n)) return { join: 'and', conds: [{ field: n[0], op: n[1], value: n[2] }] }
  const o = n as { and?: unknown[]; or?: unknown[] }
  const list = o?.and ?? o?.or
  if (Array.isArray(list) && list.every(isCond)) {
    return { join: o.or ? 'or' : 'and', conds: (list as [string, SearchOperator, unknown?][]).map((c) => ({ field: c[0], op: c[1], value: c[2] })) }
  }
  // Nós aninhados que o construtor não desenha (not, E dentro de OU…) ficam
  // fora do painel: o favorito aplica-se pelo `valid` do servidor.
  return null
}

export function fromSavedQuery(q: SavedSearchQuery, schema: Pick<SearchSchema, 'fields'>, pageSize = DEFAULT_PAGE_SIZE): SearchState {
  const { terms, custom } = domainToFacets(q.filter ?? null, schema)
  return {
    q: q.q?.trim() ? [q.q.trim()] : [],
    terms,
    filters: q.filters ?? [],
    custom,
    groupBy: (q.group_by ?? []).slice(0, MAX_GROUP_BY),
    orderBy: q.order_by ?? [],
    pageSize,
  }
}

// ------------------------------------------------------------ edição do estado

export function addQ(s: SearchState, v: string): SearchState {
  const t = v.trim()
  return !t || s.q.includes(t) ? s : { ...s, q: [...s.q, t] }
}

export function addTerm(s: SearchState, field: string, v: string): SearchState {
  const t = v.trim()
  if (!t) return s
  const i = s.terms.findIndex((x) => x.field === field)
  if (i < 0) return { ...s, terms: [...s.terms, { field, values: [t] }] }
  if (s.terms[i].values.includes(t)) return s
  const terms = s.terms.slice()
  terms[i] = { field, values: [...terms[i].values, t] }
  return { ...s, terms }
}

export function toggleFilter(s: SearchState, name: string): SearchState {
  return { ...s, filters: s.filters.includes(name) ? s.filters.filter((f) => f !== name) : [...s.filters, name] }
}

/** Liga/desliga um nível de agrupamento; a ordem dos cliques é a ordem dos níveis. */
export function toggleGroupBy(s: SearchState, value: string): SearchState {
  const field = value.split(':')[0]
  const has = s.groupBy.indexOf(value)
  if (has >= 0) return { ...s, groupBy: s.groupBy.filter((g) => g !== value) }
  // Outro nível do MESMO campo (mês → ano) substitui, não acumula.
  const same = s.groupBy.findIndex((g) => g.split(':')[0] === field)
  if (same >= 0) {
    const groupBy = s.groupBy.slice()
    groupBy[same] = value
    return { ...s, groupBy }
  }
  if (s.groupBy.length >= MAX_GROUP_BY) return s
  return { ...s, groupBy: [...s.groupBy, value] }
}

export type FacetRef =
  | { kind: 'q' }
  | { kind: 'term'; field: string }
  | { kind: 'filters'; group: string }
  | { kind: 'custom'; index: number }
  | { kind: 'groupBy' }

export function removeFacet(s: SearchState, ref: FacetRef, schema: Pick<SearchSchema, 'filters'>): SearchState {
  switch (ref.kind) {
    case 'q':
      return { ...s, q: [] }
    case 'term':
      return { ...s, terms: s.terms.filter((t) => t.field !== ref.field) }
    case 'filters': {
      const inGroup = new Set(schema.filters.filter((f) => f.group === ref.group).map((f) => f.name))
      return { ...s, filters: s.filters.filter((f) => !inGroup.has(f)) }
    }
    case 'custom':
      return { ...s, custom: s.custom.filter((_, i) => i !== ref.index) }
    case 'groupBy':
      return { ...s, groupBy: [] }
  }
}

/** A faceta que o Backspace num campo vazio remove: a última. */
export function lastFacet(s: SearchState, schema: Pick<SearchSchema, 'filters'>): FacetRef | null {
  const refs = facetRefs(s, schema)
  return refs.length ? refs[refs.length - 1] : null
}

/** Facetas pela ordem em que aparecem na caixa. */
export function facetRefs(s: SearchState, schema: Pick<SearchSchema, 'filters'>): FacetRef[] {
  const out: FacetRef[] = []
  if (s.q.length) out.push({ kind: 'q' })
  for (const t of s.terms) out.push({ kind: 'term', field: t.field })
  const groups: string[] = []
  for (const name of s.filters) {
    const g = schema.filters.find((f) => f.name === name)?.group ?? name
    if (!groups.includes(g)) groups.push(g)
  }
  for (const g of groups) out.push({ kind: 'filters', group: g })
  s.custom.forEach((_, index) => out.push({ kind: 'custom', index }))
  if (s.groupBy.length) out.push({ kind: 'groupBy' })
  return out
}

// ------------------------------------------------------------------ URL (hash)

const K = { q: 'q', terms: 't', filters: 'f', custom: 'c', groupBy: 'g', orderBy: 'o', pageSize: 'n' } as const

/** Escreve o estado em parâmetros do hash, com prefixo por painel (`members.f`). */
export function encodeSearch(s: SearchState, params: URLSearchParams, ns = ''): URLSearchParams {
  const p = new URLSearchParams(params)
  for (const k of Object.values(K)) p.delete(ns + k)
  for (const v of s.q) p.append(ns + K.q, v)
  if (s.terms.length) p.set(ns + K.terms, JSON.stringify(s.terms.map((t) => [t.field, ...t.values])))
  if (s.filters.length) p.set(ns + K.filters, s.filters.join(','))
  if (s.custom.length) p.set(ns + K.custom, JSON.stringify(s.custom))
  if (s.groupBy.length) p.set(ns + K.groupBy, s.groupBy.join(','))
  if (s.orderBy.length) p.set(ns + K.orderBy, s.orderBy.join(','))
  if (s.pageSize !== DEFAULT_PAGE_SIZE) p.set(ns + K.pageSize, String(s.pageSize))
  return p
}

function parseJson(v: string | null): unknown {
  if (!v) return null
  try {
    return JSON.parse(v)
  } catch {
    return null
  }
}

const list = (v: string | null) => (v ? v.split(',').map((x) => x.trim()).filter(Boolean) : [])

/** Lê o estado dos parâmetros; o que não se consegue ler é ignorado, nunca rebenta. */
export function decodeSearch(params: URLSearchParams, ns = ''): SearchState {
  const terms: SearchTerm[] = []
  const rawTerms = parseJson(params.get(ns + K.terms))
  if (Array.isArray(rawTerms)) {
    for (const t of rawTerms) {
      if (Array.isArray(t) && t.length > 1 && t.every((x) => typeof x === 'string')) terms.push({ field: t[0], values: t.slice(1) })
    }
  }
  const custom: CustomFilter[] = []
  const rawCustom = parseJson(params.get(ns + K.custom))
  if (Array.isArray(rawCustom)) {
    for (const c of rawCustom) {
      const conds = (c as CustomFilter)?.conds
      if (Array.isArray(conds) && conds.every((x) => typeof x?.field === 'string' && typeof x?.op === 'string')) {
        custom.push({ join: (c as CustomFilter).join === 'or' ? 'or' : 'and', conds })
      }
    }
  }
  const n = Number(params.get(ns + K.pageSize))
  return {
    q: params.getAll(ns + K.q).map((v) => v.trim()).filter(Boolean),
    terms,
    filters: list(params.get(ns + K.filters)),
    custom,
    groupBy: list(params.get(ns + K.groupBy)).slice(0, MAX_GROUP_BY),
    orderBy: list(params.get(ns + K.orderBy)).slice(0, 3),
    pageSize: Number.isInteger(n) && n >= 1 && n <= MAX_PAGE_SIZE ? n : DEFAULT_PAGE_SIZE,
  }
}

/** Parâmetros da query do hash corrente (`#/recordings?f=mine` → `f=mine`). */
export function hashParams(hash = typeof location === 'undefined' ? '' : location.hash): URLSearchParams {
  const i = hash.indexOf('?')
  return new URLSearchParams(i < 0 ? '' : hash.slice(i + 1))
}

export function hashWithParams(hash: string, params: URLSearchParams): string {
  const i = hash.indexOf('?')
  const path = (i < 0 ? hash : hash.slice(0, i)).replace(/^#/, '')
  const qs = params.toString()
  return qs ? `${path}?${qs}` : path
}

// ------------------------------------------------------------------- paginador

/** «1-50», «51-100»… lido do campo editável do paginador. */
export function parseRange(text: string): { start: number; end: number } | null {
  const m = text.trim().match(/^(\d+)\s*[-–]\s*(\d+)$/)
  if (!m) return null
  const start = Number(m[1])
  const end = Number(m[2])
  if (start < 1 || end < start) return null
  return { start, end }
}

/**
 * O que um intervalo escrito quer dizer num cursor keyset (sem OFFSET): muda o
 * tamanho da página e vai a uma página JÁ visitada. Saltar para uma posição
 * nunca vista não é possível — o servidor não tem OFFSET (§2.1, keyset).
 */
export function rangeToPage(
  r: { start: number; end: number },
  pageSize: number,
  knownPages: number,
): { pageSize: number; pageIndex: number } | null {
  const size = Math.min(MAX_PAGE_SIZE, r.end - r.start + 1)
  if (size !== pageSize) return r.start === 1 ? { pageSize: size, pageIndex: 0 } : null
  if ((r.start - 1) % size !== 0) return null
  const idx = (r.start - 1) / size
  return idx < knownPages ? { pageSize: size, pageIndex: idx } : null
}
