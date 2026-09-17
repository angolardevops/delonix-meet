/**
 * Pesquisa da Agenda. Com o servidor: o recurso `meetings` do contrato. Sem
 * ele: `GET /api/meetings` inteiro, com os campos que a lista traz.
 *
 * Nas vistas de calendário (dia, semana, mês, ano) a pesquisa FILTRA o que a
 * grelha mostra: pedem-se todas as reuniões do intervalo visível que cumprem
 * a pesquisa (várias páginas, se for preciso), sem agrupamento nem página.
 */
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage, DomainNode, isAbort, listMeetings, Meeting } from '../../api'
import type { Async } from '../../components/AsyncSection'
import { localSchema } from '../../ui/search/localSchema'
import { SearchState, toListQuery } from '../../ui/search/model'
import type { LocalFallback, ResourceSearch } from '../../ui/search/useResourceSearch'

export const meetingsFallback: LocalFallback<Meeting> = {
  load: (signal) => listMeetings(signal),
  source: {
    schema: localSchema(
      'meetings',
      [
        { name: 'title', type: 'text' },
        { name: 'description', type: 'text' },
        { name: 'owner', type: 'user' },
        { name: 'kind', type: 'enum', options: ['video', 'voice'] },
        { name: 'starts_at', type: 'datetime' },
        { name: 'duration_min', type: 'number', aggregates: ['sum'] },
        { name: 'my_status', type: 'enum', options: ['owner', 'pending', 'accepted', 'declined'] },
        { name: 'recurring', type: 'bool' },
        { name: 'meeting_room', type: 'ref' },
      ],
      [
        { name: 'mine', group: 'owner', filter: [['my_status', 'eq', 'owner']] },
        { name: 'invited', group: 'owner', filter: [['my_status', 'ne', 'owner']] },
        { name: 'pending_response', group: 'response', filter: [['my_status', 'eq', 'pending']] },
        { name: 'accepted', group: 'response', filter: [['my_status', 'eq', 'accepted']] },
        { name: 'declined', group: 'response', filter: [['my_status', 'eq', 'declined']] },
        { name: 'video', group: 'kind', filter: [['kind', 'eq', 'video']] },
        { name: 'voice', group: 'kind', filter: [['kind', 'eq', 'voice']] },
        { name: 'upcoming', group: 'period', filter: [['starts_at', 'in_period', 'future']] },
        { name: 'past', group: 'period', filter: [['starts_at', 'in_period', 'past']] },
        { name: 'today', group: 'period', filter: [['starts_at', 'in_period', 'today']] },
        { name: 'this_week', group: 'period', filter: [['starts_at', 'in_period', 'this_week']] },
        { name: 'next_7_days', group: 'period', filter: [['starts_at', 'in_period', 'next_7_days']] },
        { name: 'recurring', group: 'content', filter: [['recurring', 'eq', true]] },
      ],
      { textFields: ['title', 'description', 'owner'], defaultOrder: ['starts_at'] },
    ),
    get: (m, f) => {
      switch (f) {
        case 'owner':
          return m.owner_name
        case 'my_status':
          return m.is_owner ? 'owner' : (m.my_status ?? 'pending')
        case 'recurring':
          return !!(m.recurrence_freq || m.recurrence_parent_id)
        case 'meeting_room':
          return m.room_name ?? null
        default:
          return (m as unknown as Record<string, unknown>)[f]
      }
    },
    text: (m) => `${m.title} ${m.description ?? ''} ${m.owner_name} ${m.room_code ?? ''}`,
  },
}

/** A pesquisa FILTRA alguma coisa? (agrupar, ordenar e o tamanho da página não.) */
export function filtersSomething(s: SearchState): boolean {
  return s.q.length > 0 || s.terms.length > 0 || s.filters.length > 0 || s.custom.length > 0
}

const MAX_PAGES = 20

/**
 * Todas as reuniões de [from, to[ que cumprem a pesquisa. `null` quando a
 * pesquisa não filtra nada — a grelha usa a lista de sempre.
 */
export function useMeetingsMatching(rs: ResourceSearch<Meeting>, from: Date, to: Date): Async<Meeting[]> | null {
  const { t } = useTranslation()
  const active = filtersSomething(rs.search)
  const [state, setState] = useState<Async<Meeting[]>>({ s: 'loading' })
  const base = rs.schema ? toListQuery({ ...rs.search, groupBy: [], orderBy: [], pageSize: 100 }, rs.schema) : null
  const key = JSON.stringify([base, from.getTime(), to.getTime()])
  const fetcher = rs.fetcher

  useEffect(() => {
    if (!active || !fetcher || !base) return
    const ctrl = new AbortController()
    const range: DomainNode = ['starts_at', 'between', [from.toISOString(), new Date(to.getTime() - 1).toISOString()]]
    const filter: DomainNode[] = [...((base.filter as DomainNode[] | undefined) ?? []), range]
    setState((prev) => (prev.s === 'ready' ? prev : { s: 'loading' }))
    ;(async () => {
      const out: Meeting[] = []
      let token: string | null = null
      for (let i = 0; i < MAX_PAGES; i++) {
        const d = await fetcher({ ...base, filter, group_by: undefined, page_token: token }, ctrl.signal)
        out.push(...d.items)
        token = d.next_page_token
        if (!token) break
      }
      return out
    })()
      .then((d) => !ctrl.signal.aborted && setState({ s: 'ready', d }))
      .catch((e) => {
        if (!isAbort(e)) setState({ s: 'error', msg: apiErrorMessage(e, t('search.estado.erro')) })
      })
    return () => ctrl.abort()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, fetcher, key])

  return active ? state : null
}
