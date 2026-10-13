/**
 * A cadeia de voz da chamada: `microfone → RNNoise → gate de voz`.
 *
 * Vive à parte do `media.ts` por uma razão de MEDIÇÃO: o grafo constrói-se
 * sobre qualquer `BaseAudioContext`, e por isso o arnês `e2e/voz-natural.mjs`
 * monta ESTE código num `OfflineAudioContext` e mede-o contra a cadeia
 * anterior, com os worklets a sério (o WASM do RNNoise e o gate). O `Denoiser`
 * do `media.ts` só lhe põe o microfone à entrada e uma pista à saída.
 *
 * O que a cadeia já NÃO tem, e porquê (medido a 2026-10-13 pelo arnês, com a
 * cadeia anterior e a nova sobre o mesmo sinal — os números estão no cabeçalho
 * do `e2e/voz-natural.mjs`):
 *
 *  - o nivelador e o limitador (`DynamicsCompressorNode`). O compressor do
 *    Web Audio aplica ganho de compensação automático, que não se desliga: os
 *    dois em série SUBIAM tudo o que ficava abaixo do limiar — a cauda da sala
 *    e o eco que o cancelador deixa passar — e era isso o «eco» que se ouvia.
 *    Quem nivela é o `autoGainControl` do browser, que continua pedido;
 *  - a segunda supressão de ruído. Com o RNNoise activo pede-se ao browser
 *    `noiseSuppression: false` (ver `setNativeNoiseSuppression` no `media.ts`).
 */
import type { RnnoiseWorkletNode } from '@sapphi-red/web-noise-suppressor'

/** O nome com que o `noiseGateWorklet.js` se regista. */
export const GATE_PROCESSOR = 'noise-gate-processor'

export interface VoiceGateParams {
  /** Acima disto (dBFS, envolvente de 5 ms) o gate considera que há sinal. */
  thresholdDb: number
  /** Atenuação com o gate fechado (dB). NÃO é silêncio digital. */
  floorDb: number
  /** O gate fecha `hysteresisDb` abaixo do limiar a que abre. */
  hysteresisDb: number
  /** O sinal tem de se manter acima do limiar este tempo para o gate abrir. */
  sustainMs: number
  /** …e nesse instante não pode ter caído mais do que isto abaixo do seu pico. */
  transientDropDb: number
  /** Atraso do áudio face à detecção — o que deixa abrir ANTES do ataque. */
  lookaheadMs: number
  attackMs: number
  releaseMs: number
  holdMs: number
}

/**
 * Os valores do gate de voz, e a medição que escolheu cada um (2026-10-13,
 * `e2e/voz-natural.mjs`: a saída do RNNoise a sério, com três vozes — as
 * frases do `alsa-utils` e duas leituras corridas — e teclas a −35…−10 dBFS).
 *
 * `thresholdDb: -58` — BAIXO de propósito, e mais baixo do que os −50 da
 * porta anterior. Com o limiar a −46 o eco residual a −53 dBFS ficava no chão
 * (−25 dB), mas numa das vozes o gate comia mais de 3 dB ao início de 5 frases
 * em 9: uma consoante branda fica a −45 dBFS depois do RNNoise, mesmo em cima
 * do limiar. Contados os inícios das quatro corridas (36): a −50 o gate come
 * 9, a −58 come 2, a −64 come 1. Quem trava os cliques é a regra dos
 * transitórios, que não depende do limiar — por isso ele pode descer até
 * deixar a fala em paz. O preço: o eco residual já não é fechado pelo gate
 * (passa a 0 dB, sem ser amplificado), e é esse o lado para que se errou.
 *
 * `sustainMs: 25` + `transientDropDb: 6` — a rejeição de transitórios. Uma
 * tecla com corpo cai cerca de 1 dB por milissegundo depois do RNNoise.
 * Grelha medida (pior pico das teclas nas quatro corridas; fechado ≈ −35):
 * com 20 ms a regra só segura até 5 dB de queda e com 22 ms até 6 — ficava
 * tudo na aresta, e bastou mudar a voz que vinha antes para uma tecla a
 * −10 dBFS passar. Com 25 ms segura de 2 a 9 dB, e continua a segurar com o
 * limiar a −50 e a −64. Com 3 dB de queda, ou menos, o gate come mais um
 * início de frase (3 em 36 em vez de 2).
 *
 * `lookaheadMs: 30` — a sustentação mais 5 ms, que é o que o ataque de 3 ms
 * precisa para o gate já estar aberto quando o início da sílaba chega à
 * saída. Medido nos primeiros 30 ms de cada frase, face ao que o RNNoise
 * deixou: com 30 ms o gate tira 0,0 dB na mediana; com 0 ms tira 17 dB, em 9
 * frases de 9. É O CUSTO desta cadeia: 30 ms de atraso, contra os 12 ms dos
 * dois compressores que saíram.
 *
 * `floorDb: -24` — o ponto de partida do desenho, que a medição não
 * contrariou: cada 6 dB a mais no chão são 6 dB a menos no clique rejeitado
 * (tecla a −10 dBFS: −39 dBFS à saída com −24, −45 com −30) e 6 dB a mais no
 * que se perde quando o gate se engana. Não foi ouvido; fica o valor prudente.
 *
 * `hysteresisDb: 6`, `holdMs: 80`, `releaseMs: 150`, `attackMs: 3` — hold,
 * release e ataque são os da porta anterior: 150/250 ms de hold/release
 * deixavam passar mais 5 a 7 dB dos cliques logo a seguir à fala e não
 * mudavam nenhuma medida da voz.
 */
export const VOICE_GATE: VoiceGateParams = {
  thresholdDb: -58,
  floorDb: -24,
  hysteresisDb: 6,
  sustainMs: 25,
  transientDropDb: 6,
  lookaheadMs: 30,
  attackMs: 3,
  releaseMs: 150,
  holdMs: 80,
}

export interface VoiceChainModules {
  RnnoiseWorkletNode: typeof RnnoiseWorkletNode
  wasmBinary: ArrayBuffer
}

/**
 * Carrega o WASM do RNNoise e regista os dois worklets em `ctx`.
 *
 * Rejeita se qualquer um falhar — quem chama decide o recuo (a supressão
 * nativa do browser), e é por isso que isto não engole o erro.
 */
export async function loadVoiceChain(ctx: BaseAudioContext): Promise<VoiceChainModules> {
  const [{ loadRnnoise, RnnoiseWorkletNode: Node }, wasmUrl, wasmSimdUrl, workletUrl, gateWorkletUrl] = await Promise.all([
    import('@sapphi-red/web-noise-suppressor'),
    import('@sapphi-red/web-noise-suppressor/rnnoise.wasm?url').then((m) => m.default),
    import('@sapphi-red/web-noise-suppressor/rnnoise_simd.wasm?url').then((m) => m.default),
    import('@sapphi-red/web-noise-suppressor/rnnoiseWorklet.js?url').then((m) => m.default),
    import('./noiseGateWorklet.js?url').then((m) => m.default),
  ])
  const wasmBinary = await loadRnnoise({ url: wasmUrl, simdUrl: wasmSimdUrl })
  await Promise.all([ctx.audioWorklet.addModule(workletUrl), ctx.audioWorklet.addModule(gateWorkletUrl)])
  return { RnnoiseWorkletNode: Node, wasmBinary }
}

export interface VoiceChain {
  /** Liga-se aqui a fonte (mono). */
  input: AudioNode
  /** Daqui sai a voz tratada. */
  output: AudioNode
  destroy(): void
}

/** O nó do gate com os parâmetros dados (o atraso é fixo durante a vida do nó). */
export function createVoiceGate(ctx: BaseAudioContext, p: VoiceGateParams = VOICE_GATE): AudioWorkletNode {
  return new AudioWorkletNode(ctx, GATE_PROCESSOR, {
    processorOptions: { lookaheadMs: p.lookaheadMs },
    parameterData: {
      thresholdDb: p.thresholdDb,
      floorDb: p.floorDb,
      hysteresisDb: p.hysteresisDb,
      sustainMs: p.sustainMs,
      transientDropDb: p.transientDropDb,
      attackMs: p.attackMs,
      releaseMs: p.releaseMs,
      holdMs: p.holdMs,
    },
  })
}

/** Monta `RNNoise → gate` em `ctx`. Os módulos vêm do `loadVoiceChain(ctx)`. */
export function buildVoiceChain(ctx: BaseAudioContext, modules: VoiceChainModules, gate: VoiceGateParams = VOICE_GATE): VoiceChain {
  // O tipo da biblioteca pede um `AudioContext`, mas o nó é um
  // `AudioWorkletNode` comum e funciona num `OfflineAudioContext` (é como o
  // editor do Estúdio já o usa, em `studio/edit/mistura.ts`).
  const rnnoise = new modules.RnnoiseWorkletNode(ctx as AudioContext, { maxChannels: 1, wasmBinary: modules.wasmBinary })
  const gateNode = createVoiceGate(ctx, gate)
  rnnoise.connect(gateNode)
  return {
    input: rnnoise,
    output: gateNode,
    destroy() {
      try {
        rnnoise.destroy()
      } catch {
        /* já destruído */
      }
      rnnoise.disconnect()
      gateNode.disconnect()
    },
  }
}
