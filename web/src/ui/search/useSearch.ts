/**
 * Ligações do painel ao mundo: a URL (estado partilhável e botão voltar), o
 * schema do servidor, os favoritos e a lista paginada por cursor.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  apiErrorCode,
  apiErrorMessage,
  collectionPath,
  createSavedSearch,
  deleteSavedSearch,
  isAbort,
  listSavedSearches,
  ListEnvelope,
  ListQuery,
  SavedSearch,
  searchList,
  searchSchema,
  SearchSchema,
} from '../../api'
import { Async } from '../../components/AsyncSection'
import { runLocal, LocalSource } from './local'
import {
  decodeSearch,
  EMPTY_SEARCH,
  encodeSearch,
  fromSavedQuery,
  hashParams,
  hashWithParams,
  isEmptySearch,
  parseRange,
  rangeToPage,
  SearchState,
  toListQuery,
  toSavedQuery,
} from './model'

const same = (a: SearchState, b: SearchState) => JSON.stringify(a) === JSON.stringify(b)

/**
 * Estado do painel na query do hash. Mudar uma faceta é uma entrada nova no
 * histórico (o «voltar» desfaz a última); `replace` serve para o que não é
 * escolha da pessoa (aplicar o favorito por omissão ao abrir).
 */
export function useSearchUrl(ns = ''): [SearchState, (s: SearchState, opts?: { replace?: boolean }) => void] {
  const [state, setState] = useState<SearchState>(() => decodeSearch(hashParams(), ns))
  useEffect(() => {
    const on = () => {
      const next = decodeSearch(hashParams(), ns)
      setState((prev) => (same(prev, next) ? prev : next))
    }
    window.addEventListener('hashchange', on)
    return () => window.removeEventListener('hashchange', on)
  }, [ns])
  const set = useCallback(
    (s: SearchState, opts?: { replace?: boolean }) => {
      setState(s)
      const h = hashWithParams(location.hash, encodeSearch(s, hashParams(), ns))
      if (`#${h}` === location.hash) return
      if (opts?.replace) history.replaceState(history.state, '', `#${h}`)
      else location.hash = h
    },
    [ns],
  )
  return [state, set]
}

/** Schema de um recurso do servidor, uma vez por ecrã. */
export function useSearchSchema(resource: string | null): Async<SearchSchema> {
  const { t } = useTranslation()
  const [st, setSt] = useState<Async<SearchSchema>>({ s: 'loading' })
  useEffect(() => {
    if (!resource) return
    const ctrl = new AbortController()
    setSt({ s: 'loading' })
    searchSchema(resource, ctrl.signal)
      .then((d) => setSt({ s: 'ready', d }))
      .catch((e) => {
        if (!isAbort(e)) setSt({ s: 'error', msg: apiErrorMessage(e, t('search.estado.erroSchema')) })
      })
    return () => ctrl.abort()
  }, [resource, t])
  return st
}

export type ListFetcher<T> = (query: ListQuery, signal: AbortSignal) => Promise<ListEnvelope<T>>

/** Lista do servidor: a colecção do schema com os parâmetros uniformes. */
export function serverFetcher<T>(schema: SearchSchema, orgId?: string | null): ListFetcher<T> {
  const path = collectionPath(schema, orgId)
  return (query, signal) => searchList<T>(path, query, signal)
}

/** Lista local: a colecção inteira que o ecrã já tem. */
export function localFetcher<T>(rows: T[], src: LocalSource<T>): ListFetcher<T> {
  return (query) => Promise.resolve(runLocal(rows, query, src))
}

export interface ListSearch<T> {
  query: ListQuery
  state: Async<ListEnvelope<T>>
  pageIndex: number
  /** Início (1-based) e fim do intervalo mostrado. */
  range: { start: number; end: number } | null
  hasPrev: boolean
  hasNext: boolean
  prev: () => void
  next: () => void
  /** «1-80»: muda o tamanho a partir do início, ou volta a uma página já vista. Devolve se aceitou. */
  applyRange: (text: string) => boolean
  reload: () => void
}

/**
 * A página corrente da lista, com os cursores já vistos guardados (o servidor
 * é keyset, sem OFFSET: «anterior» é voltar a um cursor que se guardou).
 */
export function useListSearch<T>(
  fetcher: ListFetcher<T> | null,
  schema: SearchSchema | null,
  search: SearchState,
  setSearch: (s: SearchState) => void,
): ListSearch<T> {
  const { t } = useTranslation()
  const baseQuery = useMemo(() => (schema ? toListQuery(search, schema) : null), [schema, search])
  const key = JSON.stringify(baseQuery)
  const [tokens, setTokens] = useState<(string | null)[]>([null])
  const [pageIndex, setPageIndex] = useState(0)
  const [state, setState] = useState<Async<ListEnvelope<T>>>({ s: 'loading' })
  const [nonce, setNonce] = useState(0)

  // Pesquisa nova → primeira página; os cursores da anterior deixam de servir
  // (o servidor recusa-os com `search.page_token_mismatch`).
  const lastKey = useRef(key)
  if (lastKey.current !== key) {
    lastKey.current = key
    if (pageIndex !== 0) setPageIndex(0)
    if (tokens.length !== 1) setTokens([null])
  }

  const token = tokens[pageIndex] ?? null
  useEffect(() => {
    if (!fetcher || !baseQuery) return
    const ctrl = new AbortController()
    setState((prev) => (prev.s === 'ready' ? prev : { s: 'loading' }))
    fetcher({ ...baseQuery, page_token: token }, ctrl.signal)
      .then((d) => {
        if (ctrl.signal.aborted) return
        setState({ s: 'ready', d })
        setTokens((ts) => {
          const copy = ts.slice(0, pageIndex + 1)
          copy[pageIndex + 1] = d.next_page_token
          return d.next_page_token ? copy : copy.slice(0, pageIndex + 1)
        })
      })
      .catch((e) => {
        if (isAbort(e)) return
        // Um cursor de outra pesquisa (link antigo): volta ao início em vez de ficar em erro.
        if (apiErrorCode(e) === 'search.page_token_mismatch' || apiErrorCode(e) === 'page.invalid_token') {
          setPageIndex(0)
          setTokens([null])
          return
        }
        setState({ s: 'error', msg: apiErrorMessage(e, t('search.estado.erro')) })
      })
    return () => ctrl.abort()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [fetcher, key, token, nonce])

  const size = search.pageSize
  const d = state.s === 'ready' ? state.d : null
  const range = d && d.items.length ? { start: pageIndex * size + 1, end: pageIndex * size + d.items.length } : null
  return {
    query: { ...(baseQuery ?? {}), page_token: token },
    state,
    pageIndex,
    range,
    hasPrev: pageIndex > 0,
    hasNext: !!d?.next_page_token,
    prev: () => setPageIndex((i) => Math.max(0, i - 1)),
    next: () => {
      if (d?.next_page_token) setPageIndex((i) => i + 1)
    },
    applyRange: (text) => {
      const r = parseRange(text)
      if (!r) return false
      const target = rangeToPage(r, size, tokens.length)
      if (!target) return false
      if (target.pageSize !== size) setSearch({ ...search, pageSize: target.pageSize })
      else setPageIndex(target.pageIndex)
      return true
    },
    reload: () => setNonce((n) => n + 1),
  }
}

export interface Favorites {
  supported: boolean
  items: Async<SavedSearch[]>
  apply: (f: SavedSearch) => void
  save: (input: { name: string; shared: boolean; is_default: boolean }) => Promise<string | null>
  remove: (f: SavedSearch) => Promise<string | null>
}

/**
 * Favoritos do servidor para um recurso com schema no servidor. O favorito
 * «por omissão» aplica-se ao abrir o ecrã — só se a URL não trouxer já uma
 * pesquisa (um link partilhado ganha ao favorito).
 */
export function useFavorites(
  resource: string | null,
  schema: SearchSchema | null,
  search: SearchState,
  setSearch: (s: SearchState, opts?: { replace?: boolean }) => void,
): Favorites {
  const { t } = useTranslation()
  const [items, setItems] = useState<Async<SavedSearch[]>>({ s: 'loading' })
  const [nonce, setNonce] = useState(0)
  const appliedDefault = useRef(false)
  const startedEmpty = useRef(isEmptySearch(search))

  useEffect(() => {
    if (!resource) return
    const ctrl = new AbortController()
    listSavedSearches(resource, ctrl.signal)
      .then((p) => setItems({ s: 'ready', d: p.items }))
      .catch((e) => {
        if (!isAbort(e)) setItems({ s: 'error', msg: apiErrorMessage(e, t('search.favoritos.erroCarregar')) })
      })
    return () => ctrl.abort()
  }, [resource, nonce, t])

  useEffect(() => {
    if (appliedDefault.current || !schema || items.s !== 'ready') return
    appliedDefault.current = true
    const def = items.d.find((f) => f.is_default && f.valid && f.editable)
    if (def && startedEmpty.current) setSearch({ ...fromSavedQuery(def.query, schema, search.pageSize) }, { replace: true })
  }, [items, schema, search.pageSize, setSearch])

  const errorText = (e: unknown) => {
    switch (apiErrorCode(e)) {
      case 'saved_search.duplicate_name':
        return t('search.favoritos.erroNome')
      case 'saved_search.limit_reached':
        return t('search.favoritos.erroLimite')
      case 'saved_search.no_organization':
        return t('search.favoritos.erroOrg')
      case 'saved_search.not_owner':
        return t('search.favoritos.erroDono')
      default:
        return apiErrorMessage(e, t('search.favoritos.erroGuardar'))
    }
  }

  return {
    supported: !!resource,
    items,
    apply: (f) => schema && setSearch(fromSavedQuery(f.query, schema, search.pageSize)),
    save: async ({ name, shared, is_default }) => {
      if (!resource || !schema) return t('search.favoritos.erroGuardar')
      try {
        await createSavedSearch({ resource, name, query: toSavedQuery(search, schema), shared, is_default })
        setNonce((n) => n + 1)
        return null
      } catch (e) {
        return errorText(e)
      }
    },
    remove: async (f) => {
      try {
        await deleteSavedSearch(f.id)
        setNonce((n) => n + 1)
        return null
      } catch (e) {
        return errorText(e)
      }
    },
  }
}

export const NO_FAVORITES: Favorites = {
  supported: false,
  items: { s: 'ready', d: [] },
  apply: () => {},
  save: async () => null,
  remove: async () => null,
}

export { EMPTY_SEARCH }
