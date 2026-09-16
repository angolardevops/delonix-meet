/**
 * Chamada directa (voz ou vídeo) vista por quem está nela: quem liga a quem,
 * e o que o servidor de presença já disse (`ringing`, `accepted`, `declined`).
 *
 * Vive fora da sala porque nasce antes dela — o `call-start` sai do diretório
 * e a sala só abre quando o servidor responde `ringing`. Guarda-se por código
 * de sala em `sessionStorage`, para um F5 dentro da chamada não esquecer a
 * quem se estava a ligar.
 */

export interface DirectCall {
  room_code: string
  kind: 'video' | 'voice'
  /** `out`: fui eu que liguei; `in`: atendi. */
  direction: 'out' | 'in'
  /** Nome do outro lado, quando é uma pessoa (não um grupo). */
  peer_name: string | null
  /** Só em `out`: o que o servidor respondeu ao `call-start`. */
  ringing: string[]
  offline: string[]
  accepted: string[]
  declined: string[]
  /** Chegou o `ringing`? Antes disso não se sabe se tocou em alguém. */
  answered_by_server: boolean
}

const KEY = (code: string) => `dx_call_${code}`

export function loadDirectCall(code: string): DirectCall | null {
  try {
    const raw = sessionStorage.getItem(KEY(code))
    return raw ? (JSON.parse(raw) as DirectCall) : null
  } catch {
    return null
  }
}

export function saveDirectCall(c: DirectCall): void {
  try {
    sessionStorage.setItem(KEY(c.room_code), JSON.stringify(c))
  } catch {
    /* sem armazenamento: vale enquanto a página estiver aberta */
  }
}

export type VoicePhase =
  | 'connecting' // a entrar na sala
  | 'calling' // entrei, liguei, ninguém atendeu ainda
  | 'declined' // todos os que tocaram recusaram
  | 'unavailable' // ninguém estava online: ficou chamada perdida
  | 'waiting' // atendi mas o outro lado já não está (ou ainda não chegou)
  | 'in-call' // há alguém comigo
  | 'reconnecting' // há alguém, mas a minha media está a recuperar
  | 'ended' // houve alguém e saiu

/**
 * A fase da chamada, só a partir do que o servidor disse. Pura, para se
 * testar: um «em chamada» mostrado sem ninguém do outro lado é a mentira que
 * esta função existe para não dizer.
 */
export function voicePhase(i: {
  roomState: string
  callState: string
  peers: number
  everHadPeer: boolean
  call: DirectCall | null
}): VoicePhase {
  if (i.roomState !== 'in') return 'connecting'
  if (i.peers > 0) {
    return i.callState === 'reconnecting' || i.callState === 'recovering' || i.callState === 'degraded'
      ? 'reconnecting'
      : 'in-call'
  }
  if (i.everHadPeer) return 'ended'
  const c = i.call
  if (!c || c.direction === 'in') return 'waiting'
  if (!c.answered_by_server) return 'calling'
  const reached = c.ringing.filter((id) => !c.declined.includes(id))
  if (reached.length > 0 || c.accepted.length > 0) return 'calling'
  if (c.declined.length > 0) return 'declined'
  return 'unavailable'
}
