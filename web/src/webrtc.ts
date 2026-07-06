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
  hangup(): void
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
        await this.pc.setRemoteDescription({ type: 'answer', sdp: enhanceOpus(m.sdp) })
        await this.flushIce()
      }),
    )
    signal.on('sfu-offer', (m) =>
      this.enqueue(async () => {
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

  hangup() {
    this.pc.close()
    this.crypto?.close()
    this.signal.close()
  }
}
