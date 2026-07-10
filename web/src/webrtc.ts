import { FrameCrypto } from './e2ee'
import { ClientMsg, Signaling } from './signaling'

/**
 * Melhora o Opus no SDP *recebido*: o fmtp do lado remoto é o que o nosso
 * encoder respeita, por isso pedimos 128 kbps, estéreo e FEC em banda —
 * o default do Chrome (~32 kbps mono) é o que faz o áudio soar "abafado".
 */
export function enhanceOpus(sdp: string): string {
  // Linha completa do rtpmap (inclui o sufixo /2 dos canais).
  const OPUS = /a=rtpmap:(\d+) opus\/48000(?:\/\d+)?/i.exec(sdp)
  if (!OPUS) return sdp
  const pt = OPUS[1]
  const fmtpRe = new RegExp(`a=fmtp:${pt} ([^\r\n]*)`)
  const extra = 'maxaveragebitrate=128000;stereo=1;sprop-stereo=1;useinbandfec=1'
  const m = fmtpRe.exec(sdp)
  if (!m) return sdp.replace(OPUS[0], `${OPUS[0]}\r\na=fmtp:${pt} ${extra}`)
  let params = m[1]
  for (const kv of extra.split(';')) {
    const key = kv.split('=')[0]
    params = params.includes(`${key}=`) ? params.replace(new RegExp(`${key}=\\d+`), kv) : `${params};${kv}`
  }
  return sdp.replace(fmtpRe, `a=fmtp:${pt} ${params}`)
}

/** Config extra para E2EE: o Chrome exige a flag na criação do PC. */
function pcConfig(base: RTCConfiguration, crypto?: FrameCrypto): RTCConfiguration {
  return crypto ? ({ ...base, encodedInsertableStreams: true } as RTCConfiguration) : base
}

/**
 * Simulcast (só SFU): o browser envia 3 camadas do mesmo vídeo — `q` (¼),
 * `h` (½) e `f` (inteira) — e o SFU encaminha a camada certa a cada
 * subscritor conforme o tamanho da sala. Uplink sobe ~35%, downlink de
 * salas grandes cai para uma fração.
 */
const SIMULCAST_ENCODINGS: RTCRtpEncodingParameters[] = [
  { rid: 'q', scaleResolutionDownBy: 4, maxBitrate: 300_000 },
  { rid: 'h', scaleResolutionDownBy: 2, maxBitrate: 1_200_000 },
  { rid: 'f', maxBitrate: 6_000_000 },
]

function addSimulcastVideo(pc: RTCPeerConnection, track: MediaStreamTrack, stream: MediaStream): RTCRtpSender {
  try {
    const tr = pc.addTransceiver(track, {
      direction: 'sendrecv',
      streams: [stream],
      sendEncodings: SIMULCAST_ENCODINGS,
    })
    return tr.sender
  } catch {
    // Browser sem suporte a sendEncodings: cai para camada única.
    return pc.addTrack(track, stream)
  }
}

export interface CallCallbacks {
  /** Media chegou (ou mudou) para um participante. */
  onStream: (peerId: string, stream: MediaStream) => void
  /** A ligação de media com o participante caiu. */
  onPeerLeft: (peerId: string) => void
}

export interface Call {
  replaceVideoTrack(track: MediaStreamTrack): Promise<void>
  replaceAudioTrack(track: MediaStreamTrack): Promise<void>
  /** Adiciona/substitui a track de vídeo publicada, renegociando se preciso. */
  enableVideo(track: MediaStreamTrack, stream: MediaStream): Promise<void>
  /**
   * Publica o ecrã como track ADICIONAL (a câmara continua a fluir).
   * No mesh (sem SFU) cai para o comportamento antigo: substitui a câmara.
   */
  startScreen(track: MediaStreamTrack, stream: MediaStream): Promise<void>
  stopScreen(): Promise<void>
  /** Telemetria QoS por participante (só SFU): kbps recebidos e perda. */
  qos?(): Promise<QosReport>
  hangup(): void
}

export interface QosReport {
  /** RTT até ao SFU em ms (transporte partilhado por todos os peers). */
  rtt: number | null
  /** Uplink próprio: kbps enviados (todas as tracks). */
  upKbps: number
  byPeer: Record<string, { kbps: number; lossPct: number }>
}

/**
 * Teto alto de bitrate: o controlo de congestão do WebRTC (transport-cc/REMB)
 * sobe até aqui quando a largura de banda de upload permite e desce sozinho
 * quando não — é isto que dá a adaptação automática à rede.
 */
async function tuneVideoSender(sender: RTCRtpSender) {
  try {
    const params = sender.getParameters()
    params.degradationPreference = 'balanced'
    if (!params.encodings?.length) params.encodings = [{}]
    params.encodings[0].maxBitrate = 6_000_000
    await sender.setParameters(params)
  } catch {
    /* parâmetros não suportados — segue com defaults */
  }
}

async function replaceTrackOfKind(pcs: Iterable<RTCPeerConnection>, kind: string, track: MediaStreamTrack) {
  for (const pc of pcs) {
    const sender = pc.getSenders().find((s) => s.track?.kind === kind)
    if (sender) await sender.replaceTrack(track)
  }
}

/**
 * Mesh: um RTCPeerConnection por participante remoto (P2P, SRTP direto
 * entre browsers). Ideal para 1:1 e salas pequenas. O recém-chegado cria a
 * oferta para cada peer existente — só um lado de cada par inicia.
 */
export class MeshCall implements Call {
  private pcs = new Map<string, RTCPeerConnection>()
  private pendingIce = new Map<string, RTCIceCandidateInit[]>()

  constructor(
    private signal: Signaling,
    private localStream: MediaStream,
    private rtcConfig: RTCConfiguration,
    private cb: CallCallbacks,
    private crypto?: FrameCrypto,
  ) {
    signal.on('joined', async ({ peers }) => {
      for (const p of peers) await this.callPeer(p.peer_id)
    })
    signal.on('peer-left', ({ peer_id }) => this.dropPeer(peer_id))
    signal.on('offer', async ({ from, sdp }) => {
      const pc = this.getOrCreatePc(from)
      await pc.setRemoteDescription({ type: 'offer', sdp: enhanceOpus(sdp) })
      await this.flushIce(from, pc)
      const answer = await pc.createAnswer()
      await pc.setLocalDescription(answer)
      this.signal.send({ type: 'answer', to: from, sdp: answer.sdp! })
    })
    signal.on('answer', async ({ from, sdp }) => {
      const pc = this.pcs.get(from)
      if (!pc) return
      await pc.setRemoteDescription({ type: 'answer', sdp: enhanceOpus(sdp) })
      await this.flushIce(from, pc)
    })
    signal.on('ice', async ({ from, candidate }) => {
      const pc = this.pcs.get(from)
      if (pc && pc.remoteDescription) {
        await pc.addIceCandidate(candidate).catch(() => {})
      } else {
        const queue = this.pendingIce.get(from) ?? []
        queue.push(candidate)
        this.pendingIce.set(from, queue)
      }
    })
  }

  private getOrCreatePc(peerId: string): RTCPeerConnection {
    let pc = this.pcs.get(peerId)
    if (pc) return pc
    pc = new RTCPeerConnection(pcConfig(this.rtcConfig, this.crypto))
    this.pcs.set(peerId, pc)

    for (const track of this.localStream.getTracks()) {
      const sender = pc.addTrack(track, this.localStream)
      this.crypto?.protectSender(sender)
      if (track.kind === 'video') void tuneVideoSender(sender)
    }
    pc.onicecandidate = (e) => {
      if (e.candidate) this.signal.send({ type: 'ice', to: peerId, candidate: e.candidate.toJSON() })
    }
    const remote = new MediaStream()
    pc.ontrack = (e) => {
      this.crypto?.protectReceiver(e.receiver, e.track.kind)
      remote.addTrack(e.track)
      this.cb.onStream(peerId, remote)
    }
    pc.onconnectionstatechange = () => {
      if (pc!.connectionState === 'failed' || pc!.connectionState === 'closed') {
        this.dropPeer(peerId)
      }
    }
    return pc
  }

  private async callPeer(peerId: string) {
    const pc = this.getOrCreatePc(peerId)
    // Sem tracks locais (sem cam/mic) queremos na mesma *receber* media.
    if (pc.getSenders().length === 0) {
      pc.addTransceiver('audio', { direction: 'recvonly' })
      pc.addTransceiver('video', { direction: 'recvonly' })
    }
    const offer = await pc.createOffer()
    await pc.setLocalDescription(offer)
    this.signal.send({ type: 'offer', to: peerId, sdp: offer.sdp! })
  }

  private async flushIce(peerId: string, pc: RTCPeerConnection) {
    for (const c of this.pendingIce.get(peerId) ?? []) {
      await pc.addIceCandidate(c).catch(() => {})
    }
    this.pendingIce.delete(peerId)
  }

  private dropPeer(peerId: string) {
    this.pcs.get(peerId)?.close()
    this.pcs.delete(peerId)
    this.pendingIce.delete(peerId)
    this.cb.onPeerLeft(peerId)
  }

  async replaceVideoTrack(track: MediaStreamTrack) {
    await replaceTrackOfKind(this.pcs.values(), 'video', track)
  }

  async replaceAudioTrack(track: MediaStreamTrack) {
    await replaceTrackOfKind(this.pcs.values(), 'audio', track)
  }

  /** Mesh: sem track extra — o ecrã substitui a câmara (comportamento antigo). */
  async startScreen(track: MediaStreamTrack) {
    await this.replaceVideoTrack(track)
  }

  async stopScreen() {
    /* o Room repõe a câmara via replaceVideoTrack */
  }

  /** Mesh: adiciona a track em cada peer e renegoceia (re-oferta). */
  async enableVideo(track: MediaStreamTrack, stream: MediaStream) {
    for (const [peerId, pc] of this.pcs) {
      const sender = pc.getSenders().find((s) => s.track?.kind === 'video')
      if (sender) {
        await sender.replaceTrack(track)
        continue
      }
      const s = pc.addTrack(track, stream)
      this.crypto?.protectSender(s)
      void tuneVideoSender(s)
      const offer = await pc.createOffer()
      await pc.setLocalDescription(offer)
      this.signal.send({ type: 'offer', to: peerId, sdp: offer.sdp! })
    }
  }

  hangup() {
    for (const [id] of this.pcs) this.dropPeer(id)
    this.crypto?.close()
    this.signal.close()
  }
}

/**
 * SFU: um único RTCPeerConnection ligado ao servidor Rust, que reencaminha
 * o media de/para todos os participantes. Escala para salas grandes.
 * O cliente faz a oferta inicial (publicação); depois o servidor é sempre
 * o ofertante (novas subscrições) — sem glare por construção.
 */
export class SfuCall implements Call {
  private pc: RTCPeerConnection
  private pendingIce: RTCIceCandidateInit[] = []
  private screenSender: RTCRtpSender | null = null
  private screenAudioSender: RTCRtpSender | null = null
  /** Serializa o processamento de SDP para não intercalar negociações. */
  private queue: Promise<void> = Promise.resolve()

  constructor(
    private signal: Signaling,
    localStream: MediaStream,
    rtcConfig: RTCConfiguration,
    private cb: CallCallbacks,
    private crypto?: FrameCrypto,
  ) {
    this.pc = new RTCPeerConnection(pcConfig(rtcConfig, crypto))

    for (const track of localStream.getTracks()) {
      const sender =
        track.kind === 'video'
          ? addSimulcastVideo(this.pc, track, localStream)
          : this.pc.addTrack(track, localStream)
      this.crypto?.protectSender(sender)
    }
    if (localStream.getTracks().length === 0) {
      this.pc.addTransceiver('audio', { direction: 'recvonly' })
      this.pc.addTransceiver('video', { direction: 'recvonly' })
    }

    this.pc.onicecandidate = (e) => {
      if (e.candidate) this.send({ type: 'sfu-ice', candidate: e.candidate.toJSON() })
    }
    this.pc.ontrack = (e) => {
      // O SFU reencaminha frames encriptados sem os conseguir abrir;
      // a desencriptação acontece aqui, no receiver de cada cliente.
      this.crypto?.protectReceiver(e.receiver, e.track.kind)
      // O SFU define stream_id = peer_id do publisher.
      const stream = e.streams[0]
      if (stream) this.cb.onStream(stream.id, stream)
    }

    signal.on('sfu-answer', (m) =>
      this.enqueue(async () => {
        // Só aplica a resposta se ainda esperamos uma (a nossa oferta pode ter
        // sido descartada por rollback numa colisão) — evita erro de estado.
        if (this.pc.signalingState !== 'have-local-offer') return
        await this.pc.setRemoteDescription({ type: 'answer', sdp: enhanceOpus(m.sdp) }).catch(() => {})
        await this.flushIce()
      }),
    )
    signal.on('sfu-offer', (m) =>
      this.enqueue(async () => {
        // Perfect negotiation: se há uma oferta local pendente (glare — o servidor
        // renegociou ao mesmo tempo que nós ofertámos), faz ROLLBACK antes de
        // aceitar a do servidor. Sem isto o setRemoteDescription(offer) falhava e
        // o subscritor (ex.: o anfitrião a receber um novo convidado) nunca
        // adicionava a track → media num sentido só.
        if (this.pc.signalingState !== 'stable') {
          await this.pc.setLocalDescription({ type: 'rollback' }).catch(() => {})
        }
        await this.pc.setRemoteDescription({ type: 'offer', sdp: enhanceOpus(m.sdp) })
        await this.flushIce()
        const answer = await this.pc.createAnswer()
        await this.pc.setLocalDescription(answer)
        this.send({ type: 'sfu-answer', sdp: answer.sdp! })
      }),
    )
    signal.on('sfu-ice', (m) =>
      this.enqueue(async () => {
        if (this.pc.remoteDescription) await this.pc.addIceCandidate(m.candidate).catch(() => {})
        else this.pendingIce.push(m.candidate)
      }),
    )
    // peer-left: o SFU remove as tracks e renegoceia; a UI limpa o tile
    // via roster, mas garantimos o callback para libertar o stream.
    signal.on('peer-left', ({ peer_id }) => this.cb.onPeerLeft(peer_id))

    // Oferta inicial (publica as nossas tracks) só depois do `joined` —
    // um convidado na sala de espera ainda não tem PC no servidor.
    let started = false
    signal.on('joined', () => {
      if (started) return
      started = true
      this.enqueue(async () => {
        const offer = await this.pc.createOffer()
        await this.pc.setLocalDescription(offer)
        this.send({ type: 'sfu-offer', sdp: offer.sdp! })
      })
    })
  }

  private send(msg: ClientMsg) {
    this.signal.send(msg)
  }

  private enqueue(task: () => Promise<void>) {
    this.queue = this.queue.then(task).catch((e) => console.warn('[sfu]', e))
  }

  private async flushIce() {
    for (const c of this.pendingIce) await this.pc.addIceCandidate(c).catch(() => {})
    this.pendingIce = []
  }

  async replaceVideoTrack(track: MediaStreamTrack) {
    await replaceTrackOfKind([this.pc], 'video', track)
  }

  async replaceAudioTrack(track: MediaStreamTrack) {
    await replaceTrackOfKind([this.pc], 'audio', track)
  }

  /**
   * SFU: se já há sender de vídeo, substitui a track; senão adiciona-a e
   * renegocia (o cliente volta a ofertar). Serializado pela fila para não
   * intercalar com renegociações do servidor.
   */
  async enableVideo(track: MediaStreamTrack, stream: MediaStream) {
    const sender = this.pc.getSenders().find((s) => s.track?.kind === 'video')
    if (sender) {
      await sender.replaceTrack(track)
      return
    }
    const s = addSimulcastVideo(this.pc, track, stream)
    this.crypto?.protectSender(s)
    this.enqueue(async () => {
      const offer = await this.pc.createOffer()
      await this.pc.setLocalDescription(offer)
      this.send({ type: 'sfu-offer', sdp: offer.sdp! })
    })
  }

  /**
   * SFU: o ecrã vai como track adicional (sem simulcast) num transceiver
   * próprio — a câmara continua a fluir; o servidor trata-o como
   * "apresentação" e o gravador guarda-o em ficheiro separado.
   */
  async startScreen(track: MediaStreamTrack, stream: MediaStream) {
    if (this.screenSender) return
    // Avisa o SFU ANTES de a track chegar: vídeo sem rid = ecrã.
    this.send({ type: 'screen-share', on: true })
    const sender = this.pc.addTrack(track, stream)
    this.crypto?.protectSender(sender)
    void tuneVideoSender(sender)
    this.screenSender = sender
    // Áudio do sistema (partilha de separador/ecrã com som): 2º áudio do
    // mesmo peer — o SFU classifica-o como "screen-audio".
    const sysAudio = stream.getAudioTracks()[0]
    if (sysAudio) {
      const aSender = this.pc.addTrack(sysAudio, stream)
      this.crypto?.protectSender(aSender)
      this.screenAudioSender = aSender
    }
    this.enqueue(async () => {
      const offer = await this.pc.createOffer()
      await this.pc.setLocalDescription(offer)
      this.send({ type: 'sfu-offer', sdp: offer.sdp! })
    })
  }

  async stopScreen() {
    const sender = this.screenSender
    if (!sender) return
    this.screenSender = null
    this.send({ type: 'screen-share', on: false })
    this.pc.removeTrack(sender)
    if (this.screenAudioSender) {
      this.pc.removeTrack(this.screenAudioSender)
      this.screenAudioSender = null
    }
    this.enqueue(async () => {
      const offer = await this.pc.createOffer()
      await this.pc.setLocalDescription(offer)
      this.send({ type: 'sfu-offer', sdp: offer.sdp! })
    })
  }

  /** Deltas de bytes/pacotes entre chamadas ao qos() (por stat id). */
  private lastQos = new Map<string, { bytes: number; ts: number; pkts: number; lost: number }>()

  async qos(): Promise<QosReport> {
    const out: QosReport = { rtt: null, upKbps: 0, byPeer: {} }
    const report = await this.pc.getStats()
    const uuidRe = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/
    report.forEach((s: Record<string, number | string | boolean>) => {
      if (s.type === 'candidate-pair' && s.state === 'succeeded' && typeof s.currentRoundTripTime === 'number') {
        out.rtt = Math.round((s.currentRoundTripTime as number) * 1000)
      }
      if (s.type === 'outbound-rtp') {
        const prev = this.lastQos.get(String(s.id))
        const bytes = (s.bytesSent as number) ?? 0
        const ts = s.timestamp as number
        if (prev && ts > prev.ts) out.upKbps += Math.max(0, Math.round(((bytes - prev.bytes) * 8) / (ts - prev.ts)))
        this.lastQos.set(String(s.id), { bytes, ts, pkts: 0, lost: 0 })
      }
      if (s.type === 'inbound-rtp') {
        // trackIdentifier = "<publisher-uuid>-<kind>-<rid>" (definido pelo SFU)
        const tid = String(s.trackIdentifier ?? '')
        if (!uuidRe.test(tid)) return
        const peer = tid.slice(0, 36)
        const prev = this.lastQos.get(String(s.id))
        const bytes = (s.bytesReceived as number) ?? 0
        const pkts = (s.packetsReceived as number) ?? 0
        const lost = (s.packetsLost as number) ?? 0
        const ts = s.timestamp as number
        let kbps = 0
        let lossPct = 0
        if (prev && ts > prev.ts) {
          kbps = Math.max(0, Math.round(((bytes - prev.bytes) * 8) / (ts - prev.ts)))
          const dp = pkts - prev.pkts
          const dl = lost - prev.lost
          if (dp + dl > 0) lossPct = Math.round((100 * dl) / (dp + dl))
        }
        this.lastQos.set(String(s.id), { bytes, ts, pkts, lost })
        const cur = out.byPeer[peer] ?? { kbps: 0, lossPct: 0 }
        out.byPeer[peer] = { kbps: cur.kbps + kbps, lossPct: Math.max(cur.lossPct, lossPct) }
      }
    })
    return out
  }

  hangup() {
    this.pc.close()
    this.crypto?.close()
    this.signal.close()
  }
}
