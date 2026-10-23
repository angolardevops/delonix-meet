/**
 * Webinar (sala de formato `broadcast`, A-09): quem está em palco publica,
 * quem assiste não. A regra é a MESMA do servidor (`signaling::em_palco`) — o
 * servidor é que a impõe no SFU; isto só decide o que o ecrã oferece.
 */
import type { Role } from '../signaling'

export function emPalco(role: Role): boolean {
  return role === 'host' || role === 'cohost' || role === 'speaker' || role === 'broadcast'
}

/** Num webinar, este papel só assiste? Fora de um webinar, ninguém só assiste. */
export function aAssistir(formatoBroadcast: boolean, role: Role): boolean {
  return formatoBroadcast && !emPalco(role)
}
