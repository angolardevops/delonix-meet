/**
 * Mistura do projecto: A1 (voz) e A2 (música/sala) num `OfflineAudioContext`,
 * mais rápido que tempo real, e depois sonoridade/normalização em JS.
 *
 * Cadeia: clipe (ganho, fade de transição) → barramento da faixa (fader) →
 * [redução de ruído RNNoise, só na voz] → mistura → alvo LUFS com limitador
 * OU normalização de pico.
 *
 * Limite honesto: um clipe com velocidade ≠ 1 muda também o TOM do som (o
 * `playbackRate` de um `AudioBufferSourceNode` não preserva a altura). A
 * pré-visualização preserva-a (`preservesPitch` do elemento); a exportação não.
 */
import type { Projecto } from './projecto'
import { duracaoDoClip, estadoDaFaixa, fonte } from './projecto'
import { audioDe, TAXA } from './midia'
import { dbParaGanho, ganhoParaAlvo, limitar, lufsIntegrado, normalizarPico, aplicarGanho } from './sinal'

export interface Misturado {
  canais: Float32Array[]
  taxa: number
  /** Sonoridade medida ANTES do ganho de alvo (para o painel). */
  lufsMedido: number
  lufsFinal: number
}

async function rnnoise(ctx: BaseAudioContext): Promise<AudioNode> {
  const [{ loadRnnoise, RnnoiseWorkletNode }, wasmUrl, wasmSimdUrl, workletUrl] = await Promise.all([
    import('@sapphi-red/web-noise-suppressor'),
    import('@sapphi-red/web-noise-suppressor/rnnoise.wasm?url').then((m) => m.default),
    import('@sapphi-red/web-noise-suppressor/rnnoise_simd.wasm?url').then((m) => m.default),
    import('@sapphi-red/web-noise-suppressor/rnnoiseWorklet.js?url').then((m) => m.default),
  ])
  const wasmBinary = await loadRnnoise({ url: wasmUrl, simdUrl: wasmSimdUrl })
  await ctx.audioWorklet.addModule(workletUrl)
  return new RnnoiseWorkletNode(ctx as AudioContext, { maxChannels: 2, wasmBinary })
}

export function temAudioNoProjecto(p: Projecto): boolean {
  return p.clips.some((c) => (c.faixa === 'A1' || c.faixa === 'A2') && c.congelado === null)
}

export async function misturar(
  p: Projecto,
  lerBlob: (fonteId: string) => Promise<Blob>,
  opcoes: { inicio?: number; fim?: number } = {},
  sinal?: AbortSignal,
): Promise<Misturado | null> {
  const inicio = opcoes.inicio ?? 0
  const fimProjecto = p.clips.reduce((a, c) => Math.max(a, c.inicio + duracaoDoClip(c)), 0)
  const fim = Math.min(opcoes.fim ?? fimProjecto, fimProjecto)
  if (fim - inicio <= 0) return null
  const clips = p.clips.filter((c) => (c.faixa === 'A1' || c.faixa === 'A2') && c.congelado === null)
  const comprimento = Math.ceil((fim - inicio) * TAXA)
  const ctx = new OfflineAudioContext(2, comprimento, TAXA)

  const barramentos = new Map<string, AudioNode>()
  for (const id of ['A1', 'A2'] as const) {
    const e = estadoDaFaixa(p, id)
    const g = ctx.createGain()
    g.gain.value = e.visivel ? dbParaGanho(e.ganhoDb) : 0
    let saida: AudioNode = g
    if (id === 'A1' && p.mistura.reduzirRuido && clips.some((c) => c.faixa === 'A1')) {
      try {
        const rn = await rnnoise(ctx)
        g.connect(rn)
        saida = rn
      } catch (e) {
        // Sem RNNoise (WASM bloqueado, browser sem worklets) a exportação segue
        // sem redução — melhor do que não exportar. Fica no registo.
        console.warn('[editor] redução de ruído indisponível', e)
      }
    }
    saida.connect(ctx.destination)
    barramentos.set(id, g)
  }

  for (const c of clips) {
    if (sinal?.aborted) throw new DOMException('abortado', 'AbortError')
    const f = fonte(p, c.fonteId)
    if (!f || f.tipo === 'video') continue
    const fimClip = c.inicio + duracaoDoClip(c)
    if (fimClip <= inicio || c.inicio >= fim) continue
    let buffer: AudioBuffer
    try {
      buffer = await audioDe(await lerBlob(c.fonteId))
    } catch {
      continue
    }
    const src = ctx.createBufferSource()
    src.buffer = buffer
    src.playbackRate.value = c.velocidade
    const g = ctx.createGain()
    const base = dbParaGanho(c.ganhoDb)
    g.gain.value = base
    // Início relativo à janela exportada.
    const quando = c.inicio - inicio
    const saltoLinha = Math.max(0, -quando)
    const offsetFonte = c.entrada + saltoLinha * c.velocidade
    const duracaoFonte = c.saida - offsetFonte
    if (duracaoFonte <= 0) continue
    if (c.transicao && saltoLinha < c.transicao.duracao) {
      g.gain.setValueAtTime(0, Math.max(0, quando))
      g.gain.linearRampToValueAtTime(base, Math.max(0, quando) + c.transicao.duracao - saltoLinha)
    }
    src.connect(g)
    g.connect(barramentos.get(c.faixa)!)
    src.start(Math.max(0, quando), offsetFonte, duracaoFonte)
  }

  const r = await ctx.startRendering()
  const canais = [r.getChannelData(0).slice(), r.getChannelData(1).slice()]
  const lufsMedido = lufsIntegrado(canais, TAXA)
  if (p.mistura.alvoLufs !== null) {
    aplicarGanho(canais, dbParaGanho(ganhoParaAlvo(lufsMedido, p.mistura.alvoLufs)))
    limitar(canais, TAXA, -1)
  } else if (p.mistura.normalizar) {
    normalizarPico(canais, -1)
  }
  return { canais, taxa: TAXA, lufsMedido, lufsFinal: lufsIntegrado(canais, TAXA) }
}

export function paraAudioBuffer(m: Misturado): AudioBuffer {
  const b = new AudioBuffer({ length: m.canais[0].length, numberOfChannels: m.canais.length, sampleRate: m.taxa })
  m.canais.forEach((c, i) => b.copyToChannel(c as Float32Array<ArrayBuffer>, i))
  return b
}
