/**
 * Exportação de um projecto — o único sítio onde nasce um ficheiro.
 *
 * Imagem: cada quadro é desenhado num canvas (`desenho.ts`) a partir dos frames
 * exactos das fontes (`frames.ts`) e codificado com `VideoEncoder`, com o mesmo
 * perfil que o corte já escolhia (hardware primeiro, VP8 em software como
 * último degrau — ver `escolherPerfil` em `editor.ts`).
 *
 * Som: a mistura do projecto (`mistura.ts`) em Opus.
 *
 * Não é tempo real: o ritmo é o do decodificador e do codificador, e fica
 * medido (`fpsMedidos`) para a estimativa da exportação seguinte.
 */
import { ArrayBufferTarget, Muxer } from 'webm-muxer'
import { codificarAudio, escolherPerfil } from '../editor'
import { Desenhador } from './desenho'
import { abrirFornecedor } from './frames'
import { misturar, paraAudioBuffer, temAudioNoProjecto } from './mistura'
import type { Projecto } from './projecto'
import { duracaoDoProjecto } from './projecto'

export interface OpcoesDeRender {
  largura: number
  altura: number
  fps: number
  videoBps: number
  audioBps: number
  enquadramento: 'caber' | 'preencher'
  soAudio: boolean
  legendas: string | null
  marcaDeAgua: string | null
  destaque: string
  familia: string
  /** Janela da linha de tempo; omissa = projecto inteiro. */
  inicio?: number
  fim?: number
}

export interface ProgressoDeRender {
  fase: 'audio' | 'video' | 'a-fechar'
  fraccao: number
}

export interface Renderizado {
  blob: Blob
  duracao: number
  fpsMedidos: number | null
  lufs: number | null
}

export function exportacaoSuportada(soAudio: boolean): boolean {
  const g = globalThis as { AudioEncoder?: unknown; VideoEncoder?: unknown; OfflineAudioContext?: unknown }
  return typeof g.AudioEncoder === 'function' && typeof g.OfflineAudioContext === 'function' && (soAudio || typeof g.VideoEncoder === 'function')
}

export async function renderizar(
  p: Projecto,
  lerBlob: (fonteId: string) => Promise<Blob>,
  o: OpcoesDeRender,
  aoProgredir?: (pr: ProgressoDeRender) => void,
  sinal?: AbortSignal,
): Promise<Renderizado> {
  const abortar = () => {
    if (sinal?.aborted) throw new DOMException('abortado', 'AbortError')
  }
  const inicio = Math.max(0, o.inicio ?? 0)
  const fim = Math.min(o.fim ?? duracaoDoProjecto(p), duracaoDoProjecto(p))
  const duracao = fim - inicio
  if (duracao <= 0) throw new Error('projecto vazio')

  aoProgredir?.({ fase: 'audio', fraccao: 0 })
  const mistura = temAudioNoProjecto(p) ? await misturar(p, lerBlob, { inicio, fim }, sinal) : null
  abortar()
  const fatia = mistura ? { buffer: paraAudioBuffer(mistura), sampleRate: mistura.taxa, canais: mistura.canais.length } : null

  const alvo = new ArrayBufferTarget()

  if (o.soAudio) {
    if (!fatia) throw new Error('projecto sem som')
    const muxer = new Muxer({
      target: alvo,
      audio: { codec: 'A_OPUS', sampleRate: fatia.sampleRate, numberOfChannels: fatia.canais },
      firstTimestampBehavior: 'offset',
    })
    aoProgredir?.({ fase: 'a-fechar', fraccao: 1 })
    await codificarAudio(fatia, muxer, o.audioBps)
    muxer.finalize()
    return { blob: new Blob([alvo.buffer], { type: 'audio/webm' }), duracao, fpsMedidos: null, lufs: mistura?.lufsFinal ?? null }
  }

  // Dimensões pares: os codificadores recusam ímpares em 4:2:0.
  const W = o.largura - (o.largura % 2)
  const H = o.altura - (o.altura % 2)
  const perfil = await escolherPerfil(W, H, { fps: o.fps, bitrate: o.videoBps })
  const codecMatroska = perfil.codec.startsWith('vp09') ? 'V_VP9' : 'V_VP8'
  const muxer = new Muxer({
    target: alvo,
    video: { codec: codecMatroska, width: W, height: H, frameRate: o.fps },
    ...(fatia ? { audio: { codec: 'A_OPUS', sampleRate: fatia.sampleRate, numberOfChannels: fatia.canais } } : {}),
    firstTimestampBehavior: 'offset',
  })
  let erroDoEncoder: Error | null = null
  const encoder = new VideoEncoder({
    output: (chunk, meta) => muxer.addVideoChunk(chunk, meta),
    error: (e) => {
      erroDoEncoder = e instanceof Error ? e : new Error(String(e))
    },
  })
  encoder.configure(perfil)

  const canvas = new OffscreenCanvas(W, H)
  const ctx = canvas.getContext('2d', { alpha: false })
  if (!ctx) throw new Error('canvas 2D indisponível')
  const desenhador = new Desenhador(
    p,
    async (id) => abrirFornecedor(await lerBlob(id)),
    { largura: W, altura: H, fps: o.fps, enquadramento: o.enquadramento, legendas: o.legendas, marcaDeAgua: o.marcaDeAgua, destaque: o.destaque, familia: o.familia },
  )

  const total = Math.max(1, Math.round(duracao * o.fps))
  const chaveCada = o.fps * 2
  const t0 = performance.now()
  try {
    for (let n = 0; n < total; n++) {
      abortar()
      if (erroDoEncoder) throw erroDoEncoder
      await desenhador.quadro(ctx, inicio + n / o.fps)
      const frame = new VideoFrame(canvas, { timestamp: Math.round((n * 1e6) / o.fps), duration: Math.round(1e6 / o.fps) })
      encoder.encode(frame, { keyFrame: n % chaveCada === 0 })
      frame.close()
      // Contrapressão: sem isto, um codificador em software fica com centenas
      // de quadros em fila e a memória sobe até o separador morrer.
      while (encoder.encodeQueueSize > 6) await new Promise((r) => setTimeout(r, 4))
      if (n % 10 === 0) aoProgredir?.({ fase: 'video', fraccao: n / total })
    }
    aoProgredir?.({ fase: 'a-fechar', fraccao: 1 })
    await encoder.flush()
    if (erroDoEncoder) throw erroDoEncoder
    if (fatia) await codificarAudio(fatia, muxer, o.audioBps)
    muxer.finalize()
  } finally {
    desenhador.fechar()
    if (encoder.state !== 'closed') encoder.close()
  }
  const segundos = (performance.now() - t0) / 1000
  return {
    blob: new Blob([alvo.buffer], { type: 'video/webm' }),
    duracao,
    fpsMedidos: segundos > 0.5 ? total / segundos : null,
    lufs: mistura?.lufsFinal ?? null,
  }
}
