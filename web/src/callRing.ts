/** Regras do toque de chamada, à parte para serem testáveis sem o browser. */

/** Um toque sem resposta pára sozinho (o chamador pode ter fechado a app sem
 *  cancelar, ou a ligação dele caiu antes do `call-cancel`). */
export const RING_TIMEOUT_MS = 45_000

/** Código da sala aberta agora (`#/r/<código>`), ou null. */
export function roomCodeInHash(hash: string): string | null {
  const m = /^#\/r\/([^?/]+)/.exec(hash)
  return m ? decodeURIComponent(m[1]) : null
}
