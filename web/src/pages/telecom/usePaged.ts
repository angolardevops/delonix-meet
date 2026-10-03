/**
 * Lista paginada por cursor: a primeira página pelo useAsync (três estados,
 * abortável) e as seguintes acrescentadas a pedido. Um erro ao carregar mais
 * não deita fora o que já está no ecrã — aparece ao lado do botão.
 */
import { DependencyList, useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { isAbort, Page } from '../../api'
import { useAsync } from '../../components/AsyncSection'
import { orgErrorMessage, refusalAware } from '../admin/orgShared'
import { nextToken } from './format'

export interface PagedList<T> {
  items: T[]
  /** Cursor da página seguinte; null quando não há mais. */
  next: string | null
}

export function usePaged<T>(load: (token: string | undefined, signal: AbortSignal) => Promise<Page<T>>, deps: DependencyList) {
  const { t } = useTranslation()
  const first = useAsync<PagedList<T>>(
    (signal) => refusalAware(load(undefined, signal), t).then((p) => ({ items: p.items, next: nextToken(p) })),
    deps,
  )
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')
  const ctrl = useRef<AbortController | null>(null)
  useEffect(() => () => ctrl.current?.abort(), [])

  const state = first.state
  const mutate = first.mutate
  const loadMore = useCallback(() => {
    if (state.s !== 'ready' || !state.d.next || busy) return
    ctrl.current?.abort()
    const c = new AbortController()
    ctrl.current = c
    setBusy(true)
    setErr('')
    load(state.d.next, c.signal)
      .then((p) => {
        if (c.signal.aborted) return
        mutate((d) => ({ items: [...d.items, ...p.items], next: nextToken(p) }))
        setBusy(false)
      })
      .catch((e) => {
        if (isAbort(e)) return
        setBusy(false)
        setErr(orgErrorMessage(e, t, 'ui.erroCarregar'))
      })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state, busy, mutate, t])

  /** Recomeça da primeira página; um «carregar mais» em curso deixa de contar. */
  const reloadFirst = first.reload
  const reload = useCallback(() => {
    ctrl.current?.abort()
    setBusy(false)
    setErr('')
    reloadFirst()
  }, [reloadFirst])

  return { state, reload, loadMore, busy, err }
}
