// Arnês da cadeia de voz — a página que o `voz-natural.mjs` conduz.
//
// Monta, num `OfflineAudioContext` a 48 kHz, as cadeias a comparar e passa por
// TODAS o mesmo sinal, na mesma renderização:
//
//   · a NOVA é a do produto — `loadVoiceChain` e `buildVoiceChain` de
//     `src/voiceChain.ts`, com os worklets que o Vite serve (o WASM do RNNoise
//     e o `noiseGateWorklet.js`). Não é uma cópia;
//   · a ANTIGA é a que a `develop` tinha em e55a5f87, reconstruída aqui com os
//     números de então e o gate congelado (`voz-natural-gate-antigo.js`).
//
// Um só RNNoise alimenta todas as cadeias: o que as distingue é o que vem
// DEPOIS dele, e assim nenhuma diferença pode vir de dois estados do RNNoise.
import { audioConstraints, Denoiser, withNativeNoiseSuppression } from '../src/media'
import { buildVoiceChain, createVoiceGate, loadVoiceChain, VOICE_GATE, type VoiceGateParams } from '../src/voiceChain'
import gateAntigoUrl from './voz-natural-gate-antigo.js?url'

const TAXA = 48_000

/** O que o `.mjs` pede: um nome e, para as variantes da nova, os parâmetros que mudam. */
export interface PedidoDeCadeia {
  nome: string
  tipo: 'entrada' | 'rnnoise' | 'antiga' | 'antiga-sem-dinamica' | 'nova'
  gate?: Partial<VoiceGateParams>
}

interface Resultado {
  cadeias: string[]
  /** PCM float32 de cada cadeia, em base64 (o CDP não transporta binário). */
  pcm: Record<string, string>
  /** Os parâmetros com que a NOVA correu — os do produto. */
  gateDoProduto: VoiceGateParams
  /** Bytes do WASM do RNNoise que a página recebeu, e se é um módulo válido. */
  wasm: { bytes: number; valido: boolean }
  crossOriginIsolated: boolean
}

/** O que o recuo do produto fez a um microfone (falso) aberto sem a supressão nativa. */
interface Recuo {
  abertoSemSupressao: boolean
  /** Pedir o estado que já tem devolve a MESMA pista — no caminho normal nada se reabre. */
  semMudancaEAMesma: boolean
  /** Pedir a supressão de volta: como ficou a pista devolvida. */
  supressao: boolean | undefined
  eco: boolean | undefined
  ganho: boolean | undefined
  reabriu: boolean
  antigaParada: boolean
  viva: boolean
  mudoPreservado: boolean
  /** Os factos do browser em que o recuo assenta, medidos aqui e não lembrados. */
  factos: { idealComPistaViva: boolean | undefined; exactComPistaViva: string }
}

/** O `Denoiser` do produto, em tempo real, sobre o microfone falso. */
interface AoVivo {
  activo: boolean
  estado: string
  amostras: number
  taxa: number
  pico: number
  /** Depois do `stop()`: o microfone continua nas mãos de quem o abriu. */
  microfoneVivo: boolean
}

// O TS do repo ainda não traz este construtor do Chromium nos tipos do DOM.
declare const MediaStreamTrackProcessor: new (init: { track: MediaStreamTrack }) => {
  readable: ReadableStream<{ numberOfFrames: number; sampleRate: number; copyTo(d: Float32Array, o: { planeIndex: number; format: string }): void; close(): void }>
}

declare global {
  interface Window {
    __voz: {
      pronto: boolean
      erro: string | null
      render: (sinalB64: string, cadeias: PedidoDeCadeia[], esperaMs: number) => Promise<Resultado>
      recuo: () => Promise<Recuo>
      aoVivo: (segundos: number) => Promise<AoVivo>
    }
  }
}

function deBase64(b64: string): Float32Array {
  const bin = atob(b64)
  const bytes = new Uint8Array(bin.length)
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i)
  return new Float32Array(bytes.buffer)
}

function paraBase64(f: Float32Array): string {
  const bytes = new Uint8Array(f.buffer, f.byteOffset, f.byteLength)
  let s = ''
  const PASSO = 0x8000
  for (let i = 0; i < bytes.length; i += PASSO) s += String.fromCharCode(...bytes.subarray(i, i + PASSO))
  return btoa(s)
}

async function render(sinalB64: string, cadeias: PedidoDeCadeia[], esperaMs: number): Promise<Resultado> {
  const sinal = deBase64(sinalB64)
  const ctx = new OfflineAudioContext(cadeias.length, sinal.length, TAXA)
  const modulos = await loadVoiceChain(ctx)
  await ctx.audioWorklet.addModule(gateAntigoUrl)

  const buffer = ctx.createBuffer(1, sinal.length, TAXA)
  buffer.copyToChannel(sinal as Float32Array<ArrayBuffer>, 0)
  const fonte = ctx.createBufferSource()
  fonte.buffer = buffer

  // O RNNoise partilhado. Sai de `buildVoiceChain` com um gate que NÃO se
  // usa (fica sem destino): o que interessa é o nó de entrada, que é o RNNoise.
  const base = buildVoiceChain(ctx, modulos)
  const rnnoise = base.input
  fonte.connect(rnnoise)

  const juntar = ctx.createChannelMerger(cadeias.length)
  juntar.connect(ctx.destination)

  cadeias.forEach((c, canal) => {
    let saida: AudioNode
    if (c.tipo === 'entrada') {
      saida = fonte
    } else if (c.tipo === 'rnnoise') {
      saida = rnnoise
    } else if (c.tipo === 'antiga' || c.tipo === 'antiga-sem-dinamica') {
      // A cadeia de e55a5f87 (`media.ts`, classe `Denoiser`), número a número.
      const gate = new AudioWorkletNode(ctx, 'noise-gate-processor-antigo')
      rnnoise.connect(gate)
      saida = gate
      if (c.tipo === 'antiga') {
        const nivelador = ctx.createDynamicsCompressor()
        nivelador.threshold.value = -24
        nivelador.knee.value = 12
        nivelador.ratio.value = 3
        nivelador.attack.value = 0.01
        nivelador.release.value = 0.25
        const limitador = ctx.createDynamicsCompressor()
        limitador.threshold.value = -3
        limitador.knee.value = 0
        limitador.ratio.value = 20
        limitador.attack.value = 0.003
        limitador.release.value = 0.1
        gate.connect(nivelador).connect(limitador)
        saida = limitador
      }
    } else {
      // A do produto. Sem `gate` é exactamente a que vai para a chamada.
      const gate = c.gate ? createVoiceGate(ctx, { ...VOICE_GATE, ...c.gate }) : base.output
      if (c.gate) rnnoise.connect(gate)
      saida = gate
    }
    saida.connect(juntar, 0, canal)
  })

  // O WASM do RNNoise carrega-se DENTRO do worklet, de forma assíncrona, e
  // enquanto não acaba o nó devolve silêncio. Num contexto offline a
  // renderização não espera por ele — por isso dá-se-lhe tempo antes de
  // arrancar, e o `.mjs` confirma depois que o primeiro bloco não saiu mudo.
  await new Promise((r) => setTimeout(r, esperaMs))
  fonte.start(0)
  const rendido = await ctx.startRendering()

  const pcm: Record<string, string> = {}
  cadeias.forEach((c, canal) => {
    pcm[c.nome] = paraBase64(rendido.getChannelData(canal))
  })
  return {
    cadeias: cadeias.map((c) => c.nome),
    pcm,
    gateDoProduto: VOICE_GATE,
    wasm: { bytes: modulos.wasmBinary.byteLength, valido: WebAssembly.validate(modulos.wasmBinary) },
    crossOriginIsolated: self.crossOriginIsolated,
  }
}

/**
 * O RECUO, com o código do produto: um microfone aberto como a cadeia de voz
 * o pede (sem a supressão nativa) tem de poder voltar a tê-la — é o que
 * acontece quando o RNNoise não carrega. Corre sobre o microfone falso do
 * Chromium; o que se prova é o `withNativeNoiseSuppression` de `src/media.ts`.
 */
async function recuo(): Promise<Recuo> {
  // Primeiro os factos: com uma pista do microfone VIVA (sem supressão), o que
  // devolve um pedido novo com a supressão ligada — «ideal» e `exact`?
  const viva = (await navigator.mediaDevices.getUserMedia({ audio: audioConstraints(undefined, false) })).getAudioTracks()[0]
  const id = viva.getSettings().deviceId
  const outra = (await navigator.mediaDevices.getUserMedia({ audio: audioConstraints(id, true) })).getAudioTracks()[0]
  const idealComPistaViva = outra.getSettings().noiseSuppression
  outra.stop()
  let exactComPistaViva = 'aceite'
  try {
    const e = await navigator.mediaDevices.getUserMedia({ audio: { ...audioConstraints(id, true), noiseSuppression: { exact: true } } })
    e.getTracks().forEach((t) => t.stop())
  } catch (e) {
    exactComPistaViva = e instanceof Error ? e.name : String(e)
  }
  viva.stop()

  const s = await navigator.mediaDevices.getUserMedia({ audio: audioConstraints(undefined, false) })
  const crua = s.getAudioTracks()[0]
  crua.enabled = false // o mute escolhido tem de sobreviver à reabertura
  const abertoSemSupressao = crua.getSettings().noiseSuppression === false
  const mesma = await withNativeNoiseSuppression(crua, false)
  const reposta = await withNativeNoiseSuppression(crua, true)
  const st = reposta.getSettings()
  const r: Recuo = {
    abertoSemSupressao,
    semMudancaEAMesma: mesma === crua,
    supressao: st.noiseSuppression,
    eco: st.echoCancellation,
    ganho: st.autoGainControl,
    reabriu: reposta !== crua,
    antigaParada: reposta === crua || crua.readyState === 'ended',
    viva: reposta.readyState === 'live',
    mudoPreservado: reposta.enabled === false,
    factos: { idealComPistaViva, exactComPistaViva },
  }
  reposta.stop()
  return r
}

/**
 * A classe `Denoiser` do produto — `AudioContext` a sério, fonte e destino de
 * `MediaStream` — sobre o microfone falso: monta-se, a pista que devolve leva
 * amostras em tempo real, e o `stop()` larga tudo. O quadro mede o GRAFO
 * offline; isto prova que a classe que a sala usa o monta e ele anda.
 */
async function aoVivo(segundos: number): Promise<AoVivo> {
  const s = await navigator.mediaDevices.getUserMedia({ audio: audioConstraints(undefined, false) })
  const crua = s.getAudioTracks()[0]
  const d = new Denoiser()
  const limpa = await d.process(crua)
  const activo = d.active
  const leitor = new MediaStreamTrackProcessor({ track: limpa }).readable.getReader()
  let amostras = 0
  let taxa = 0
  let pico = 0
  const ate = performance.now() + segundos * 1000
  while (performance.now() < ate) {
    const { value: f, done } = await leitor.read()
    if (done || !f) break
    taxa = f.sampleRate
    const x = new Float32Array(f.numberOfFrames)
    f.copyTo(x, { planeIndex: 0, format: 'f32-planar' })
    for (let i = 0; i < x.length; i++) pico = Math.max(pico, Math.abs(x[i]))
    amostras += x.length
    f.close()
  }
  const estado = limpa.readyState
  d.stop()
  const r: AoVivo = { activo, estado, amostras, taxa, pico, microfoneVivo: crua.readyState === 'live' }
  crua.stop()
  return r
}

window.__voz = { pronto: true, erro: null, render, recuo, aoVivo }
const log = document.getElementById('log')
if (log) log.textContent = 'pronto'
