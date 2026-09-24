/**
 * Ler fontes no browser: o que um blob é (duração, dimensões, se tem imagem e
 * som), o áudio decodificado (com cache) e os picos da onda sonora.
 */
import type { Fonte, OrigemDaFonte, TipoDeFonte } from './projecto'
import { novoId } from './projecto'
import { picos } from './sinal'

export interface Sondagem {
  duracao: number
  largura: number | null
  altura: number | null
  tipo: TipoDeFonte
}

/**
 * Duração e dimensões pelo elemento de media. Um WebM do MediaRecorder chega
 * com duração `Infinity` até se procurar até ao fim — o mesmo truque que o
 * painel de edição já usava.
 */
export function sondar(blob: Blob): Promise<Sondagem> {
  return new Promise((resolve, reject) => {
    const url = URL.createObjectURL(blob)
    const v = document.createElement('video')
    v.preload = 'metadata'
    v.muted = true
    let feito = false
    const limpar = () => {
      v.removeAttribute('src')
      v.load()
      URL.revokeObjectURL(url)
    }
    const acabar = () => {
      if (feito) return
      const d = v.duration
      if (!Number.isFinite(d) || d <= 0) return
      feito = true
      const temImagem = v.videoWidth > 0
      // Com imagem, presume-se som; `criarFonte` confirma-o decodificando.
      const tipo: TipoDeFonte = temImagem ? 'av' : 'audio'
      const r = { duracao: d, largura: temImagem ? v.videoWidth : null, altura: temImagem ? v.videoHeight : null, tipo }
      limpar()
      resolve(r)
    }
    v.onloadedmetadata = () => {
      if (Number.isFinite(v.duration) && v.duration > 0) acabar()
      else v.currentTime = 1e7
    }
    v.ondurationchange = acabar
    v.onseeked = acabar
    v.onerror = () => {
      if (feito) return
      feito = true
      limpar()
      reject(new Error('media ilegível'))
    }
    v.src = url
  })
}

/**
 * Se o blob tem faixa de áudio. O `<video>` não diz de forma portável; a
 * decodificação diz — e o custo paga-se uma vez, porque o resultado fica em
 * cache para a onda sonora e para a mistura.
 */
export async function temAudio(blob: Blob): Promise<boolean> {
  try {
    const b = await audioDe(blob)
    return b.length > 0
  } catch {
    return false
  }
}

export async function criarFonte(blob: Blob, nome: string, origem: OrigemDaFonte, tipoConhecido?: TipoDeFonte): Promise<Fonte> {
  const s = await sondar(blob)
  let tipo = tipoConhecido ?? s.tipo
  if (!tipoConhecido && tipo === 'av' && !(await temAudio(blob))) tipo = 'video'
  return {
    id: novoId('f'),
    nome,
    tipo,
    origem,
    duracao: s.duracao,
    largura: s.largura,
    altura: s.altura,
    bytes: blob.size,
    criadaEm: Date.now(),
  }
}

// ---------------------------------------------------------------------------
//  Áudio decodificado, com cache
// ---------------------------------------------------------------------------

export const TAXA = 48_000

/** Poucas entradas: um AudioBuffer de uma aula de uma hora são centenas de MB. */
const cache = new Map<Blob, Promise<AudioBuffer>>()
const MAX_CACHE = 4

export function audioDe(blob: Blob): Promise<AudioBuffer> {
  let p = cache.get(blob)
  if (!p) {
    p = (async () => {
      const ctx = new OfflineAudioContext(2, 1, TAXA)
      return ctx.decodeAudioData(await blob.arrayBuffer())
    })()
    cache.set(blob, p)
    p.catch(() => cache.delete(blob))
    while (cache.size > MAX_CACHE) cache.delete(cache.keys().next().value as Blob)
  }
  return p
}

export function canaisDe(b: AudioBuffer): Float32Array[] {
  return Array.from({ length: b.numberOfChannels }, (_, i) => b.getChannelData(i))
}

/** Picos para a onda sonora: um valor por 1/`porSegundo` s da FONTE. */
export async function ondaDe(blob: Blob, porSegundo = 20): Promise<Float32Array> {
  const b = await audioDe(blob)
  return picos(canaisDe(b), Math.max(1, Math.round(b.duration * porSegundo)))
}
