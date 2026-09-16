/**
 * Peças do cartão de SMS: a leitura que se repete sem piscar, a chave de
 * idempotência de um envio e o botão de copiar.
 */
import { DependencyList, useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { apiErrorMessage, isAbort } from '../../api'
import type { Async } from '../../components/AsyncSection'
import { IconButton } from '../../ui/kit'
import { isRefused } from './orgShared'

export const SMS_POLL_MS = 5000

/**
 * Carrega e repete a cada `intervalMs` enquanto o separador estiver visível
 * (`null` = não repete); ao voltar a ficar visível, pede logo.
 *
 * Diferente do `useAsync`: uma repetição não volta a `loading` (o cartão não
 * pisca a cada 5 s), um erro numa repetição não apaga o que já se tinha, e só
 * há `setState` quando a resposta mudou (R21).
 */
export function usePolled<T>(load: (signal: AbortSignal) => Promise<T>, deps: DependencyList, intervalMs: number | null) {
  const { t } = useTranslation()
  const [state, setState] = useState<Async<T>>({ s: 'loading' })
  const last = useRef('')
  const ctrlRef = useRef<AbortController | null>(null)
  const loadRef = useRef(load)
  loadRef.current = load

  const run = useCallback(() => {
    ctrlRef.current?.abort()
    const ctrl = new AbortController()
    ctrlRef.current = ctrl
    loadRef
      .current(ctrl.signal)
      .then((d) => {
        if (ctrl.signal.aborted) return
        const key = JSON.stringify(d)
        if (key === last.current) return
        last.current = key
        setState({ s: 'ready', d })
      })
      .catch((e) => {
        if (isAbort(e)) return
        // Já há dados: uma repetição falhada não os troca por um erro.
        if (last.current) return
        setState({ s: 'error', msg: isRefused(e) ? t('org.erro.recusado') : apiErrorMessage(e, t('ui.erroCarregar')) })
      })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps)

  useEffect(() => {
    run()
    return () => ctrlRef.current?.abort()
  }, [run])

  useEffect(() => {
    if (intervalMs === null) return
    const id = window.setInterval(() => {
      if (!document.hidden) run()
    }, intervalMs)
    const onVisible = () => {
      if (!document.hidden) run()
    }
    document.addEventListener('visibilitychange', onVisible)
    return () => {
      window.clearInterval(id)
      document.removeEventListener('visibilitychange', onVisible)
    }
  }, [intervalMs, run])

  /** Botão «tentar de novo» do estado de erro: volta ao esqueleto. */
  const reload = useCallback(() => {
    last.current = ''
    setState({ s: 'loading' })
    run()
  }, [run])

  /** Resposta que já se tem na mão (por exemplo, a do PUT). */
  const put = useCallback((d: T) => {
    last.current = JSON.stringify(d)
    setState({ s: 'ready', d })
  }, [])

  return { state, reload, refresh: run, put }
}

/** Uma chave por SUBMISSÃO: repetir a mesma submissão não cobra dois SMS. */
export function newIdempotencyKey(): string {
  if (typeof crypto.randomUUID === 'function') return crypto.randomUUID()
  // `randomUUID` só existe em contexto seguro; numa origem HTTP de LAN falta.
  const b = crypto.getRandomValues(new Uint8Array(16))
  return Array.from(b, (x) => x.toString(16).padStart(2, '0')).join('')
}

export function CopyButton({ text, label }: { text: string; label: string }) {
  const { t } = useTranslation()
  const [done, setDone] = useState(false)
  useEffect(() => {
    if (!done) return
    const id = window.setTimeout(() => setDone(false), 1500)
    return () => window.clearTimeout(id)
  }, [done])
  return (
    <IconButton
      icon={done ? 'check' : 'copy'}
      label={done ? t('ui.copiado') : label}
      onClick={() => {
        void navigator.clipboard
          ?.writeText(text)
          .then(() => setDone(true))
          .catch(() => {})
      }}
    />
  )
}
