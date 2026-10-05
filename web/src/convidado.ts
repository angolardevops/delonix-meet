/**
 * A sessão de um CONVIDADO SEM CONTA numa reunião.
 *
 * Um convidado não tem sessão de aplicação: tem um bilhete de UMA sala — o
 * token de sala que `guestJoin` devolve, com os servidores ICE e o que ele pode
 * ver da sala — e mais nada. Este módulo é o único sítio que sabe guardar,
 * renovar e esquecer esse bilhete, e é por ele que a sala descobre quem é a
 * pessoa deste lado quando não há conta.
 *
 * O bilhete vive no `sessionStorage`, por código de sala: sobrevive a um F5
 * (que é como a sala reentra depois de uma quebra de rede) e morre com o
 * separador. O nome escolhido fica também no `localStorage`, só para vir já
 * preenchido da próxima vez — é uma conveniência, não um estado.
 */
import { currentUser, guestJoin, iceServers, joinRoom, type GuestJoinOk, type GuestRoomView, type Room } from './api'

export interface SessaoDeConvidado {
  code: string
  roomToken: string
  iceServers: RTCConfiguration
  room: GuestRoomView
  guest: { id: string; display_name: string }
  /** Epoch ms em que o token de sala deixa de servir. */
  expiraEm: number
}

/** Margem antes do fim do token em que já se pede outro: ligar com um token a segundos de expirar é ligar para ser recusado. */
export const MARGEM_DE_RENOVACAO_MS = 30_000

const chave = (code: string) => `dx_convidado_${code}`
const CHAVE_DO_NOME = 'dx_convidado_nome'

function sessao(): Storage | null {
  try {
    return globalThis.sessionStorage ?? null
  } catch {
    return null // modo privado, ou armazenamento bloqueado
  }
}

function local(): Storage | null {
  try {
    return globalThis.localStorage ?? null
  } catch {
    return null
  }
}

/** Guarda o bilhete que o servidor emitiu. */
export function guardarConvidado(ok: GuestJoinOk, agora = Date.now()): SessaoDeConvidado {
  const s: SessaoDeConvidado = {
    code: ok.room.code,
    roomToken: ok.room_token,
    iceServers: ok.ice_servers,
    room: ok.room,
    guest: ok.guest,
    expiraEm: agora + ok.expires_in * 1000,
  }
  try {
    sessao()?.setItem(chave(s.code), JSON.stringify(s))
    local()?.setItem(CHAVE_DO_NOME, s.guest.display_name)
  } catch {
    /* sem armazenamento a sessão vale só enquanto a página viver */
  }
  ultima = s
  return s
}

/** A sessão em memória, para um browser sem `sessionStorage`. */
let ultima: SessaoDeConvidado | null = null

/**
 * O bilhete guardado para esta sala — MESMO que o token já tenha expirado: o
 * nome continua a servir para pedir outro (`entrarNaSala`).
 */
export function convidadoDe(code: string): SessaoDeConvidado | null {
  try {
    const raw = sessao()?.getItem(chave(code))
    if (raw) return JSON.parse(raw) as SessaoDeConvidado
  } catch {
    /* bilhete ilegível: é como não haver */
  }
  return ultima?.code === code ? ultima : null
}

export function esquecerConvidado(code: string) {
  try {
    sessao()?.removeItem(chave(code))
  } catch {
    /* nada a fazer */
  }
  if (ultima?.code === code) ultima = null
}

/** `true` se o token já não serve (ou está a segundos disso). */
export function bilheteExpirado(s: SessaoDeConvidado, agora = Date.now()): boolean {
  return s.expiraEm - agora < MARGEM_DE_RENOVACAO_MS
}

/** O último nome com que alguém entrou como convidado neste browser. */
export function nomeDeConvidado(): string {
  try {
    return local()?.getItem(CHAVE_DO_NOME) ?? ''
  } catch {
    return ''
  }
}

/** Pede a entrada ao servidor e guarda o bilhete. As recusas saem como `GuestJoinError`. */
export async function entrarComoConvidado(code: string, nome: string): Promise<SessaoDeConvidado> {
  return guardarConvidado(await guestJoin(code, nome))
}

/**
 * A entrada na sala, para quem quer que seja: devolve o que a sessão precisa
 * para ligar — a sala, o token de sala e os servidores ICE.
 *
 * Um MEMBRO pede-os ao servidor com a sua sessão (`joinRoom`, `iceServers`).
 * Um CONVIDADO já os tem no bilhete; se o token expirou entretanto (cinco
 * minutos parado na pré-entrada, ou um F5 tardio), pede-se outro com o mesmo
 * nome — e o lugar, se ele já estava na sala, volta pelo segredo de
 * reclamação, sem nova espera.
 *
 * Quem tem conta nunca entra como convidado, mesmo com um bilhete antigo
 * guardado: a conta ganha.
 */
export async function entrarNaSala(
  code: string,
  agora = Date.now(),
): Promise<[{ room: Room; room_token: string; scheduled?: boolean }, RTCConfiguration]> {
  let c = currentUser() ? null : convidadoDe(code)
  if (!c) return Promise.all([joinRoom(code), iceServers()])
  if (bilheteExpirado(c, agora)) c = await entrarComoConvidado(code, c.guest.display_name)
  return [{ room: salaDoConvidado(c.room), room_token: c.roomToken }, c.iceServers]
}

/**
 * A sala como a sessão a lê, a partir do pouco que um convidado vê dela. O
 * dono fica vazio de propósito — um convidado nunca o é — e a sala de espera
 * está sempre ligada para ele, diga o que disser a sala.
 */
function salaDoConvidado(v: GuestRoomView): Room {
  return { id: '', code: v.code, name: v.name, owner_id: '', topology: v.topology, waiting_room: true, e2ee: v.e2ee, format: v.format }
}

/**
 * Quem está DESTE lado da sala: a conta, ou o convidado sem conta da sala que
 * o endereço mostra. É o que a sala usa para escrever o próprio nome — o
 * `currentUser()` sozinho dava um retrato sem nome a cada convidado.
 */
export function participanteLocal(): { id: string | null; username: string } {
  const u = currentUser()
  if (u) return { id: u.id, username: u.username }
  const c = salaNoEndereco()
  return c ? { id: c.guest.id, username: c.guest.display_name } : { id: null, username: '' }
}

/** `true` se quem está nesta sala entrou sem conta. */
export function souConvidado(): boolean {
  return !currentUser() && salaNoEndereco() !== null
}

function salaNoEndereco(): SessaoDeConvidado | null {
  const hash = typeof location === 'undefined' ? '' : location.hash
  const m = hash.match(/^#\/r\/([a-z-]+)/)
  return m ? convidadoDe(m[1]) : null
}
