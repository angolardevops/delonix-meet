export interface PeerInfo {
  peer_id: string
  username: string
  host: boolean
  hand: boolean
}

export type ServerMsg =
  | { type: 'joined'; peer_id: string; peers: PeerInfo[] }
  | { type: 'peer-joined'; peer: PeerInfo }
  | { type: 'peer-left'; peer_id: string }
  | { type: 'offer'; from: string; sdp: string }
  | { type: 'answer'; from: string; sdp: string }
  | { type: 'ice'; from: string; candidate: RTCIceCandidateInit }
  | { type: 'chat'; from: string; username: string; text: string }
  | { type: 'reaction'; from: string; username: string; emoji: string }
  | { type: 'hand'; from: string; raised: boolean }
  | { type: 'recording'; from: string; username: string; active: boolean }
  | { type: 'transcript'; from: string; username: string; text: string }
  | { type: 'waiting' }
  | { type: 'waiting-join'; peer: PeerInfo }
  | { type: 'waiting-left'; peer_id: string }
  | { type: 'denied' }
  | { type: 'force-muted' }
  | { type: 'kicked' }
  | { type: 'breakout-move'; code: string; label: string; back: boolean }
  | { type: 'breakouts-created'; rooms: { code: string; label: string }[] }
  | { type: 'error'; message: string }
  | { type: 'sfu-offer'; sdp: string }
  | { type: 'sfu-answer'; sdp: string }
  | { type: 'sfu-ice'; candidate: RTCIceCandidateInit }

export type ClientMsg =
  | { type: 'offer'; to: string; sdp: string }
  | { type: 'answer'; to: string; sdp: string }
  | { type: 'ice'; to: string; candidate: RTCIceCandidateInit }
  | { type: 'chat'; text: string }
  | { type: 'reaction'; emoji: string }
  | { type: 'hand'; raised: boolean }
  | { type: 'recording'; active: boolean }
  | { type: 'transcript'; text: string }
  | { type: 'admit'; to: string }
  | { type: 'deny'; to: string }
  | { type: 'force-mute'; to: string }
  | { type: 'kick'; to: string }
  | { type: 'breakouts-create'; count: number }
  | { type: 'breakouts-close' }
  | { type: 'leave' }
  | { type: 'sfu-offer'; sdp: string }
  | { type: 'sfu-answer'; sdp: string }
  | { type: 'sfu-ice'; candidate: RTCIceCandidateInit }

/** Typed wrapper over the signaling WebSocket. */
export class Signaling {
  private ws: WebSocket
  private handlers = new Map<string, ((msg: never) => void)[]>()
  onclose: (() => void) | null = null

  constructor(roomToken: string) {
    const proto = location.protocol === 'https:' ? 'wss' : 'ws'
    this.ws = new WebSocket(`${proto}://${location.host}/ws?token=${roomToken}`)
    this.ws.onmessage = (e) => {
      const msg = JSON.parse(e.data) as ServerMsg
      console.debug('[signal] <-', msg.type, this.handlers.has(msg.type) ? '' : '(no handler!)')
      this.handlers.get(msg.type)?.forEach((h) => h(msg as never))
    }
    this.ws.onclose = () => this.onclose?.()
  }

  on<T extends ServerMsg['type']>(type: T, handler: (msg: Extract<ServerMsg, { type: T }>) => void) {
    const list = this.handlers.get(type) ?? []
    list.push(handler as (msg: never) => void)
    this.handlers.set(type, list)
  }

  send(msg: ClientMsg) {
    console.debug('[signal] ->', msg.type, this.ws.readyState === WebSocket.OPEN ? '' : '(socket not open!)')
    if (this.ws.readyState === WebSocket.OPEN) this.ws.send(JSON.stringify(msg))
  }

  close() {
    this.send({ type: 'leave' })
    this.ws.close()
  }
}
