/**
 * Dados do servidor em TRÊS estados — a carregar, pronto, erro — e nunca um
 * booleano. O pedido é abortável: sair do ecrã cancela-o, e um aborto não é
 * um erro para mostrar (R49).
 */
import { DependencyList, ReactNode, useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage, isAbort } from '../api'
import { Alert, Button, Skeleton } from '../ui/kit'

export type Async<T> = { s: 'loading' } | { s: 'ready'; d: T } | { s: 'error'; msg: string }

export function useAsync<T>(load: (signal: AbortSignal) => Promise<T>, deps: DependencyList) {
  const { t } = useTranslation()
  const [state, setState] = useState<Async<T>>({ s: 'loading' })
  const [nonce, setNonce] = useState(0)
  useEffect(() => {
    const ctrl = new AbortController()
    setState((prev) => (prev.s === 'ready' ? prev : { s: 'loading' }))
    load(ctrl.signal)
      .then((d) => {
        if (!ctrl.signal.aborted) setState({ s: 'ready', d })
      })
      .catch((e) => {
        if (isAbort(e)) return
        setState({ s: 'error', msg: apiErrorMessage(e, t('ui.erroCarregar')) })
      })
    return () => ctrl.abort()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [...deps, nonce])
  const reload = useCallback(() => setNonce((n) => n + 1), [])
  /** Actualização optimista local — o próximo reload repõe a verdade do servidor. */
  const mutate = useCallback((fn: (d: T) => T) => setState((s) => (s.s === 'ready' ? { s: 'ready', d: fn(s.d) } : s)), [])
  return { state, reload, mutate }
}

export function AsyncSection<T>({
  state,
  onRetry,
  children,
  skeleton,
}: {
  state: Async<T>
  onRetry?: () => void
  children: (d: T) => ReactNode
  skeleton?: ReactNode
}) {
  const { t } = useTranslation()
  if (state.s === 'loading') {
    return (
      <>
        {skeleton ?? (
          <div style={{ display: 'grid', gap: 8, padding: 14 }} aria-busy="true">
            <Skeleton h={14} w="60%" />
            <Skeleton h={14} />
            <Skeleton h={14} w="80%" />
          </div>
        )}
      </>
    )
  }
  if (state.s === 'error') {
    return (
      <div style={{ padding: 14 }}>
        <Alert tone="danger">
          <div>{state.msg}</div>
          {onRetry && (
            <Button size="sm" variant="secondary" icon="refresh" onClick={onRetry} style={{ marginTop: 8 }}>
              {t('ui.tentarDeNovo')}
            </Button>
          )}
        </Alert>
      </div>
    )
  }
  return <>{children(state.d)}</>
}
