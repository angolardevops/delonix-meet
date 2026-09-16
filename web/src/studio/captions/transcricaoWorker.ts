/**
 * Transcrição de um projecto, com TEMPOS POR PALAVRA — o que o corte pelo texto,
 * as palavras de preenchimento e o karaoke precisam.
 *
 * O `whisperWorker.ts` da sala transcreve AO VIVO e só quer texto; este lê uma
 * gravação inteira de uma vez. Mesmo modelo, mesma política: modelo e runtime
 * servidos pelo próprio host, `allowRemoteModels = false` — nada sai da rede.
 *
 * Tempos por palavra exigem que o modelo exportado traga as atenções cruzadas.
 * Se não trouxer, o transformers.js recusa, e aqui pede-se tempos por SEGMENTO
 * e marca-se o resultado como estimado — nunca se finge precisão que não há.
 */
import { env, pipeline } from '@xenova/transformers'

env.allowRemoteModels = false
env.localModelPath = '/models/'
env.backends.onnx.wasm.wasmPaths = '/ort/'
// Cache Storage (`transformers-cache`): o modelo descarrega-se do host uma vez.
env.useBrowserCache = true

interface Pedaco {
  text: string
  timestamp: [number, number | null]
}
type Asr = (audio: Float32Array, opts: object) => Promise<{ text: string; chunks?: Pedaco[] }>

let asr: Promise<Asr> | null = null

function carregar(): Promise<Asr> {
  asr ??= pipeline('automatic-speech-recognition', 'Xenova/whisper-tiny', {
    quantized: true,
    progress_callback: (p: { status: string; file?: string; progress?: number }) => {
      if (p.status === 'progress') postMessage({ op: 'modelo', ficheiro: p.file, pct: p.progress ?? 0 })
    },
  }) as unknown as Promise<Asr>
  asr.catch(() => {
    asr = null
  })
  return asr
}

onmessage = async (e: MessageEvent) => {
  const m = e.data as { op: 'transcrever'; pcm: Float32Array; lingua: string }
  if (m.op !== 'transcrever') return
  try {
    const run = await carregar()
    postMessage({ op: 'pronto' })
    const pedacos = Math.max(1, Math.ceil(m.pcm.length / 16000 / 25))
    let feitos = 0
    const base = {
      language: m.lingua,
      task: 'transcribe',
      chunk_length_s: 30,
      stride_length_s: 5,
      chunk_callback: () => {
        feitos++
        postMessage({ op: 'progresso', fraccao: Math.min(0.99, feitos / pedacos) })
      },
    }
    let estimadas = false
    let out: { text: string; chunks?: Pedaco[] }
    try {
      out = await run(m.pcm, { ...base, return_timestamps: 'word' })
    } catch (err) {
      if (!/cross attentions/i.test((err as Error).message)) throw err
      estimadas = true
      feitos = 0
      out = await run(m.pcm, { ...base, return_timestamps: true })
    }
    const pedacosOut = (out.chunks ?? []).map((c) => ({
      texto: c.text,
      inicio: c.timestamp[0] ?? 0,
      fim: c.timestamp[1] ?? c.timestamp[0] ?? 0,
    }))
    postMessage({ op: 'resultado', estimadas, pedacos: pedacosOut, texto: out.text })
  } catch (err) {
    postMessage({ op: 'erro', mensagem: (err as Error).message })
  }
}
