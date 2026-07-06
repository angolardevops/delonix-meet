/**
 * Cliente de presença: WebSocket pessoal que se mantém ligado enquanto a app
 * está aberta, para receber chamadas estilo WhatsApp em qualquer página.
 */
import { accessTokenValue } from './api'

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
  online = new Set<string>()

  connect() {
    this.closed = false
    this.open()
  }

  private open() {
    const token = accessTokenValue()
    if (!token) return
    const proto = location.protocol === 'https:' ? 'wss' : 'ws'
    const ws = new WebSocket(`${proto}://${location.host}/rtc?token=${token}`)
    this.ws = ws
    ws.onmessage = (e) => {
      const msg = JSON.parse(e.data) as PresenceEvent
      if (msg.type === 'presence') this.online = new Set(msg.online)
      this.handlers.forEach((h) => h(msg))
    }
    ws.onclose = () => {
      if (this.closed) return
      // Reconexão simples com backoff curto.
      this.reconnectTimer = window.setTimeout(() => this.open(), 2000)
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
