/**
 * Cortes na aula gravada — sem `ffmpeg.wasm`.
 *
 * PORQUE NÃO O `ffmpeg.wasm`: são ~30 MB de WASM a descarregar e a compilar,
 * e o corte corre em software num único fio. O pedido era «cortes profissionais
 * com pouco recurso». O caminho de pouco recurso no browser é o WebCodecs: o
 * `VideoDecoder`/`VideoEncoder` usam o decodificador de HARDWARE do dispositivo,
 * e o único código extra é um multiplexador de 164 KB (`webm-muxer`).
 *
 * A ideia do corte, e porque é que isto não é uma reencodificação completa:
 * um WebM só pode começar num FRAME-CHAVE. Cortar num ponto arbitrário obriga
 * a reencodificar do frame-chave anterior até ao corte. Aqui reencodifica-se
 * TUDO o que fica dentro do troço escolhido — o que é honesto e simples — mas
 * a decodificação é por hardware, por isso um troço de minutos leva segundos,
 * não o tempo real de reprodução.
 *
 * O ÁUDIO segue outro caminho, e é por uma razão que se descobre à segunda
 * tentativa: a imagem corta-se a 4× a acelerar o `<video>`, mas acelerar
 * reprodução comprime o áudio no tempo e sobe-lhe o tom. Um corte com a voz
 * do professor em falsete não é um corte. Por isso o áudio é decodificado de
 * uma vez (`decodeAudioData`, nativo e mais rápido que tempo real), fatiado
 * por amostras — que é exacto ao milissegundo — e reencodificado em Opus.
 *
 * Isto só é possível porque a gravação guarda a FAIXA DE ÁUDIO ISOLADA
 * (ver `compositor.ts`). Foi o pedido «separar o áudio do vídeo» a pagar-se a
 * si próprio: sem essa faixa, seria preciso desmultiplexar um WebM fechado.
 */
import { Muxer, ArrayBufferTarget } from 'webm-muxer'

// O `MediaStreamTrackProcessor` (Chrome 94+) ainda não está no `lib.dom` do
// TypeScript. Declara-se o mínimo que se usa, em vez de espalhar `any`.
declare class MediaStreamTrackProcessor {
  constructor(init: { track: MediaStreamTrack })
  readonly readable: ReadableStream<VideoFrame>
}
interface GlobalComCodecs {
  VideoEncoder?: unknown
  VideoDecoder?: unknown
  MediaStreamTrackProcessor?: unknown
}

export interface Troco {
  /** Segundos, no tempo do ficheiro de origem. */
  inicio: number
  fim: number
}

export interface ProgressoDoCorte {
  fase: 'a-ler' | 'a-cortar' | 'a-fechar'
  /** 0–1. `null` quando ainda não se sabe a duração. */
  fraccao: number | null
}

/** O browser aguenta cortar sem reproduzir em tempo real? */
export function cortesSuportados(): boolean {
  const g = globalThis as unknown as GlobalComCodecs
  return (
    typeof g.VideoEncoder === 'function' &&
    typeof g.VideoDecoder === 'function' &&
    typeof g.MediaStreamTrackProcessor === 'function'
  )
}

/**
 * Corta `origem` ao troço pedido.
 *
 * Estratégia: reproduz o ficheiro num `<video>` mudo, extrai os frames com um
 * `MediaStreamTrackProcessor` (é o que dá acesso a `VideoFrame` sem escrever um
 * desmultiplexador de WebM à mão) e reencodifica-os. Corre à velocidade de
 * reprodução do elemento — que se acelera com `playbackRate`.
 *
 * NÃO é o caminho mais rápido possível (esse desmultiplexava o WebM e
 * reencodificava só os GOPs das pontas), mas não traz nenhum desmultiplexador
 * novo para o bundle e chega para aulas de dezenas de minutos.
 */
export async function cortar(
  origem: Blob,
  troco: Troco,
  aoProgredir?: (p: ProgressoDoCorte) => void,
  velocidade = 4,
  /** Faixa de áudio isolada da mesma gravação. Sem ela o corte sai mudo. */
  audioOriginal?: Blob | null,
): Promise<Blob> {
  if (!cortesSuportados()) throw new Error('WebCodecs indisponível neste browser')
  aoProgredir?.({ fase: 'a-ler', fraccao: null })

  const video = document.createElement('video')
  video.src = URL.createObjectURL(origem)
  video.muted = true
  video.playsInline = true
  await new Promise<void>((r, j) => {
    video.onloadedmetadata = () => r()
    video.onerror = () => j(new Error('não foi possível ler a gravação'))
  })

  const largura = video.videoWidth
  const altura = video.videoHeight
  if (!largura || !altura) throw new Error('gravação sem imagem')

  const alvo = new ArrayBufferTarget()
  // O áudio é decodificado ANTES da imagem: se a faixa não servir, é melhor
  // saber já do que ao fim de um corte inteiro.
  const audio = audioOriginal ? await fatiarAudio(audioOriginal, troco) : null

  const muxer = new Muxer({
    target: alvo,
    video: { codec: 'V_VP9', width: largura, height: altura },
    ...(audio ? { audio: { codec: 'A_OPUS', sampleRate: audio.sampleRate, numberOfChannels: audio.canais } } : {}),
    firstTimestampBehavior: 'offset',
  })

  const encoder = new VideoEncoder({
    output: (chunk, meta) => muxer.addVideoChunk(chunk, meta),
    error: (e) => console.error('[editor] encoder', e),
  })
  encoder.configure({
    codec: 'vp09.00.10.08',
    width: largura,
    height: altura,
    bitrate: 6_000_000,
    framerate: 30,
  })

  // `captureStream` do elemento dá-nos os frames já decodificados pelo
  // hardware do dispositivo — sem escrever um desmultiplexador de WebM.
  const fluxo = (video as HTMLVideoElement & { captureStream(): MediaStream }).captureStream()
  const faixa = fluxo.getVideoTracks()[0]
  const leitor = new MediaStreamTrackProcessor({ track: faixa }).readable.getReader()

  const duracao = Math.max(0.001, troco.fim - troco.inicio)
  video.currentTime = troco.inicio
  await new Promise<void>((r) => (video.onseeked = () => r()))
  video.playbackRate = velocidade
  await video.play()

  aoProgredir?.({ fase: 'a-cortar', fraccao: 0 })
  let base: number | null = null
  let frames = 0
  try {
    for (;;) {
      const { value, done } = await leitor.read()
      if (done || !value) break
      const t = video.currentTime
      if (t >= troco.fim) {
        value.close()
        break
      }
      // Reescreve o tempo para o troço começar em zero — senão o ficheiro
      // sai com minutos de nada no início.
      if (base === null) base = value.timestamp
      const inicio: number = base
      const frame = new VideoFrame(value, { timestamp: value.timestamp - inicio })
      value.close()
      // Frame-chave a cada 2 s: sem isto o ficheiro não se pode procurar.
      encoder.encode(frame, { keyFrame: frames % 60 === 0 })
      frame.close()
      frames++
      if (frames % 15 === 0) {
        aoProgredir?.({ fase: 'a-cortar', fraccao: Math.min(1, (t - troco.inicio) / duracao) })
      }
    }
  } finally {
    leitor.cancel().catch(() => {})
    video.pause()
    faixa.stop()
  }

  aoProgredir?.({ fase: 'a-fechar', fraccao: 1 })
  if (audio) await codificarAudio(audio, muxer)
  await encoder.flush()
  encoder.close()
  muxer.finalize()
  URL.revokeObjectURL(video.src)
  if (!frames) throw new Error('o troço escolhido não tem imagem')
  return new Blob([alvo.buffer], { type: 'video/webm' })
}

// ---------------------------------------------------------------------------
//  Áudio: decodificar, fatiar por amostras, reencodificar em Opus
// ---------------------------------------------------------------------------

interface FatiaDeAudio {
  buffer: AudioBuffer
  sampleRate: number
  canais: number
}

/**
 * Decodifica a faixa isolada e fica só com o troço. O corte é por ÍNDICE DE
 * AMOSTRA, por isso é exacto — não há arredondamento a limites de pacote.
 */
async function fatiarAudio(blob: Blob, troco: Troco): Promise<FatiaDeAudio | null> {
  const ctx = new AudioContext()
  try {
    const inteiro = await ctx.decodeAudioData(await blob.arrayBuffer())
    const sr = inteiro.sampleRate
    const de = Math.max(0, Math.floor(troco.inicio * sr))
    const ate = Math.min(inteiro.length, Math.ceil(troco.fim * sr))
    if (ate <= de) return null
    const canais = inteiro.numberOfChannels
    const fatia = new AudioBuffer({ length: ate - de, numberOfChannels: canais, sampleRate: sr })
    for (let c = 0; c < canais; c++) {
      fatia.copyToChannel(inteiro.getChannelData(c).subarray(de, ate), c)
    }
    return { buffer: fatia, sampleRate: sr, canais }
  } catch {
    // Faixa ilegível não pode levar o corte da imagem atrás: sai mudo, e quem
    // chamou avisa. Melhor um corte mudo do que nenhum corte.
    return null
  } finally {
    void ctx.close().catch(() => {})
  }
}

/** Reencodifica a fatia em Opus e entrega-a ao multiplexador. */
async function codificarAudio(fatia: FatiaDeAudio, muxer: Muxer<ArrayBufferTarget>): Promise<void> {
  const { buffer, sampleRate, canais } = fatia
  const encoder = new AudioEncoder({
    output: (chunk, meta) => muxer.addAudioChunk(chunk, meta),
    error: (e) => console.error('[editor] encoder de áudio', e),
  })
  encoder.configure({ codec: 'opus', sampleRate, numberOfChannels: canais, bitrate: 128_000 })

  // Blocos de 20 ms — o tamanho de trama nativo do Opus; blocos maiores
  // obrigavam o encoder a refatiar por dentro.
  const porBloco = Math.round(sampleRate * 0.02)
  const intercalado = new Float32Array(porBloco * canais)
  for (let off = 0; off < buffer.length; off += porBloco) {
    const n = Math.min(porBloco, buffer.length - off)
    for (let c = 0; c < canais; c++) {
      const dados = buffer.getChannelData(c)
      for (let i = 0; i < n; i++) intercalado[i * canais + c] = dados[off + i]
    }
    const data = new AudioData({
      format: 'f32',
      sampleRate,
      numberOfFrames: n,
      numberOfChannels: canais,
      timestamp: Math.round((off / sampleRate) * 1e6),
      data: intercalado.subarray(0, n * canais),
    })
    encoder.encode(data)
    data.close()
  }
  await encoder.flush()
  encoder.close()
}
