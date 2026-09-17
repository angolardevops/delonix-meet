/**
 * Uma lista de um ecrã com o painel de pesquisa — escolhe a fonte:
 *
 *  - **servidor**: o recurso tem schema em `/api/search/schemas/{resource}` →
 *    filtros, agrupamentos, página e favoritos vão ao servidor (contrato §2–§5);
 *  - **local**: o servidor responde 404 ao schema (ainda não tem pesquisa, ou é
 *    um recurso da fase 2) → o painel trabalha sobre a colecção INTEIRA que o
 *    endpoint de sempre devolve, com um schema local só com os campos que esse
 *    endpoint traz. Sem favoritos (vivem no servidor) e com o aviso no ecrã.
 *
 * Nunca se cai para local quando o servidor devolve outro erro: um 500 é um
 * erro para mostrar, não um motivo para trocar de fonte em silêncio.
 */
import { useCallback, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ApiError, isAbort, searchSchema, SearchSchema } from '../../api'
import { useAsync } from '../../components/AsyncSection'
import type { LocalSource } from './local'
import { EMPTY_SEARCH, SearchState } from './model'
import {
  Favorites,
  ListFetcher,
  ListSearch,
  localFetcher,
  NO_FAVORITES,
  serverFetcher,
  useFavorites,
  useListSearch,
  useSearchUrl,
} from './useSearch'

export interface LocalFallback<T> {
  /** O endpoint de sempre (a colecção inteira, sem corte). */
  load: (signal: AbortSignal) => Promise<T[]>
  /** Schema local (mesma forma) e como ler cada campo de uma linha. */
  source: Omit<LocalSource<T>, 'schema'> & { schema: SearchSchema }
}

export interface ResourceSearch<T> {
  /** `unsupported`: o servidor não tem o recurso e não há alternativa local honesta. */
  mode: 'loading' | 'server' | 'local' | 'error' | 'unsupported'
  error: string | null
  schema: SearchSchema | null
  search: SearchState
  setSearch: (s: SearchState, opts?: { replace?: boolean }) => void
  fetcher: ListFetcher<T> | null
  list: ListSearch<T>
  favorites: Favorites
  /** Recarrega a lista (e a colecção local, em modo local). */
  reload: () => void
}

type Source<T> = { mode: 'server'; schema: SearchSchema } | { mode: 'local'; schema: SearchSchema; rows: T[] } | { mode: 'unsupported'; schema: null }

export function useResourceSearch<T>({
  resource,
  orgId,
  ns = '',
  fallback,
  deps = [],
}: {
  /** `null` quando o recurso não existe no servidor (fase 2): vai direito ao local. */
  resource: string | null
  orgId?: string | null
  /** Prefixo do estado na URL; `null` = estado só em memória (um diálogo). */
  ns?: string | null
  fallback: LocalFallback<T> | null
  deps?: unknown[]
}): ResourceSearch<T> {
  const { t } = useTranslation()
  const [urlSearch, setUrlSearch] = useSearchUrl(ns ?? '')
  const [memSearch, setMemSearch] = useState<SearchState>(EMPTY_SEARCH)
  const setMem = useCallback((st: SearchState) => setMemSearch(st), [])
  const search = ns === null ? memSearch : urlSearch
  const setSearch = ns === null ? setMem : setUrlSearch

  const src = useAsync<Source<T>>(
    async (signal) => {
      if (resource) {
        try {
          const schema = await searchSchema(resource, signal)
          return { mode: 'server', schema }
        } catch (e) {
          if (isAbort(e) || !(e instanceof ApiError) || e.status !== 404) throw e
          if (!fallback) return { mode: 'unsupported', schema: null }
        }
      }
      if (!fallback) throw new Error(t('search.estado.erroSchema'))
      const rows = await fallback.load(signal)
      return { mode: 'local', schema: fallback.source.schema, rows }
    },
    [resource, orgId, ...deps],
  )

  const s = src.state
  const schema = s.s === 'ready' ? s.d.schema : null
  const fetcher = useMemo<ListFetcher<T> | null>(() => {
    if (s.s !== 'ready' || s.d.mode === 'unsupported') return null
    if (s.d.mode === 'server') return serverFetcher<T>(s.d.schema, orgId)
    return localFetcher(s.d.rows, { ...fallback!.source, schema: s.d.schema })
    // A fonte local é estável por ecrã; só a colecção muda.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [s, orgId])

  const list = useListSearch<T>(fetcher, schema, search, setSearch)
  const serverFavorites = useFavorites(s.s === 'ready' && s.d.mode === 'server' ? resource : null, schema, search, setSearch)

  return {
    mode: s.s === 'loading' ? 'loading' : s.s === 'error' ? 'error' : s.d.mode,
    error: s.s === 'error' ? s.msg : null,
    schema,
    search,
    setSearch,
    fetcher,
    list,
    favorites: s.s === 'ready' && s.d.mode === 'server' ? serverFavorites : NO_FAVORITES,
    reload: () => {
      if (s.s === 'ready' && s.d.mode === 'server') list.reload()
      else src.reload()
    },
  }
}
