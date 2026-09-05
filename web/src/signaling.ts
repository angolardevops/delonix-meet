/** Traço do quadro branco: pontos normalizados 0..1, cor CSS, espessura. */
export interface WbStroke {
  pts: [number, number][]
  c: string
  w: number
}

export interface PollView {
  id: string
  question: string
  options: string[]
  counts: number[]
  open: boolean
  by: string
  /** Quiz: índice da certa — só chega depois de a sondagem fechar. */
  correct: number | null
  /** Quiz com tempo: epoch ms do fim da votação. */
  ends_at: number | null
  total_right: number
  total_wrong: number
}

export interface QaView {
  id: string
  text: string
  by: string
  upvotes: number
  answered: boolean
}

export interface BreakoutRoom {
  code: string
  label: string
  people: string[]
}

export interface PeerInfo {
  peer_id: string
  username: string
  host: boolean
  hand: boolean
  cam: boolean
  mic: boolean
  /** Pode admitir convidados da sala de espera (anfitrião ou promovido). */
  can_admit?: boolean
  is_bot?: boolean
  is_pstn?: boolean
}

export type ServerMsg =
  | { type: 'joined'; peer_id: string; peers: PeerInfo[]; reconnect?: string }
  | { type: 'peer-reconnecting'; peer_id: string }
  /** Este nó vai fechar. Reconectar daqui a `reconnect_in_ms` (mais jitter)
   *  migra a sala para outro pod — ver `callRecovery`/Room.tsx. */
  | { type: 'draining'; reconnect_in_ms: number }
  | { type: 'peer-joined'; peer: PeerInfo }
  | { type: 'peer-left'; peer_id: string }
  | { type: 'offer'; from: string; sdp: string }
  | { type: 'answer'; from: string; sdp: string }
  | { type: 'ice'; from: string; candidate: RTCIceCandidateInit }
  | { type: 'chat'; from: string; username: string; text: string }
  | { type: 'reaction'; from: string; username: string; emoji: string }
  | { type: 'hand'; from: string; raised: boolean }
  | { type: 'media'; from: string; cam: boolean; mic: boolean }
  | { type: 'recording'; from: string; username: string; active: boolean }
  | { type: 'transcript'; from: string; username: string; text: string }
  | { type: 'transcript-interim'; from: string; username: string; text: string }
  | { type: 'transcription'; on: boolean; by: string }
  | { type: 'waiting' }
  | { type: 'waiting-join'; peer: PeerInfo }
  | { type: 'waiting-left'; peer_id: string }
  | { type: 'admit-role'; allowed: boolean }
  | { type: 'peer-role'; peer_id: string; can_admit: boolean }
  | { type: 'denied' }
  | { type: 'force-muted' }
  | { type: 'force-cam-off' }
  | { type: 'muted-all'; by: string; allow_unmute: boolean }
  | { type: 'host-changed'; from: string; to: string }
  | { type: 'kicked' }
  | { type: 'room-settings'; locked: boolean; host_share_only: boolean; chat_on?: boolean; allow_unmute?: boolean }
  | { type: 'share-granted'; allowed: boolean }
  | { type: 'share-request'; from: string; username: string }
  | { type: 'wb-open'; by: string }
  | { type: 'polls'; polls: PollView[] }
  | { type: 'qa'; questions: QaView[] }
  | { type: 'timer'; ends_at: number | null }
  | { type: 'server-recording'; active: boolean; by: string }
  | { type: 'wb-stroke'; stroke: WbStroke }
  | { type: 'wb-clear' }
  | { type: 'wb-close' }
  | { type: 'wb-state'; strokes: WbStroke[] }
  | { type: 'presenting'; from: string; on: boolean }
  | { type: 'breakout-move'; code: string; label: string; back: boolean; ends_at: number | null }
  | { type: 'breakouts-created'; rooms: BreakoutRoom[]; ends_at: number | null }
  | { type: 'error'; message: string }
  | { type: 'sfu-offer'; sdp: string }
  | { type: 'sfu-answer'; sdp: string }
  | { type: 'sfu-ice'; candidate: RTCIceCandidateInit }
  | { type: 'remote-control'; from: string; action: string; payload: any }

export type ClientMsg =
  | { type: 'offer'; to: string; sdp: string }
  | { type: 'answer'; to: string; sdp: string }
  | { type: 'ice'; to: string; candidate: RTCIceCandidateInit }
  | { type: 'chat'; text: string }
  | { type: 'reaction'; emoji: string }
  | { type: 'hand'; raised: boolean }
  | { type: 'media'; cam: boolean; mic: boolean }
  | { type: 'recording'; active: boolean }
  | { type: 'transcript'; text: string }
  | { type: 'transcript-interim'; text: string }
  | { type: 'transcription-toggle'; on: boolean }
  | { type: 'admit'; to: string }
  | { type: 'deny'; to: string }
  | { type: 'promote-admit'; to: string; allowed: boolean }
  | { type: 'force-mute'; to: string }
  | { type: 'force-cam'; to: string }
  | { type: 'mute-all'; allow_unmute: boolean }
  | { type: 'chat-toggle'; on: boolean }
  | { type: 'transfer-host'; to: string }
  | { type: 'kick'; to: string }
  | { type: 'room-lock'; locked: boolean }
  | { type: 'host-share-only'; on: boolean }
  | { type: 'share-grant'; to: string; allowed: boolean }
  | { type: 'share-request' }
  | { type: 'wb-open' }
  | { type: 'poll-create'; question: string; options: string[]; correct_option?: number | null; duration_secs?: number | null }
  | { type: 'poll-vote'; poll: string; option: number }
  | { type: 'poll-close'; poll: string }
  | { type: 'qa-ask'; text: string }
  | { type: 'qa-upvote'; id: string }
  | { type: 'qa-answered'; id: string }
  | { type: 'timer-set'; minutes: number }
  | { type: 'timer-clear' }
  | { type: 'server-record'; active: boolean; e2ee_key?: string | null }
  | { type: 'wb-stroke'; stroke: WbStroke }
  | { type: 'wb-clear' }
  | { type: 'wb-close' }
  | { type: 'screen-share'; on: boolean }
  /** De quem queremos VÍDEO (página visível da grelha). O SFU deixa de enviar
   *  o resto — o áudio de todos e o ecrã partilhado nunca dependem disto. */
  | {
      type: 'video-interest'
      peers: string[]
      /** Camada desejada por publicador. Decidida no cliente porque é lá que
       *  se sabe o tamanho do tile, a aba em segundo plano, a CPU, a bateria e
       *  a poupança de dados — ver `layerPolicy.ts`. É uma SUGESTÃO: a perda
       *  medida por RTCP corta por cima dela no servidor. */
      quality?: Record<string, 'q' | 'h' | 'f'>
    }
  | { type: 'breakouts-create'; count: number; minutes: number | null }
  | { type: 'breakout-rename'; code: string; label: string }
  | { type: 'breakout-add' }
  | { type: 'breakout-move-user'; name: string; code: string }
  | { type: 'breakouts-close' }
  | { type: 'leave' }
  | { type: 'sfu-offer'; sdp: string }
  | { type: 'sfu-answer'; sdp: string }
  | { type: 'sfu-ice'; candidate: RTCIceCandidateInit }
  | { type: 'remote-control'; to: string; action: string; payload: any }

/** Typed wrapper over the signaling WebSocket. */
export class Signaling {
  private ws: WebSocket
  private handlers = new Map<string, ((msg: never) => void)[]>()
  onclose: (() => void) | null = null

  /** Guarda o segredo de reclamação desta sala (R91). */
  static guardarSegredo(roomCode: string, segredo: string) {
    // `sessionStorage` e não `localStorage`: o segredo vale para ESTA aba e
    // para esta sessão. Num `localStorage` sobreviveria ao fecho do browser e
    // duas abas na mesma sala disputariam o mesmo lugar.
    try {
      sessionStorage.setItem(`dx_seat_${roomCode}`, segredo)
    } catch {
      /* modo privado: sem reclamação, entra-se de novo — degrada, não parte */
    }
  }

  /** Esquece o lugar. Chamado ao SAIR de propósito — sair não é cair. */
  static esquecerSegredo(roomCode: string) {
    try {
      sessionStorage.removeItem(`dx_seat_${roomCode}`)
    } catch {
      /* idem */
    }
  }

  private static lerSegredo(roomCode?: string): string {
    if (!roomCode) return ''
    try {
      return sessionStorage.getItem(`dx_seat_${roomCode}`) ?? ''
    } catch {
      return ''
    }
  }

  constructor(roomToken: string, roomCode?: string) {
    const proto = location.protocol === 'https:' ? 'wss' : 'ws'
    // `room` (código público, não sensível) permite ao load balancer fazer
    // consistent-hash por sala e fixar TODOS os pares da mesma sala no MESMO
    // pod. O SFU é in-memory por pod: sem esta afinidade, dois participantes
    // podem cair em pods diferentes e não trocar media (admissão/partilha/vídeo
    // falham). Ver deploy/k8s/40-ingress.yaml (upstream-hash-by: $arg_room).
    const room = roomCode ? `&room=${encodeURIComponent(roomCode)}` : ''
    // Se houver um lugar reservado desta sala, reclama-se (R91). Sem segredo,
    // entra-se de novo — que é exactamente o que acontecia antes.
    const seg = Signaling.lerSegredo(roomCode)
    const reclamar = seg ? `&reconnect=${encodeURIComponent(seg)}` : ''
    this.ws = new WebSocket(
      `${proto}://${location.host}/ws?token=${roomToken}${room}${reclamar}`,
    )
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
