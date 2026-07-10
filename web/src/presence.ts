/**
 * Cliente de presença: WebSocket pessoal que se mantém ligado enquanto a app
 * está aberta, para receber chamadas estilo WhatsApp em qualquer página.
 */
import { accessTokenValue, tryRefreshToken } from './api'

/** Verdadeiro se o JWT expirou (ou expira nos próximos 30s). Decodifica só o
 *  payload — sem verificar assinatura (é só para decidir refrescar antes de ligar). */
function jwtExpired(token: string): boolean {
  try {
    const payload = token.split('.')[1]
    const json = JSON.parse(atob(payload.replace(/-/g, '+').replace(/_/g, '/')))
    return typeof json.exp === 'number' && Date.now() / 1000 >= json.exp - 30
  } catch {
    return true
  }
}

export interface IncomingCall {
  room_code: string
  kind: 'video' | 'voice'
  caller_id: string
  caller_name: string
  title: string
}

export type PresenceEvent =
  | { type: 'incoming-call'; room_code: string; kind: 'video' | 'voice'; caller_id: string; caller_name: string; title: string }
  | { type: 'ringing'; room_code: string; kind: 'video' | 'voice'; ringing: string[]; offline: string[] }
  | { type: 'accepted'; room_code: string; by_id: string; by_name: string }
  | { type: 'declined'; room_code: string; by_id: string; by_name: string }
  | { type: 'cancelled'; room_code: string }
  | { type: 'presence'; online: string[] }
  | { type: 'user-online'; user_id: string }
  | { type: 'user-offline'; user_id: string }
  | { type: 'meeting-declined'; meeting_id: string; meeting_title: string; by_id: string; by_name: string; reason: string }
  | { type: 'missed-calls'; calls: MissedCall[] }
  | { type: 'error'; message: string }

export interface MissedCall {
  id: string
  room_code: string
  caller_id: string
  caller_name: string
  kind: 'video' | 'voice'
  created_at: string
}

type Handler = (e: PresenceEvent) => void

export class Presence {
  private ws: WebSocket | null = null
  private handlers = new Set<Handler>()
  private closed = false
  private reconnectTimer: number | null = null
  // Backoff exponencial: 2s → 4s → 8s → … → 30s máx.
  private backoff = 2000
  online = new Set<string>()

  connect() {
    this.closed = false
    this.backoff = 2000
    this.open()
  }

  private async open() {
    let token = accessTokenValue()
    if (!token) return
    // Refresca ANTES de ligar se o token expirou — evita o 401 inicial no /rtc
    // (o WS não passa pelo fluxo automático de refresh do request()). Sessões
    // antigas ficavam com a presença em baixo até um reconnect tardio.
    if (jwtExpired(token)) {
      const ok = await tryRefreshToken()
      if (!ok || this.closed) return // sessão expirada → aguarda login manual
      token = accessTokenValue()
      if (!token) return
    }
    const proto = location.protocol === 'https:' ? 'wss' : 'ws'
    const ws = new WebSocket(`${proto}://${location.host}/rtc?token=${token}`)
    this.ws = ws
    ws.onmessage = (e) => {
      const msg = JSON.parse(e.data) as PresenceEvent
      if (msg.type === 'presence') this.online = new Set(msg.online)
      else if (msg.type === 'user-online') this.online = new Set([...this.online, msg.user_id])
      else if (msg.type === 'user-offline') { this.online = new Set([...this.online].filter(id => id !== msg.user_id)) }
      // Ligação estável: repõe o backoff.
      this.backoff = 2000
      this.handlers.forEach((h) => h(msg))
    }
    ws.onclose = () => {
      if (this.closed) return
      const delay = this.backoff
      this.backoff = Math.min(this.backoff * 2, 30_000)
      // Renova o token antes de reconectar — o access token tem 15 min de TTL
      // e o WS não passa pelo fluxo automático de refresh do `request()`.
      this.reconnectTimer = window.setTimeout(async () => {
        if (this.closed) return
        const ok = await tryRefreshToken()
        if (!ok) return // sessão expirada — aguarda login manual
        this.open()
      }, delay)
    }
  }

  on(h: Handler): () => void {
    this.handlers.add(h)
    return () => this.handlers.delete(h)
  }

  private send(msg: unknown) {
    if (this.ws?.readyState === WebSocket.OPEN) this.ws.send(JSON.stringify(msg))
  }

  startCall(opts: { targets?: string[]; groupId?: string; kind: 'video' | 'voice'; title?: string }) {
    this.send({
      type: 'call-start',
      targets: opts.targets ?? [],
      group_id: opts.groupId ?? null,
      kind: opts.kind,
      title: opts.title ?? null,
    })
  }
  accept(roomCode: string) {
    this.send({ type: 'call-accept', room_code: roomCode })
  }
  decline(roomCode: string) {
    this.send({ type: 'call-decline', room_code: roomCode })
  }
  cancel(roomCode: string) {
    this.send({ type: 'call-cancel', room_code: roomCode })
  }

  close() {
    this.closed = true
    if (this.reconnectTimer) clearTimeout(this.reconnectTimer)
    this.ws?.close()
    this.ws = null
  }
}
