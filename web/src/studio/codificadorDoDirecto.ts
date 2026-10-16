/**
 * O codificador ADAPTATIVO do directo — WebCodecs em vez do `MediaRecorder`.
 *
 * PORQUE EXISTE. O `MediaRecorder` não deixa mudar o débito a meio: para o
 * baixar é preciso recomeçá-lo, e um segundo cabeçalho Matroska no meio do
 * fluxo é lixo para o `ffmpeg -c:v copy` do servidor (R76). Resultado: numa
 * ligação que não dá vazão, o estúdio insistia no mesmo débito, a fila do
 * socket crescia, o atraso crescia e a memória com ele. O `VideoEncoder`
 * aceita `configure({ bitrate })` a quente — é a peça que falta para a emissão
 * se ajustar à rede em vez de afogar nela.
 *
 * O CONTRATO COM O SERVIDOR NÃO MUDA, e é por isso que este módulo pode
 * existir: continua a sair **Matroska com H.264 e Opus** pelo mesmo socket, e
 * o servidor continua a declarar `-f matroska` e a copiar o vídeo sem
 * reencodificar. O que muda é quem o escreve: o `webm-muxer` em modo
 * `matroska` (que aceita codecs fora do WebM) com `streaming: true`, em vez do
 * muxer interno do browser.
 *
 * TRÊS COISAS QUE SÓ AQUI SE PODEM FAZER, e que o `MediaRecorder` não dava:
 *
 * 1. **Débito a quente** (`mudarDebito`), com um frame-chave a seguir — sem
 *    ele o descodificador do outro lado arrastava artefactos do débito antigo.
 * 2. **Frames-chave a intervalo fixo** (2 s), que é o que as plataformas de
 *    directo pedem e o `MediaRecorder` decidia sozinho.
 * 3. **Descartar frames ANTES de codificar** quando o codificador não
 *    acompanha. Isto é seguro e é o oposto de saltar um pedaço já muxado: um
 *    frame que nunca entrou no fluxo não deixa buraco nenhum. É o «frames
 *    perdidos por atraso do codificador» que o OBS mostra.
 *
 * O QUE ESTE MÓDULO NÃO DECIDE: quando baixar e quanto. Isso é o
 * `proximoDebito` do `directo.ts`, que é puro e tem teste — aqui só se executa.
 */
import { Muxer, StreamTarget } from 'webm-muxer'

// O `MediaStreamTrackProcessor` (Chrome 94+) ainda não está no `lib.dom` do
// TypeScript. Declara-se o mínimo que se usa, como no `editor.ts`.
declare class MediaStreamTrackProcessor<T> {
  constructor(init: { track: MediaStreamTrack })
  readonly readable: ReadableStream<T>
}

interface GlobalComCodecs {
  VideoEncoder?: unknown
  AudioEncoder?: unknown
  MediaStreamTrackProcessor?: unknown
}

/** Matroska quer o H.264 em AVCC com o `avcC` no CodecPrivate — não Annex-B. */
const CODEC_VIDEO_MATROSKA = 'V_MPEG4/ISO/AVC'
const CODEC_AUDIO_MATROSKA = 'A_OPUS'

/** Intervalo entre frames-chave. As plataformas de directo pedem 2 s. */
export const INTERVALO_CHAVE_MS = 2000

/**
 * Quantos frames se deixam à espera no codificador antes de começar a
 * descartar. Dois é o suficiente para absorver um soluço; a partir daí o
 * codificador está a perder a corrida e guardar mais só aumenta o atraso.
 */
export const FILA_MAXIMA_DO_CODIFICADOR = 2

/** O browser sabe codificar H.264 ao vivo com débito adaptativo? */
export function codificadorAdaptativoSuportado(): boolean {
  const g = globalThis as unknown as GlobalComCodecs
  return (
    typeof g.VideoEncoder === 'function' &&
    typeof g.AudioEncoder === 'function' &&
    typeof g.MediaStreamTrackProcessor === 'function'
  )
}

/**
 * O nível H.264 que o tamanho e os fps exigem, em hexadecimal.
 *
 * Um nível baixo de mais faz o `configure` recusar (ou, pior, aceitar e
 * entregar imagem que a plataforma recusa). A conta é a dos macroblocos por
 * segundo da norma, arredondada para os degraus que interessam a este produto.
 */
export function nivelH264(largura: number, altura: number, fps: number): string {
  // MaxMBPS (macroblocos por segundo) de cada nível, da tabela A-1 da norma.
  // Os degraus são os que este produto atinge; o 3.2 e o 4.1 não acrescentam
  // nada entre os vizinhos para os tamanhos que o estúdio compõe.
  const mbs = Math.ceil(largura / 16) * Math.ceil(altura / 16) * fps
  if (mbs <= 108_000) return '1f' // 3.1 — 720p30
  if (mbs <= 245_760) return '28' // 4.0 — 1080p30
  if (mbs <= 522_240) return '2a' // 4.2 — 1080p60
  if (mbs <= 589_824) return '32' // 5.0
  if (mbs <= 983_040) return '33' // 5.1 — 2160p30
  return '34' // 5.2 — 2160p60
}

/**
 * Os candidatos de codec, do melhor ao mais compatível: High, Main,
 * Baseline-constrained. Todos cabem no FLV do RTMP; o High é o que dá melhor
 * imagem pelo mesmo débito.
 */
export function candidatosH264(largura: number, altura: number, fps: number): string[] {
  const n = nivelH264(largura, altura, fps)
  return [`avc1.6400${n}`, `avc1.4d40${n}`, `avc1.42e0${n}`]
}

export interface OpcoesDoCodificador {
  largura: number
  altura: number
  fps: number
  bitrate: number
  audioBitrate: number
}

/**
 * Pergunta ao browser em vez de assumir, como o `escolherPerfil` do editor —
 * e pela mesma razão: o `configure()` não falha por falta de hardware, cai
 * para software em silêncio. Numa emissão isso não é lentidão, é um estúdio a
 * ficar sem CPU no ar.
 */
export async function escolherConfigH264(o: OpcoesDoCodificador): Promise<VideoEncoderConfig | null> {
  const base = {
    width: o.largura,
    height: o.altura,
    framerate: o.fps,
    bitrate: o.bitrate,
    latencyMode: 'realtime' as const,
    // AVCC (o `format` por omissão), que é o que o Matroska quer: os chunks
    // vêm com `description` (o `avcC`) e o muxer escreve-o no CodecPrivate.
    avc: { format: 'avc' as const },
  }
  for (const codec of candidatosH264(o.largura, o.altura, o.fps)) {
    for (const hardwareAcceleration of ['prefer-hardware', undefined] as const) {
      const c: VideoEncoderConfig = { ...base, codec, ...(hardwareAcceleration ? { hardwareAcceleration } : {}) }
      try {
        const r = await VideoEncoder.isConfigSupported(c)
        if (r.supported) return r.config ?? c
      } catch {
        // `isConfigSupported` atira em configurações que não sabe descrever —
        // isso é «não», não é uma falha da emissão.
      }
    }
  }
  return null
}

/**
 * Uma emissão a codificar. Vive entre o fluxo composto e o socket do
 * `Directo`: recebe frames, entrega pedaços de Matroska.
 */
export class CodificadorDoDirecto {
  private muxer: Muxer<StreamTarget> | null = null
  private video: VideoEncoder | null = null
  private audio: AudioEncoder | null = null
  private config: VideoEncoderConfig | null = null
  private paragens: (() => void)[] = []
  private ultimaChave = 0
  private pedirChave = true
  private vivo = false
  /**
   * O instante ZERO da emissão, em microssegundos: o primeiro que aparecer em
   * QUALQUER das duas faixas.
   *
   * Tem de ser um só para as duas. Os tempos de um `MediaStreamTrackProcessor`
   * contam a idade do documento, não da emissão, e o `firstTimestampBehavior:
   * 'offset'` do muxer põe CADA faixa a zero pela primeira amostra DELA — ou
   * seja, se a imagem começar 300 ms antes do som, o som fica 300 ms adiantado
   * para sempre. Daí o `permissive` e esta subtracção feita aqui.
   */
  private base = -1

  /** Frames que nunca chegaram ao codificador porque ele não acompanhava. */
  framesDescartados = 0
  /** O débito que o codificador tem configurado agora. */
  debito = 0

  constructor(
    private readonly aoSair: (dados: Uint8Array) => void,
    /** Chamado quando o codificador morre a meio — a emissão tem de parar. */
    private readonly aoFalhar: (razao: string) => void,
  ) {}

  /**
   * Arranca. Devolve `false` quando o browser não aguenta a configuração — e
   * nesse caso quem chama volta ao `MediaRecorder`, SEM ter aberto o ar: uma
   * emissão não é sítio para descobrir que o codificador não existe.
   */
  async comecar(stream: MediaStream, o: OpcoesDoCodificador): Promise<boolean> {
    const faixaVideo = stream.getVideoTracks()[0]
    if (!faixaVideo) return false
    const config = await escolherConfigH264(o)
    if (!config) return false

    const faixaAudio = stream.getAudioTracks()[0]
    const defAudio = faixaAudio?.getSettings() ?? {}
    const canais = defAudio.channelCount ?? 2
    const taxa = defAudio.sampleRate ?? 48000

    this.muxer = new Muxer({
      target: new StreamTarget({ onData: (d) => this.aoSair(d) }),
      video: { codec: CODEC_VIDEO_MATROSKA, width: o.largura, height: o.altura, frameRate: o.fps },
      ...(faixaAudio ? { audio: { codec: CODEC_AUDIO_MATROSKA, numberOfChannels: canais, sampleRate: taxa } } : {}),
      // `streaming`: escreve só para a frente, que é o que um socket aceita.
      // `offset`: os tempos de um `MediaStreamTrackProcessor` são a idade do
      // documento, não zero — sem isto o primeiro pedaço nasce no futuro.
      streaming: true,
      type: 'matroska',
      // `permissive` porque o zero é NOSSO: ver `base`.
      firstTimestampBehavior: 'permissive',
    })

    this.config = config
    this.debito = config.bitrate ?? o.bitrate
    this.video = new VideoEncoder({
      output: (chunk, meta) => this.muxer?.addVideoChunk(chunk, meta, this.relativo(chunk.timestamp)),
      error: (e) => this.falhar(e),
    })
    this.video.configure(config)

    if (faixaAudio) {
      this.audio = new AudioEncoder({
        output: (chunk, meta) => this.muxer?.addAudioChunk(chunk, meta, this.relativo(chunk.timestamp)),
        error: (e) => this.falhar(e),
      })
      this.audio.configure({ codec: 'opus', sampleRate: taxa, numberOfChannels: canais, bitrate: o.audioBitrate })
    }

    this.vivo = true
    this.lerVideo(faixaVideo)
    if (faixaAudio) this.lerAudio(faixaAudio)
    return true
  }

  /** O tempo de uma amostra contado a partir do início da emissão. */
  private relativo(timestamp: number): number {
    if (this.base < 0) this.base = timestamp
    return Math.max(0, timestamp - this.base)
  }

  /**
   * Muda o débito a quente e pede um frame-chave logo a seguir: sem ele, o
   * descodificador do outro lado arrasta artefactos do débito antigo até ao
   * próximo, que pode estar a dois segundos.
   */
  mudarDebito(novo: number): void {
    if (!this.video || !this.config || novo === this.debito || this.video.state !== 'configured') return
    this.config = { ...this.config, bitrate: novo }
    try {
      this.video.configure(this.config)
      this.debito = novo
      this.pedirChave = true
    } catch {
      // Um browser que recuse reconfigurar mantém o débito que tinha: pior do
      // que adaptar, melhor do que morrer no ar.
    }
  }

  private falhar(e: unknown): void {
    if (!this.vivo) return
    this.vivo = false
    this.aoFalhar((e as Error)?.message || 'o codificador falhou')
  }

  private lerVideo(faixa: MediaStreamTrack): void {
    const leitor = new MediaStreamTrackProcessor<VideoFrame>({ track: faixa }).readable.getReader()
    this.paragens.push(() => void leitor.cancel().catch(() => undefined))
    const ciclo = async () => {
      while (this.vivo) {
        const { done, value } = await leitor.read()
        if (done || !value) break
        const enc = this.video
        if (!enc || enc.state !== 'configured') {
          value.close()
          break
        }
        // DESCARTAR ANTES DE CODIFICAR. Um frame que nunca entrou no fluxo não
        // deixa buraco; um pedaço já muxado que se salta parte o Matroska.
        if (enc.encodeQueueSize > FILA_MAXIMA_DO_CODIFICADOR) {
          this.framesDescartados++
          value.close()
          continue
        }
        const agora = value.timestamp / 1000
        const chave = this.pedirChave || agora - this.ultimaChave >= INTERVALO_CHAVE_MS
        if (chave) {
          this.ultimaChave = agora
          this.pedirChave = false
        }
        try {
          enc.encode(value, { keyFrame: chave })
        } catch (e) {
          this.falhar(e)
        }
        value.close()
      }
    }
    void ciclo().catch((e) => this.falhar(e))
  }

  private lerAudio(faixa: MediaStreamTrack): void {
    const leitor = new MediaStreamTrackProcessor<AudioData>({ track: faixa }).readable.getReader()
    this.paragens.push(() => void leitor.cancel().catch(() => undefined))
    const ciclo = async () => {
      while (this.vivo) {
        const { done, value } = await leitor.read()
        if (done || !value) break
        const enc = this.audio
        if (!enc || enc.state !== 'configured') {
          value.close()
          break
        }
        try {
          enc.encode(value)
        } catch (e) {
          this.falhar(e)
        }
        value.close()
      }
    }
    void ciclo().catch((e) => this.falhar(e))
  }

  /**
   * Fecha. Esvazia os codificadores ANTES de finalizar o muxer: o que ficou na
   * fila do codificador ainda é imagem da emissão, e perdê-la é cortar os
   * últimos segundos do que já estava gravado do outro lado.
   */
  async parar(): Promise<void> {
    this.vivo = false
    for (const p of this.paragens) p()
    this.paragens = []
    for (const enc of [this.video, this.audio]) {
      if (!enc || enc.state === 'closed') continue
      try {
        await enc.flush()
      } catch {
        /* um codificador já morto não tem nada para esvaziar */
      }
      try {
        enc.close()
      } catch {
        /* idem */
      }
    }
    this.video = null
    this.audio = null
    try {
      this.muxer?.finalize()
    } catch {
      /* um muxer em `streaming` já escreveu tudo o que podia escrever */
    }
    this.muxer = null
  }
}
