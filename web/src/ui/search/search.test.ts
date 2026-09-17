import { describe, expect, it, vi } from 'vitest'
import type { Domain, SearchSchema } from '../../api'
import { dateKey, isoWeek, periodRange, runLocal, LocalSource } from './local'
import {
  addQ,
  addTerm,
  decodeSearch,
  domainToFacets,
  EMPTY_SEARCH,
  encodeSearch,
  facetRefs,
  fromSavedQuery,
  hashWithParams,
  parseRange,
  rangeToPage,
  removeFacet,
  SearchState,
  toggleFilter,
  toggleGroupBy,
  toListQuery,
  toSavedQuery,
} from './model'

// `api.ts` lê o localStorage ao carregar; a bateria corre em node.
vi.stubGlobal('localStorage', { getItem: () => null, setItem: () => {}, removeItem: () => {} })
const { collectionPath, listQueryString } = await import('../../api')

const schema: SearchSchema = {
  resource: 'recordings',
  label: 'Gravações',
  collection: '/api/recordings',
  org_scoped: false,
  timezone: 'Africa/Luanda',
  text_search: { fields: ['title'], typo_tolerant: true },
  fields: [
    { name: 'title', label: 'Título', type: 'text', operators: ['eq', 'contains', 'is_set'], filterable: true, sortable: true, groupable: false, aggregates: [] },
    { name: 'uploader', label: 'Autor', type: 'user', operators: ['eq', 'in'], filterable: true, sortable: false, groupable: true, aggregates: [] },
    { name: 'category', label: 'Categoria', type: 'enum', operators: ['eq', 'in'], filterable: true, sortable: false, groupable: true, aggregates: [], options: [{ value: 'meeting', label: 'Reunião' }, { value: 'lecture', label: 'Aula' }] },
    { name: 'duration_secs', label: 'Duração', type: 'number', operators: ['gte', 'lt', 'between'], filterable: true, sortable: true, groupable: false, aggregates: ['sum', 'avg'] },
    { name: 'transcribed', label: 'Transcrita', type: 'bool', operators: ['eq'], filterable: true, sortable: false, groupable: true, aggregates: [] },
    { name: 'created_at', label: 'Criada', type: 'datetime', operators: ['gte', 'lt', 'in_period', 'between'], filterable: true, sortable: true, groupable: true, granularities: ['day', 'week', 'month', 'quarter', 'year'], aggregates: [] },
  ],
  filters: [
    { name: 'mine', label: 'As minhas', group: 'owner', filter: [['uploader', 'eq', 'ana']] },
    { name: 'shared', label: 'Partilhadas', group: 'owner', filter: [['uploader', 'ne', 'ana']] },
    { name: 'this_week', label: 'Esta semana', group: 'period', filter: [['created_at', 'in_period', 'this_week']] },
    { name: 'long', label: 'Longas', group: 'duration', filter: [['duration_secs', 'gte', 3600]] },
  ],
  group_by: [],
  default_order: ['-created_at'],
  periods: ['today', 'this_week'],
}

describe('estado → contrato das listas', () => {
  it('termos viram contains com OU; enum vira in; custom respeita E/OU; lista no topo = E', () => {
    let s: SearchState = addTerm(addTerm(EMPTY_SEARCH, 'title', 'orçamento'), 'title', 'plano')
    s = addTerm(s, 'category', 'lecture')
    s = { ...s, custom: [{ join: 'or', conds: [{ field: 'duration_secs', op: 'gte', value: 60 }, { field: 'title', op: 'is_set' }] }] }
    s = addQ(s, 'voz')
    const q = toListQuery(toggleFilter(s, 'mine'), schema)
    expect(q.q).toBe('voz')
    expect(q.filters).toEqual(['mine'])
    expect(q.filter).toEqual([
      { or: [['title', 'contains', 'orçamento'], ['title', 'contains', 'plano']] },
      ['category', 'eq', 'lecture'],
      { or: [['duration_secs', 'gte', 60], ['title', 'is_set']] },
    ])
  })

  it('a query string leva sempre page_size (envelope, nunca o array herdado) e o filter em JSON', () => {
    expect(listQueryString({})).toBe('page_size=50')
    const qs = new URLSearchParams(listQueryString({ q: ' a ', filter: [['title', 'contains', 'x']], group_by: ['created_at:month', 'uploader'], page_token: 'tok' }))
    expect(qs.get('q')).toBe('a')
    expect(JSON.parse(qs.get('filter')!)).toEqual([['title', 'contains', 'x']])
    expect(qs.get('group_by')).toBe('created_at:month,uploader')
    expect(qs.get('page_token')).toBe('tok')
    // um domínio vazio não vai
    expect(new URLSearchParams(listQueryString({ filter: [] })).has('filter')).toBe(false)
  })

  it('o caminho da colecção vem do schema, com o org_id no sítio e nada fora de /api', () => {
    expect(collectionPath({ collection: '/api/orgs/{org_id}/members', org_scoped: true }, 'a b')).toBe('/api/orgs/a%20b/members')
    expect(() => collectionPath({ collection: '/api/orgs/{org_id}/members', org_scoped: true }, null)).toThrow()
    expect(() => collectionPath({ collection: 'https://mal/x', org_scoped: false })).toThrow()
  })

  it('favorito: guardar e voltar a abrir devolve as mesmas facetas', () => {
    let s = addTerm(addTerm(EMPTY_SEARCH, 'title', 'a'), 'title', 'b')
    s = addTerm(s, 'category', 'meeting')
    s = { ...toggleFilter(s, 'long'), groupBy: ['uploader', 'created_at:month'], custom: [{ join: 'and', conds: [{ field: 'duration_secs', op: 'lt', value: 10 }, { field: 'transcribed', op: 'eq', value: true }] }] }
    const back = fromSavedQuery(toSavedQuery(s, schema), schema)
    expect(back.terms).toEqual(s.terms)
    expect(back.custom).toEqual(s.custom)
    expect(back.filters).toEqual(['long'])
    expect(back.groupBy).toEqual(['uploader', 'created_at:month'])
  })

  it('um domínio que não é termo nem custom simples não rebenta (e condições soltas viram custom)', () => {
    const f = domainToFacets([{ not: ['title', 'is_set'] }, ['duration_secs', 'gte', 5]], schema)
    expect(f.terms).toEqual([])
    expect(f.custom).toEqual([{ join: 'and', conds: [{ field: 'duration_secs', op: 'gte', value: 5 }] }])
  })
})

describe('facetas', () => {
  it('agrupar: a ordem dos cliques é a ordem dos níveis; outra granularidade do mesmo campo substitui; máximo 3', () => {
    let s = toggleGroupBy(EMPTY_SEARCH, 'uploader')
    s = toggleGroupBy(s, 'created_at:month')
    expect(s.groupBy).toEqual(['uploader', 'created_at:month'])
    s = toggleGroupBy(s, 'created_at:year')
    expect(s.groupBy).toEqual(['uploader', 'created_at:year'])
    s = toggleGroupBy(toggleGroupBy(s, 'category'), 'transcribed')
    expect(s.groupBy).toEqual(['uploader', 'created_at:year', 'category'])
    expect(toggleGroupBy(s, 'uploader').groupBy).toEqual(['created_at:year', 'category'])
  })

  it('filtros do mesmo grupo formam UMA faceta; remover a faceta tira o grupo inteiro', () => {
    const s = toggleFilter(toggleFilter(toggleFilter(EMPTY_SEARCH, 'mine'), 'shared'), 'this_week')
    const refs = facetRefs(s, schema)
    expect(refs).toEqual([{ kind: 'filters', group: 'owner' }, { kind: 'filters', group: 'period' }])
    expect(removeFacet(s, refs[0], schema).filters).toEqual(['this_week'])
  })
})

describe('URL', () => {
  it('ida e volta pelo hash, com prefixo por painel e sem apagar os outros parâmetros', () => {
    const s: SearchState = {
      q: ['voz & sip'],
      terms: [{ field: 'title', values: ['a,b', 'c'] }],
      filters: ['mine', 'this_week'],
      custom: [{ join: 'or', conds: [{ field: 'duration_secs', op: 'between', value: [1, 2] }] }],
      groupBy: ['uploader', 'created_at:month'],
      orderBy: ['-created_at'],
      pageSize: 80,
    }
    const p = encodeSearch(s, new URLSearchParams('id=42'), 'members.')
    expect(p.get('id')).toBe('42')
    expect(decodeSearch(new URLSearchParams(p.toString()), 'members.')).toEqual(s)
    expect(decodeSearch(new URLSearchParams(p.toString()))).toEqual(EMPTY_SEARCH)
    expect(hashWithParams('#/recordings?x=1', p)).toMatch(/^\/recordings\?id=42&/)
  })

  it('lixo na URL é ignorado, não rebenta', () => {
    const s = decodeSearch(new URLSearchParams('t=%7Bnope&c=[1,2]&n=9999&g=a,b,c,d'))
    expect(s.terms).toEqual([])
    expect(s.custom).toEqual([])
    expect(s.pageSize).toBe(50)
    expect(s.groupBy).toEqual(['a', 'b', 'c'])
  })
})

describe('paginador', () => {
  it('lê «1-80» e recusa lixo', () => {
    expect(parseRange('1-80')).toEqual({ start: 1, end: 80 })
    expect(parseRange(' 51 – 100 ')).toEqual({ start: 51, end: 100 })
    expect(parseRange('80-1')).toBeNull()
    expect(parseRange('abc')).toBeNull()
  })

  it('keyset: muda o tamanho a partir do início, ou vai a uma página já visitada; nunca inventa um OFFSET', () => {
    expect(rangeToPage({ start: 1, end: 80 }, 50, 1)).toEqual({ pageSize: 80, pageIndex: 0 })
    expect(rangeToPage({ start: 1, end: 500 }, 50, 1)).toEqual({ pageSize: 100, pageIndex: 0 })
    expect(rangeToPage({ start: 51, end: 100 }, 50, 3)).toEqual({ pageSize: 50, pageIndex: 1 })
    expect(rangeToPage({ start: 151, end: 200 }, 50, 3)).toBeNull()
    expect(rangeToPage({ start: 20, end: 99 }, 50, 3)).toBeNull()
  })
})

interface Row {
  title: string
  uploader: string
  category: string
  duration_secs: number | null
  transcribed: boolean
  created_at: string
}
const now = new Date(2026, 8, 17, 12, 0) // quinta, 17 Set 2026
const rows: Row[] = [
  { title: 'Orçamento 2027', uploader: 'ana', category: 'meeting', duration_secs: 4000, transcribed: true, created_at: new Date(2026, 8, 16, 10).toISOString() },
  { title: 'Aula SIP', uploader: 'rui', category: 'lecture', duration_secs: 1200, transcribed: false, created_at: new Date(2026, 8, 2, 10).toISOString() },
  { title: 'Aula RTP', uploader: 'ana', category: 'lecture', duration_secs: null, transcribed: false, created_at: new Date(2026, 7, 30, 10).toISOString() },
]
const src: LocalSource<Row> = { schema, get: (r, f) => r[f as keyof Row], text: (r) => r.title, now: () => now }

describe('lista local (colecção inteira no browser)', () => {
  it('q sem acentos, filtros do mesmo grupo com OU e de grupos diferentes com E', () => {
    expect(runLocal(rows, { q: 'orcamento' }, src).items.map((r) => r.title)).toEqual(['Orçamento 2027'])
    expect(runLocal(rows, { filters: ['mine', 'shared'] }, src).total).toBe(3)
    expect(runLocal(rows, { filters: ['mine', 'this_week'] }, src).items.map((r) => r.title)).toEqual(['Orçamento 2027'])
    expect(runLocal(rows, { filter: [['duration_secs', 'is_not_set']] }, src).total).toBe(1)
    expect(runLocal(rows, { filter: { or: [['category', 'eq', 'meeting'], ['duration_secs', 'between', [1000, 1300]]] } }, src).total).toBe(2)
  })

  it('ordena pela ordem por omissão, pagina por token e diz quando acaba', () => {
    const p1 = runLocal(rows, { page_size: 2 }, src)
    expect(p1.items.map((r) => r.title)).toEqual(['Orçamento 2027', 'Aula SIP'])
    expect(p1.next_page_token).toBe('2')
    const p2 = runLocal(rows, { page_size: 2, page_token: p1.next_page_token }, src)
    expect(p2.items.map((r) => r.title)).toEqual(['Aula RTP'])
    expect(p2.next_page_token).toBeNull()
  })

  it('grupos: só o 1.º nível, com contagem, agregados, filtro para abrir e o que falta agrupar', () => {
    const env = runLocal(rows, { group_by: ['uploader', 'created_at:month'] }, src)
    expect(env.groups!.map((g) => [g.key, g.count, g.aggregates.duration_secs.sum])).toEqual([
      ['ana', 2, 4000],
      ['rui', 1, 1200],
    ])
    expect(env.groups![0].group_by).toEqual(['created_at:month'])
    const sub = runLocal(rows, { filter: env.groups![0].filter, group_by: env.groups![0].group_by }, src)
    expect(sub.groups!.map((g) => [g.key, g.count])).toEqual([
      ['2026-08', 1],
      ['2026-09', 1],
    ])
    // abrir o grupo de mês devolve só as linhas desse mês
    expect(runLocal(rows, { filter: [env.groups![0].filter, sub.groups![1].filter] as Domain as never }, src).items.map((r) => r.title)).toEqual(['Orçamento 2027'])
    const enumG = runLocal(rows, { group_by: ['category'] }, src).groups!
    expect(enumG.map((g) => g.label)).toEqual(['Aula', 'Reunião'])
  })

  it('chaves de data e períodos na forma do contrato', () => {
    expect(dateKey(now, 'day')).toBe('2026-09-17')
    expect(dateKey(now, 'week')).toBe('2026-W38')
    expect(isoWeek(new Date(2021, 0, 3))).toBe('2020-W53')
    expect(dateKey(now, 'quarter')).toBe('2026-Q3')
    const w = periodRange('this_week', now)!
    expect(w[0]!.getDate()).toBe(14)
    expect(periodRange('nope', now)).toBeNull()
  })
})
