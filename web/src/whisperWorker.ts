/**
 * Worker de transcrição Whisper (WASM, 100% local) — fallback para browsers
 * sem Web Speech API (Firefox). Modelo e runtime servidos do próprio host:
 * nada sai da rede local.
 */
import { env, pipeline } from '@xenova/transformers'

env.allowRemoteModels = false
env.localModelPath = '/models/'
// Runtime ONNX (wasm) também self-hosted.
env.backends.onnx.wasm.wasmPaths = '/ort/'

type Asr = (audio: Float32Array, opts: object) => Promise<{ text: string }>
let asr: Promise<Asr> | null = null

function load(): Promise<Asr> {
  asr ??= pipeline('automatic-speech-recognition', 'Xenova/whisper-tiny', {
    quantized: true,
  }) as unknown as Promise<Asr>
  return asr
}

onmessage = async (e: MessageEvent) => {
  const m = e.data as { op: 'warmup' } | { op: 'chunk'; pcm: Float32Array; lang: string }
  try {
    if (m.op === 'warmup') {
      await load()
      postMessage({ op: 'ready' })
      return
    }
    const run = await load()
    const out = await run(m.pcm, {
      language: m.lang,
      task: 'transcribe',
      chunk_length_s: 30,
      // Sem timestamps: só o texto.
      return_timestamps: false,
    })
    const text = (out.text ?? '').trim()
    if (text) postMessage({ op: 'final', text })
  } catch (err) {
    postMessage({ op: 'error', message: (err as Error).message })
  }
}
