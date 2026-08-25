import { FrameCrypto } from './e2ee'
import { ClientMsg, Signaling } from './signaling'
import type { LinhaDoTempo } from './callTimings'
import {
  callQualityScore,
  extractQuality,
  type QualityCursor,
  type QualitySample,
  type StatEntry,
} from './callQuality'
import {
  DEFAULT_RECOVERY,
  onGraceExpired,
  onPeerState,
  type CallState,
  type RecoveryConfig,
} from './callRecovery'

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
  // `usedtx=1`: em silêncio o encoder deixa de enviar (≈0 kbps em vez de 128).
  // Numa sala de N pessoas, N-1 estão caladas a cada instante — é a diferença
  // entre (N-1)×128 kbps de downlink e praticamente nada.
  const extra = 'maxaveragebitrate=128000;stereo=1;sprop-stereo=1;useinbandfec=1;usedtx=1'
  // GLOBAL: cada m-line de áudio (microfone E áudio do ecrã partilhado) tem o
  // seu `a=fmtp`. Com `replace` não-global só a PRIMEIRA era tratada, e o áudio
  // da partilha de ecrã ficava com os defaults do browser.
  const fmtpReG = new RegExp(`a=fmtp:${pt} ([^\r\n]*)`, 'g')
  if (!fmtpReG.test(sdp)) {
    fmtpReG.lastIndex = 0
    return sdp.replace(new RegExp(`(a=rtpmap:${pt} opus/48000(?:/\\d+)?)`, 'g'), `$1\r\na=fmtp:${pt} ${extra}`)
  }
  fmtpReG.lastIndex = 0
  return sdp.replace(fmtpReG, (_full, params: string) => {
    let out = params
    for (const kv of extra.split(';')) {
      const key = kv.split('=')[0]
      out = out.includes(`${key}=`) ? out.replace(new RegExp(`${key}=\\d+`), kv) : `${out};${kv}`
    }
    return `a=fmtp:${pt} ${out}`
  })
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
  /**
   * Estado da ligação de media mudou (ver `callRecovery.ts`). Opcional: quem
   * não o fornecer mantém o comportamento anterior, sem recuperação visível.
   */
  onState?: (state: CallState) => void
}

export interface Call {
  replaceVideoTrack(track: MediaStreamTrack): Promise<void>
  replaceAudioTrack(track: MediaStreamTrack): Promise<void>
  /** Adiciona/substitui a track de vídeo publicada, renegociando se preciso. */
  enableVideo(track: MediaStreamTrack, stream: MediaStream): Promise<void>
  /**
   * Para de publicar vídeo SEM desmontar o transceiver (`replaceTrack(null)`).
   * Permite que o `Room` liberte mesmo a câmara (LED apaga, encoder pára) e
   * volte a ligá-la depois com `enableVideo` — sem renegociar e, no SFU, sem
   * perder as `sendEncodings` de simulcast, que só podem ser definidas na
   * criação do transceiver.
   */
  disableVideo(): Promise<void>
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

/**
 * Amostra completa de qualidade + a pontuação Delonix (0–100).
 *
 * Substituiu um relatório de TRÊS números (RTT, uplink, perda por peer). A
 * auditoria de 2026-08-25 mediu que era o que havia das ~25 métricas que o §4.4
 * pede — e com três números não há diagnóstico nem SLO defensável.
 */
export type QosReport = QualitySample & {
  /** Delonix Call Quality Score, 0–100. Ver `callQuality.ts` para o modelo
   *  e, sobretudo, para o que ele NÃO é (não é MOS, não está calibrado
   *  contra julgamento humano). */
  score: number
}

/**
 * Teto alto de bitrate: o controlo de congestão do WebRTC (transport-cc/REMB)
 * sobe até aqui quando a largura de banda de upload permite e desce sozinho
 * quando não — é isto que dá a adaptação automática à rede.
 */
async function tuneVideoSender(sender: RTCRtpSender, maxBitrate = 6_000_000, degradation: RTCDegradationPreference = 'balanced') {
  try {
    const params = sender.getParameters()
    params.degradationPreference = degradation
    if (!params.encodings?.length) params.encodings = [{}]
    params.encodings[0].maxBitrate = maxBitrate
    await sender.setParameters(params)
  } catch {
    /* parâmetros não suportados — segue com defaults */
  }
}

/**
 * Teto do ecrã partilhado. É MUITO mais baixo que o da câmara (6 Mbps) por uma
 * razão estrutural: o ecrã não tem simulcast, logo TODOS os subscritores
 * recebem exatamente este fluxo — não há camada leve para quem tem rede fraca.
 * 2,5 Mbps a 1080p com `degradationPreference: 'maintain-resolution'` mantém o
 * texto legível (o que interessa numa apresentação) e baixa o framerate quando
 * a banda aperta, em vez de desfocar.
 */
const SCREEN_MAX_BITRATE = 2_500_000

/**
 * Constraints do ecrã partilhado. Sem limite explícito, um monitor 4K era
 * capturado a 4K/60 e enviado tal-qual a toda a gente.
 */
export const SCREEN_CONSTRAINTS: DisplayMediaStreamOptions = {
  video: {
    width: { max: 1920 },
    height: { max: 1080 },
    frameRate: { ideal: 5, max: 15 },
  },
  // Áudio do sistema/separador (Chrome/Edge; o utilizador escolhe no picker)
  audio: { echoCancellation: false, noiseSuppression: false, autoGainControl: false },
}

async function replaceTrackOfKind(pcs: Iterable<RTCPeerConnection>, kind: string, track: MediaStreamTrack) {
  for (const pc of pcs) {
    const sender = pc.getSenders().find((s) => s.track?.kind === kind)
    if (sender) await sender.replaceTrack(track)
  }
}

/**
 * Sender que já ESTÁ a enviar `kind`, ou um transceiver `recvonly`/inativo do
 * mesmo tipo que possa ser convertido em emissor.
 *
 * Sem isto, quem entrava sem microfone (permissão negada, modo espectador, ou
 * só com câmara) nunca conseguia falar: `getSenders().find(s => s.track?.kind)`
 * não encontrava nada — o sender de um transceiver `recvonly` tem `track` a
 * `null` — e o `replaceAudioTrack` era um no-op silencioso. O botão do mic
 * acendia, o medidor de nível mexia, e não saía um único pacote.
 */
function reusableTransceiver(pc: RTCPeerConnection, kind: 'audio' | 'video'): RTCRtpTransceiver | null {
  return (
    pc.getTransceivers().find((t) => {
      if (t.direction === 'stopped' || t.currentDirection === 'stopped') return false
      const tk = t.receiver.track?.kind ?? t.sender.track?.kind
      return tk === kind && !t.sender.track
    }) ?? null
  )
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

  /** Mesh: mesma lógica do SFU — publicar o mic mesmo sem sender ativo. */
  async replaceAudioTrack(track: MediaStreamTrack) {
    const stream = new MediaStream([track])
    for (const [peerId, pc] of this.pcs) {
      const active = pc.getSenders().find((s) => s.track?.kind === 'audio')
      if (active) {
        await active.replaceTrack(track)
        continue
      }
      const reusable = reusableTransceiver(pc, 'audio')
      let sender: RTCRtpSender
      if (reusable) {
        await reusable.sender.replaceTrack(track)
        reusable.direction = 'sendrecv'
        sender = reusable.sender
      } else {
        sender = pc.addTrack(track, stream)
      }
      this.crypto?.protectSender(sender)
      const offer = await pc.createOffer()
      await pc.setLocalDescription(offer)
      this.signal.send({ type: 'offer', to: peerId, sdp: offer.sdp! })
    }
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

  async disableVideo() {
    for (const pc of this.pcs.values()) {
      const sender = pc.getSenders().find((s) => s.track?.kind === 'video')
      if (sender) await sender.replaceTrack(null)
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
  /** Sender da CÂMARA (não do ecrã). Guardado para o poder reutilizar em
   *  `enableVideo` depois de um `disableVideo` — sem isto, cada ciclo
   *  desligar/ligar acrescentava uma m-line nova de vídeo à sessão. */
  private videoSender: RTCRtpSender | null = null
  private screenSender: RTCRtpSender | null = null
  private screenAudioSender: RTCRtpSender | null = null
  /** Serializa o processamento de SDP para não intercalar negociações. */
  private queue: Promise<void> = Promise.resolve()

  // ---- Recuperação de chamada (ver callRecovery.ts) ----
  private state: CallState = 'connecting'
  private attempts = 0
  private graceTimer: ReturnType<typeof setTimeout> | null = null
  private restartTimer: ReturnType<typeof setTimeout> | null = null

  constructor(
    private signal: Signaling,
    localStream: MediaStream,
    rtcConfig: RTCConfiguration,
    private cb: CallCallbacks,
    private crypto?: FrameCrypto,
    private recovery: RecoveryConfig = DEFAULT_RECOVERY,
    /** Linha do tempo da sessão. Vem de FORA porque começa antes desta classe
     *  existir — o utilizador quis entrar muito antes de haver uma PC. */
    private tempos?: LinhaDoTempo,
  ) {
    this.pc = new RTCPeerConnection(pcConfig(rtcConfig, crypto))

    for (const track of localStream.getTracks()) {
      const sender =
        track.kind === 'video'
          ? addSimulcastVideo(this.pc, track, localStream)
          : this.pc.addTrack(track, localStream)
      if (track.kind === 'video') this.videoSender = sender
      this.crypto?.protectSender(sender)
    }
    if (localStream.getTracks().length === 0) {
      this.pc.addTransceiver('audio', { direction: 'recvonly' })
      this.pc.addTransceiver('video', { direction: 'recvonly' })
    }

    this.pc.onicecandidate = (e) => {
      if (e.candidate) this.send({ type: 'sfu-ice', candidate: e.candidate.toJSON() })
      // `candidate` a null = a recolha terminou. É o fim do `ice_gathering_ms`.
      else this.tempos?.marcar('ice_completo')
    }
    this.pc.ontrack = (e) => {
      // O SFU reencaminha frames encriptados sem os conseguir abrir;
      // a desencriptação acontece aqui, no receiver de cada cliente.
      this.crypto?.protectReceiver(e.receiver, e.track.kind)
      // O SFU define stream_id = peer_id do publisher.
      const stream = e.streams[0]
      if (stream) this.cb.onStream(stream.id, stream)
      // Primeira media de OUTRA pessoa. É o instante em que a reunião começa
      // de facto — antes disto o utilizador está a olhar para um ecrã vazio.
      this.tempos?.marcar(e.track.kind === 'audio' ? 'primeiro_audio' : 'primeiro_video')
    }

    // Recuperação de media. Antes disto não existia handler nenhum: uma PC que
    // caísse em `failed` — mudança de Wi-Fi para dados móveis, NAT a refazer o
    // binding, portátil a acordar da suspensão — ficava morta até o utilizador
    // recarregar a página.
    this.pc.onconnectionstatechange = () => {
      if (this.pc.connectionState === 'connected') this.tempos?.marcar('ligado')
      this.onPcState(this.pc.connectionState)
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
        const rolledBack = this.pc.signalingState !== 'stable'
        if (rolledBack) {
          await this.pc.setLocalDescription({ type: 'rollback' }).catch(() => {})
        }
        await this.pc.setRemoteDescription({ type: 'offer', sdp: enhanceOpus(m.sdp) })
        await this.flushIce()
        const answer = await this.pc.createAnswer()
        await this.pc.setLocalDescription(answer)
        this.send({ type: 'sfu-answer', sdp: answer.sdp! })
        // O rollback DESCARTOU a nossa oferta — e com ela a negociação das
        // tracks que ela publicava (ecrã, câmara). A resposta que o servidor
        // acabar por mandar a essa oferta já não nos serve: estamos `stable` e
        // o handler de `sfu-answer` descarta-a. Nada mais voltaria a propor
        // aquelas tracks e a partilha de ecrã desaparecia em silêncio — o
        // mesmo sintoma da R13, agora do lado do cliente. Por isso RE-OFERTAMOS.
        //
        // Não usamos `onnegotiationneeded` (que faria isto sozinho no browser)
        // porque toda a negociação desta classe é explícita e serializada pela
        // fila; misturar os dois daria ofertas a competir.
        if (rolledBack) {
          const reoffer = await this.pc.createOffer()
          await this.pc.setLocalDescription(reoffer)
          this.send({ type: 'sfu-offer', sdp: reoffer.sdp! })
        }
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

    // Oferta inicial (publica as nossas tracks). A SfuCall só é construída
    // DEPOIS do `joined` (ver Room.tsx `callHolder` — o convidado na sala de
    // espera não negocia). Como o evento `joined` já disparou antes de esta PC
    // existir, ofertamos JÁ na construção — esperar por outro `joined` (que não
    // volta) deixava o servidor sem PC e sem media.
    this.enqueue(async () => {
      const offer = await this.pc.createOffer()
      await this.pc.setLocalDescription(offer)
      this.tempos?.marcar('oferta')
      this.send({ type: 'sfu-offer', sdp: offer.sdp! })
    })
  }

  private send(msg: ClientMsg) {
    this.signal.send(msg)
  }

  private enqueue(task: () => Promise<void>) {
    this.queue = this.queue.then(task).catch((e) => console.warn('[sfu]', e))
  }

  // ------------------------------------------------------------------
  //  Recuperação de media. A DECISÃO vive em `callRecovery.ts` (pura e
  //  testada); aqui só se executa o que ela mandar.
  // ------------------------------------------------------------------

  private setState(next: CallState) {
    if (next === this.state) return
    this.state = next
    console.debug('[sfu] estado da chamada ->', next)
    this.cb.onState?.(next)
  }

  private clearTimers() {
    if (this.graceTimer) clearTimeout(this.graceTimer)
    if (this.restartTimer) clearTimeout(this.restartTimer)
    this.graceTimer = null
    this.restartTimer = null
  }

  private onPcState(pcState: RTCPeerConnectionState) {
    const d = onPeerState(pcState, this.state, this.attempts, this.recovery)
    this.attempts = d.attempts
    // Voltou a haver media: qualquer temporizador pendente é obsoleto. Sem
    // isto, uma recuperação espontânea deixava um restart agendado a disparar
    // depois, renegociando uma ligação que já estava boa.
    if (d.state === 'connected' || d.state === 'disconnected') this.clearTimers()
    // Recuperação COMPLETA: veio de um estado degradado e voltou a connected.
    if (d.state === 'connected' && this.state !== 'connected' && this.state !== 'connecting') {
      this.tempos?.contarRecuperacao()
    }
    this.setState(d.state)
    this.runAction(d.action)
  }

  private runAction(action: ReturnType<typeof onPeerState>['action']) {
    switch (action.kind) {
      case 'none':
        return
      case 'observe':
        if (this.graceTimer) return
        this.graceTimer = setTimeout(() => {
          this.graceTimer = null
          const d = onGraceExpired(this.state, this.attempts, this.recovery)
          this.attempts = d.attempts
          this.setState(d.state)
          this.runAction(d.action)
        }, action.graceMs)
        return
      case 'restart':
        if (this.restartTimer) return
        this.restartTimer = setTimeout(() => {
          this.restartTimer = null
          this.restartIce()
        }, action.delayMs)
        return
      case 'give-up':
        // Não há mais nada a tentar daqui. Quem consome o `onState` decide o
        // que oferecer (no Room.tsx, o recarregar que já era o comportamento).
        this.clearTimers()
        return
    }
  }

  /**
   * Reinicia o ICE: oferta nova com credenciais novas (`iceRestart`), o que
   * faz o browser recolher candidatos de raiz pelo caminho de rede ACTUAL.
   *
   * Passa pela mesma fila que todo o resto da negociação. Não se usa
   * `pc.restartIce()` (que dispararia `onnegotiationneeded`) porque nesta
   * classe a negociação é toda explícita e serializada — misturar os dois daria
   * ofertas a competir, que é a família de bugs da R13.
   *
   * Do lado do servidor não é preciso nada: o `webrtc-rs` detecta as
   * credenciais novas no `set_remote_description` e reinicia o seu ICE
   * (`peer_connection/mod.rs`, `have_remote_credentials_change`).
   */
  private restartIce() {
    this.enqueue(async () => {
      if (this.state === 'disconnected') return
      // Uma negociação a meio (glare, ou renegociação do servidor em curso):
      // ofertar agora falharia por estado. Volta a tentar em breve — a
      // tentativa já foi contabilizada, por isso isto não fura o orçamento.
      if (this.pc.signalingState !== 'stable') {
        this.restartTimer = setTimeout(() => {
          this.restartTimer = null
          this.restartIce()
        }, 500)
        return
      }
      this.setState('recovering')
      this.tempos?.contarReinicioIce()
      const offer = await this.pc.createOffer({ iceRestart: true })
      await this.pc.setLocalDescription(offer)
      this.send({ type: 'sfu-offer', sdp: offer.sdp! })
    })
  }

  private async flushIce() {
    for (const c of this.pendingIce) await this.pc.addIceCandidate(c).catch(() => {})
    this.pendingIce = []
  }

  async replaceVideoTrack(track: MediaStreamTrack) {
    await replaceTrackOfKind([this.pc], 'video', track)
  }

  /**
   * SFU: publica o microfone. Se já há sender de áudio ativo, troca a track;
   * senão reaproveita o transceiver `recvonly` (ou cria um) e RENEGOCEIA — sem
   * isto, quem entrasse sem mic ficava mudo para o resto da sessão.
   */
  async replaceAudioTrack(track: MediaStreamTrack) {
    const active = this.pc.getSenders().find((s) => s.track?.kind === 'audio')
    if (active) {
      await active.replaceTrack(track)
      return
    }
    const stream = new MediaStream([track])
    const reusable = reusableTransceiver(this.pc, 'audio')
    let sender: RTCRtpSender
    if (reusable) {
      await reusable.sender.replaceTrack(track)
      reusable.direction = 'sendrecv'
      sender = reusable.sender
    } else {
      sender = this.pc.addTrack(track, stream)
    }
    this.crypto?.protectSender(sender)
    this.enqueue(async () => {
      const offer = await this.pc.createOffer()
      await this.pc.setLocalDescription(offer)
      this.send({ type: 'sfu-offer', sdp: offer.sdp! })
    })
  }

  /**
   * SFU: se já há sender de vídeo, substitui a track; senão adiciona-a e
   * renegocia (o cliente volta a ofertar). Serializado pela fila para não
   * intercalar com renegociações do servidor.
   */
  async enableVideo(track: MediaStreamTrack, stream: MediaStream) {
    // Reutiliza o sender da câmara mesmo que esteja com `track === null`
    // (câmara desligada): mantém o simulcast e dispensa renegociação.
    const sender = this.videoSender ?? this.pc.getSenders().find((s) => s.track?.kind === 'video')
    if (sender) {
      this.videoSender = sender
      await sender.replaceTrack(track)
      return
    }
    const s = addSimulcastVideo(this.pc, track, stream)
    this.videoSender = s
    this.crypto?.protectSender(s)
    this.enqueue(async () => {
      const offer = await this.pc.createOffer()
      await this.pc.setLocalDescription(offer)
      this.send({ type: 'sfu-offer', sdp: offer.sdp! })
    })
  }

  async disableVideo() {
    const sender = this.videoSender ?? this.pc.getSenders().find((s) => s.track?.kind === 'video')
    if (sender) {
      this.videoSender = sender
      await sender.replaceTrack(null)
    }
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
    void tuneVideoSender(sender, SCREEN_MAX_BITRATE, 'maintain-resolution')
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

  /** Contadores da amostra anterior (os do WebRTC são cumulativos). */
  private cursors = new Map<string, QualityCursor>()

  async qos(): Promise<QosReport> {
    const report = await this.pc.getStats()
    // O `RTCStatsReport` é um Map-like; a extracção é pura e trabalha sobre um
    // array simples, o que a torna testável sem browser (ver callQuality.test.ts).
    const entries: StatEntry[] = []
    report.forEach((v) => entries.push(v as unknown as StatEntry))
    const sample = extractQuality(entries, this.cursors)
    return { ...sample, score: callQualityScore(sample) }
  }

  hangup() {
    // Terminal ANTES de fechar a PC: o `close()` dispara
    // `onconnectionstatechange`, e sem o estado já em `disconnected` a máquina
    // interpretaria a saída intencional como uma avaria e tentava recuperar.
    this.setState('disconnected')
    this.clearTimers()
    this.pc.close()
    this.crypto?.close()
    this.signal.close()
  }
}
