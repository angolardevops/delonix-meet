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
  // `companion`: esta CONTA já estava na sala noutro dispositivo (R114). Quem
  // entra em segundo lugar entra sem áudio — dois microfones da mesma pessoa
  // no mesmo espaço físico fazem um ciclo de eco.
  | { type: 'joined'; peer_id: string; peers: PeerInfo[]; reconnect?: string; companion?: boolean }
  // A outra sessão desta conta saiu: já não há com quem fazer eco (R114).
  | { type: 'companion_ended' }
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

// ---------- frontend/b1-sala ----------
//
// Tipos novos da sinalização da sala. Ficam aqui em baixo (e não dentro das
// uniões de cima) para a integração com os outros ramos ser trivial: os
// `interface` re-declarados FUNDEM-SE com os de cima, e as mensagens novas
// vivem em uniões próprias com `sendB1`/`onB1` para as usar.

export type Role = 'host' | 'cohost' | 'speaker' | 'broadcast' | 'attendee'
export type Origin = 'sso' | 'password' | 'guest' | 'pstn' | 'bot'
export type WbKind = 'stroke' | 'text' | 'note' | 'shape'

export interface PeerInfo {
  /** Papel. Só `host` e `cohost` mudam permissões no servidor. */
  role?: Role
  /** Origem, decidida no servidor a partir do token. */
  origin?: Origin
  /** Cargo na organização do dono da sala, se houver. */
  title?: string
}

export interface WbStroke {
  /** Pode ser gerado pelo cliente (para apagar/mover o que desenhou); ausente → o servidor gera. */
  id?: string
  kind?: WbKind
  /** Texto de `text`/`note` (obrigatório nesses). */
  text?: string
  /** Forma de `shape`: os dois pontos são os cantos. */
  shape?: 'rect' | 'ellipse' | 'line' | 'arrow'
  /** Autor — carimbado pelo servidor; o que o cliente mandar é ignorado. */
  by?: string
  /** Página (0 = primeira). */
  page?: number
  /** Pressão por ponto, 0..1, mesmo comprimento de `pts`. */
  p?: number[]
}

export interface QaView {
  /** Só os anfitriões recebem perguntas escondidas. */
  hidden?: boolean
  spotlight?: boolean
}

export interface LiveDestination {
  label: string
  /** `connecting` | `live` | `error` | `stopped` */
  state: string
  kbps?: number
}

export type ServerMsgB1 =
  /** `joined` passou a trazer o início da sessão (epoch ms; 0 = desconhecido). */
  | { type: 'joined'; peer_id: string; peers: PeerInfo[]; reconnect?: string; companion?: boolean; started_at?: number }
  | { type: 'chat'; from: string; username: string; text: string; id?: string; at?: number; reply_to?: string }
  /** Só para quem enviou com `client_id`. */
  | { type: 'chat-sent'; client_id: string; id: string; at: number }
  /** Estado completo das reacções de uma mensagem. */
  | { type: 'chat-reactions'; id: string; counts: Record<string, number> }
  | { type: 'peer-role'; peer_id: string; role: Role; can_admit: boolean }
  | { type: 'spotlight'; peer: string | null }
  | { type: 'live'; on: boolean; destinations: LiveDestination[]; since: number | null }
  | {
      type: 'room-settings'
      locked: boolean
      host_share_only: boolean
      chat_on?: boolean
      allow_unmute?: boolean
      waiting_room?: boolean
    }
  | { type: 'wb-state'; strokes: WbStroke[] }
  | { type: 'wb-erase'; id: string }
  | { type: 'wb-transform'; id: string; dx: number; dy: number }
  | { type: 'wb-update'; id: string; text: string }
  | { type: 'wb-cursor'; from: string; x: number; y: number; laser: boolean; input?: 'mouse' | 'pen' | 'touch' }
  | { type: 'wb-pages'; count: number; current: number }
  | { type: 'wb-writers'; restricted: boolean; writers: string[] }
  | { type: 'announcement'; from: string; text: string; at: number }

export type ClientMsgB1 =
  | { type: 'chat'; text: string; reply_to?: string | null; client_id?: string | null }
  | { type: 'chat-react'; id: string; emoji: string }
  /** Só anfitrião. `host` não se dá por aqui (é o `transfer-host`). */
  | { type: 'set-role'; to: string; role: Exclude<Role, 'host'> }
  /** Só anfitrião. `null` limpa. */
  | { type: 'spotlight'; peer: string | null }
  /** Anfitrião ou co-anfitrião. */
  | { type: 'admit-all' }
  /** Só anfitrião. */
  | { type: 'waiting-room'; on: boolean }
  /** Só anfitrião. `hidden` por omissão `true`. */
  | { type: 'qa-hide'; id: string; hidden?: boolean }
  /** Só anfitrião. `null` limpa. */
  | { type: 'qa-spotlight'; id: string | null }
  | { type: 'wb-stroke'; stroke: WbStroke }
  /** Autor ou anfitrião. */
  | { type: 'wb-erase'; id: string }
  | { type: 'wb-transform'; id: string; dx: number; dy: number }
  | { type: 'wb-update'; id: string; text: string }
  /** Efémero; o servidor trava a ~20/s por emissor. */
  | { type: 'wb-cursor'; x: number; y: number; laser?: boolean; input?: 'mouse' | 'pen' | 'touch' }
  /** Quem pode escrever. */
  | { type: 'wb-add-page' }
  /** Anfitrião ou apresentador. */
  | { type: 'wb-page'; page: number }
  /** Só anfitrião. */
  | { type: 'wb-lock'; on: boolean }
  /** Só anfitrião. */
  | { type: 'wb-grant'; to: string; allowed: boolean }
  | { type: 'breakouts-create'; count: number; minutes: number | null; assign?: 'auto' | 'manual' }
  /** Só anfitrião: vai para a sala principal e todas as salas de grupo. */
  | { type: 'breakouts-broadcast'; text: string }

/** Envia uma mensagem nova da sala pelo mesmo socket. */
export function sendB1(sig: Signaling, msg: ClientMsgB1) {
  sig.send(msg as unknown as ClientMsg)
}

/** Subscreve uma mensagem nova da sala. */
export function onB1<T extends ServerMsgB1['type']>(
  sig: Signaling,
  type: T,
  handler: (msg: Extract<ServerMsgB1, { type: T }>) => void,
) {
  const on = sig.on as unknown as (this: Signaling, t: string, h: (msg: unknown) => void) => void
  on.call(sig, type, handler as (msg: unknown) => void)
}
