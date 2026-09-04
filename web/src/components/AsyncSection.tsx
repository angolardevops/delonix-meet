import { ReactNode, useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage, isAbort } from '../api'
import EmptyState from './EmptyState'
import { Btn } from './ui'

/**
 * Os três estados de uma lista que vem da rede (achados 4.1 e 4.2 do
 * docs/ux-perf-review.md), no padrão de estado do `delonix-portal`.
 *
 * PORQUÊ: a app não tinha NENHUM estado de carregamento — zero `skeleton` no
 * codebase — e os `catch` vazios faziam com que uma API em baixo desse
 * exatamente o mesmo ecrã que uma lista genuinamente vazia. O utilizador via um
 * dashboard completo e vazio e concluía que não tinha nada.
 *
 * Um cartão vazio tem de significar «não há». «Ainda não sei» é o esqueleto e
 * «não consegui saber» é o erro, com um botão para tentar outra vez.
 *
 * O QUE VEM DO PORTAL (src/api/client.ts + src/pages/Dashboard.tsx):
 *  · um `AbortController` por efeito, abortado na limpeza;
 *  · `isAbort(e)` em TODOS os `.catch()` — abortar não é falhar, e sem esta
 *    guarda o duplo-efeito do StrictMode pinta um erro em cada montagem;
 *  · estado de servidor separado do estado de UI: este hook só conhece o
 *    primeiro, e nada disto vive num store global.
 */
export type Load<T> =
  | { s: 'loading' }
  | { s: 'ready'; d: T }
  | { s: 'error'; msg: string }

/**
 * Corre `fetcher(signal)` e devolve o estado mais um `retry`. Nunca lança.
 * O `fetcher` tem de ser estável (`useCallback`) — é a dependência do efeito.
 */
export function useLoad<T>(
  fetcher: (signal: AbortSignal) => Promise<T>,
  fallbackMsg = '',
): [Load<T>, () => void] {
  const [state, setState] = useState<Load<T>>({ s: 'loading' })
  const [nonce, setNonce] = useState(0)

  useEffect(() => {
    const ctrl = new AbortController()
    setState({ s: 'loading' })
    fetcher(ctrl.signal)
      .then((d) => {
        if (!ctrl.signal.aborted) setState({ s: 'ready', d })
      })
      .catch((e: unknown) => {
        // Abortar é a limpeza do efeito a fazer o seu trabalho, não uma falha.
        if (isAbort(e)) return
        setState({ s: 'error', msg: apiErrorMessage(e, fallbackMsg) })
      })
    return () => ctrl.abort()
  }, [fetcher, nonce, fallbackMsg])

  const retry = useCallback(() => setNonce((n) => n + 1), [])
  return [state, retry]
}

/**
 * Renderiza o estado certo. `rows` é o número de linhas do esqueleto — deve
 * bater com o número típico da secção, para o conteúdo não saltar quando chega.
 */
export default function AsyncSection<T>({
  load,
  retry,
  rows = 3,
  isEmpty,
  empty,
  children,
}: {
  load: Load<T>
  retry: () => void
  rows?: number
  isEmpty?: (d: T) => boolean
  empty: ReactNode
  children: (d: T) => ReactNode
}) {
  const { t } = useTranslation()

  if (load.s === 'loading') {
    return (
      <div className="skel-list" role="status" aria-live="polite" aria-label={t('common.loading')}>
        {Array.from({ length: rows }, (_, i) => (
          <div className="skel-row" key={i}>
            <span className="skel-line skel-title" />
            <span className="skel-line skel-meta" />
          </div>
        ))}
      </div>
    )
  }

  if (load.s === 'error') {
    return (
      <EmptyState
        title={t('load.failedTitle')}
        hint={load.msg || t('load.failedHint')}
        action={<Btn variant="ghost" onClick={retry}>{t('load.retry')}</Btn>}
      />
    )
  }

  if (isEmpty ? isEmpty(load.d) : Array.isArray(load.d) && load.d.length === 0) {
    return <>{empty}</>
  }
  return <>{children(load.d)}</>
}
