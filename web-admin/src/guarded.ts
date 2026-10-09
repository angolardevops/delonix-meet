/**
 * Resultado de uma leitura que o servidor pode recusar por autorização.
 *
 * Versão mínima do `guarded()` de `web/src/pages/integrations/common.tsx`
 * (que vem com bagagem de Integrações de tenant que esta app não usa): o
 * 403 não é uma avaria, é o servidor a dizer quem pode — converte-se num
 * estado próprio em vez de cair no `useAsync` como erro.
 */
import { ApiError } from './api'

export type Guarded<T> = { forbidden: true } | { forbidden: false; d: T }

export function guarded<T>(p: Promise<T>): Promise<Guarded<T>> {
  return p.then(
    (d) => ({ forbidden: false as const, d }),
    (e) => {
      if (e instanceof ApiError && e.status === 403) return { forbidden: true as const }
      throw e
    },
  )
}
