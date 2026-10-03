/**
 * Peças comuns aos cartões da Telefonia: dizer uma razão ou um valor
 * enumerado na língua de quem lê (ou tal qual, se a consola não o conhece) e
 * o rodapé de «carregar mais».
 */
import { useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { Alert, Button } from '../../ui/kit'
import { enumKey, reasonKey } from './format'

/** Travessão para «não se aplica» — nunca no lugar de um valor por medir. */
export const NA = '—'

export function useTelecomText() {
  const { t } = useTranslation()
  const reason = useCallback(
    (code: string) => {
      const k = reasonKey(code)
      return k ? t(k) : code
    },
    [t],
  )
  const label = useCallback(
    (grupo: string, valor: string) => {
      const k = enumKey(grupo, valor)
      return k ? t(k) : valor
    },
    [t],
  )
  return { reason, label }
}

export function LoadMore({ next, busy, err, onMore }: { next: string | null; busy: boolean; err: string; onMore: () => void }) {
  const { t } = useTranslation()
  if (!next && !err) return null
  return (
    <div className="tel-more">
      {err && <Alert tone="danger">{err}</Alert>}
      {next && (
        <Button size="sm" variant="secondary" busy={busy} onClick={onMore}>
          {t('telecom.carregarMais')}
        </Button>
      )}
    </div>
  )
}
