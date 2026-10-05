import { ApiError, DialOut, DialOutStatus } from '../api'

/** Ainda a tocar ou em chamada: o que se continua a acompanhar e se pode desligar. */
export const VIVO: readonly DialOutStatus[] = ['queued', 'dialing', 'ringing', 'in_call']

export function estaVivo(s: DialOutStatus): boolean {
  return VIVO.includes(s)
}

/** O tom do crachá: em chamada é sucesso, a tocar é aviso, o que falhou é aviso, o resto é neutro. */
export function tomDoEstado(s: DialOutStatus): 'success' | 'warning' | 'neutral' | 'live' {
  switch (s) {
    case 'in_call':
      return 'success'
    case 'queued':
    case 'dialing':
    case 'ringing':
      return 'live'
    case 'declined':
    case 'no_answer':
    case 'failed':
      return 'warning'
    default:
      return 'neutral'
  }
}

/**
 * A chave i18n da recusa, a partir do `code` estável do servidor. Sem código
 * conhecido devolve `null` e quem chama usa a mensagem genérica — nunca se
 * mostra ao utilizador o texto técnico do servidor como se fosse da interface.
 */
export function chaveDoErro(e: unknown): string | null {
  if (!(e instanceof ApiError)) return null
  const code = (e.body as { code?: string } | null)?.code
  if (!code) return null
  const conhecidos = [
    'dial_out.room_e2ee',
    'dial_out.room_recording',
    'dial_out.extension_inactive',
    'dial_out.not_configured',
    'dial_out.bridge_not_configured',
    'dial_out.already_active',
    'dial_out.too_many',
    'dial_out.rate_limited',
    'authz.missing_capability',
  ]
  return conhecidos.includes(code) ? `room.ligar.erro.${code.replace('.', '_')}` : null
}

/** A causa que a pessoa percebe, quando um pedido falhou. */
export function chaveDaFalha(d: Pick<DialOut, 'status' | 'failure_code'>): string | null {
  if (d.status !== 'failed' || !d.failure_code) return null
  if (d.failure_code === 'USER_NOT_REGISTERED') return 'room.ligar.falha.naoRegistado'
  if (d.failure_code === 'stale') return 'room.ligar.falha.semResposta'
  return 'room.ligar.falha.outra'
}

/** Mais recente primeiro, sem repetidos, e só os `n` primeiros. */
export function maisRecentes(items: DialOut[], n: number): DialOut[] {
  return [...items].sort((a, b) => b.created_at.localeCompare(a.created_at)).slice(0, n)
}
