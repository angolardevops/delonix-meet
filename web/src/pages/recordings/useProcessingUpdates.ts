/**
 * Acompanha as gravações que o servidor ainda está a COMPOR.
 *
 * A linha nasce quando a gravação pára e passa a pronta segundos ou minutos
 * depois; sem isto ficava «A processar» no ecrã até a pessoa recarregar.
 *
 * Relê SÓ essas gravações, uma a uma (`GET /api/recordings/{id}/details`), e
 * guarda o que vier à parte — não volta a pedir a lista. É de propósito: a
 * lista é o estado do ecrã (selecção, painel aberto, vídeo a tocar), e uma
 * releitura de fundo que falhasse punha-a em erro e desmontava o leitor. Aqui
 * uma leitura que falha não muda nada: fica o que estava, e tenta-se outra vez.
 *
 * Cada leitura só começa depois de a anterior assentar (sem pedidos
 * empilhados num servidor ocupado a compor), e não se lê com o separador
 * escondido.
 */
import { useCallback, useEffect, useMemo, useState } from 'react'
import { ApiError, recordingDetails, type RecordingItem } from '../../api'
import { isProcessing, withFresh, type FreshRecordings } from './format'

/** Intervalo entre o fim de uma leitura e o início da seguinte. */
export const PROCESSING_POLL_MS = 4000

const sameReading = (a: RecordingItem | undefined, b: RecordingItem) =>
  !!a && a.status === b.status && a.size_bytes === b.size_bytes && a.failure_reason === b.failure_reason && JSON.stringify(a) === JSON.stringify(b)

/** Devolve a função que troca cada linha a compor pela sua leitura mais recente. */
export function useProcessingUpdates<T extends RecordingItem>(rows: readonly T[]): (row: T) => T {
  const [fresh, setFresh] = useState<FreshRecordings>({})
  const ids = useMemo(
    () =>
      rows
        .filter(isProcessing)
        .map((r) => r.id)
        .sort()
        .join(','),
    [rows],
  )
  useEffect(() => {
    if (!ids) return
    const pending = new Set(ids.split(','))
    const ctrl = new AbortController()
    let timer = 0
    const tick = async () => {
      if (document.visibilityState !== 'hidden') {
        for (const id of [...pending]) {
          try {
            const latest = await recordingDetails(id, ctrl.signal)
            if (ctrl.signal.aborted) return
            // Sem mudança não se escreve: cada escrita redesenha a página inteira.
            setFresh((prev) => (sameReading(prev[id], latest) ? prev : { ...prev, [id]: latest }))
            if (!isProcessing(latest)) pending.delete(id)
          } catch (e) {
            if (ctrl.signal.aborted) return
            // Deixou de existir (ou de ser visível): não há mais nada a ler.
            if (e instanceof ApiError && e.status === 404) pending.delete(id)
            // Qualquer outra falha: fica o que estava e tenta-se na próxima volta.
          }
        }
      }
      if (pending.size > 0) timer = window.setTimeout(tick, PROCESSING_POLL_MS)
    }
    timer = window.setTimeout(tick, PROCESSING_POLL_MS)
    return () => {
      ctrl.abort()
      window.clearTimeout(timer)
    }
  }, [ids])
  return useCallback((row: T) => withFresh(row, fresh), [fresh])
}
