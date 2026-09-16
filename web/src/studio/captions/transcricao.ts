/**
 * Ponte para o worker de transcrição, e o que se sabe do modelo local.
 */
import type { Palavra } from '../edit/projecto'
import { distribuirPalavras } from './legendas'

export const CAMINHO_DO_MODELO = '/models/Xenova/whisper-tiny/config.json'

/**
 * O modelo está instalado NESTE servidor? Vem do build da imagem
 * (`deploy/fetch-whisper.sh`); num servidor de desenvolvimento sem ele, a
 * transcrição não arranca — e a interface diz isso antes de o utilizador
 * esperar por um erro.
 */
export async function modeloDisponivel(sinal?: AbortSignal): Promise<boolean> {
  try {
    const r = await fetch(CAMINHO_DO_MODELO, { method: 'HEAD', signal: sinal })
    const tipo = r.headers.get('content-type') ?? ''
    // O servidor de desenvolvimento responde 200 com o index.html a qualquer caminho.
    return r.ok && !tipo.includes('text/html')
  } catch {
    return false
  }
}

/** Bytes do modelo em Cache Storage (`transformers-cache`), e a quota do browser. */
export async function espacoDoModelo(): Promise<{ modelo: number; usado: number | null; quota: number | null }> {
  let modelo = 0
  try {
    if ('caches' in globalThis && (await caches.has('transformers-cache'))) {
      const c = await caches.open('transformers-cache')
      for (const pedido of await c.keys()) {
        const r = await c.match(pedido)
        const tam = Number(r?.headers.get('content-length'))
        modelo += Number.isFinite(tam) && tam > 0 ? tam : (await r?.clone().blob())?.size ?? 0
      }
    }
  } catch {
    // Cache Storage indisponível (contexto não seguro): conta-se zero.
  }
  let usado: number | null = null
  let quota: number | null = null
  try {
    const e = await navigator.storage?.estimate?.()
    usado = e?.usage ?? null
    quota = e?.quota ?? null
  } catch {
    /* sem StorageManager */
  }
  return { modelo, usado, quota }
}

export async function apagarCacheDoModelo(): Promise<void> {
  if ('caches' in globalThis) await caches.delete('transformers-cache')
}

/** Áudio a 16 kHz mono — o que o Whisper come. */
export async function paraDezasseisK(canais: Float32Array[], taxa: number): Promise<Float32Array> {
  const n = canais[0]?.length ?? 0
  if (!n) return new Float32Array(0)
  const comprimento = Math.ceil((n * 16000) / taxa)
  const ctx = new OfflineAudioContext(1, comprimento, 16000)
  const b = ctx.createBuffer(canais.length, n, taxa)
  canais.forEach((c, i) => b.copyToChannel(c as Float32Array<ArrayBuffer>, i))
  const src = ctx.createBufferSource()
  src.buffer = b
  src.connect(ctx.destination)
  src.start()
  const r = await ctx.startRendering()
  return r.getChannelData(0)
}

export interface ResultadoDaTranscricao {
  palavras: Palavra[]
  estimadas: boolean
}

export interface EventosDaTranscricao {
  aoModelo?: (pct: number) => void
  aoProgredir?: (fraccao: number) => void
}

/**
 * Transcreve `pcm` (16 kHz). Os tempos vêm relativos ao início do áudio dado;
 * `deslocamento` soma-se para ficarem em tempo da linha de tempo.
 */
export function transcrever(
  pcm: Float32Array,
  lingua: string,
  deslocamento: number,
  ev: EventosDaTranscricao = {},
  sinal?: AbortSignal,
): Promise<ResultadoDaTranscricao> {
  return new Promise((resolve, reject) => {
    const w = new Worker(new URL('./transcricaoWorker.ts', import.meta.url), { type: 'module' })
    const acabar = () => w.terminate()
    sinal?.addEventListener('abort', () => {
      acabar()
      reject(new DOMException('abortado', 'AbortError'))
    })
    w.onmessage = (e) => {
      const m = e.data as
        | { op: 'modelo'; pct: number }
        | { op: 'pronto' }
        | { op: 'progresso'; fraccao: number }
        | { op: 'resultado'; estimadas: boolean; pedacos: { texto: string; inicio: number; fim: number }[] }
        | { op: 'erro'; mensagem: string }
      if (m.op === 'modelo') ev.aoModelo?.(m.pct)
      else if (m.op === 'progresso') ev.aoProgredir?.(m.fraccao)
      else if (m.op === 'resultado') {
        acabar()
        const palavras = m.estimadas
          ? m.pedacos.flatMap((p) => distribuirPalavras(p.texto, p.inicio, p.fim))
          : m.pedacos.map((p) => ({ texto: p.texto.trim(), inicio: p.inicio, fim: Math.max(p.fim, p.inicio + 0.05) }))
        resolve({
          estimadas: m.estimadas,
          palavras: palavras
            .filter((p) => p.texto)
            .map((p) => ({ ...p, inicio: p.inicio + deslocamento, fim: p.fim + deslocamento })),
        })
      } else if (m.op === 'erro') {
        acabar()
        reject(new Error(m.mensagem))
      }
    }
    w.onerror = (e) => {
      acabar()
      reject(new Error(e.message || 'worker de transcrição falhou'))
    }
    const lingua2 = lingua.split('-')[0]
    w.postMessage({ op: 'transcrever', pcm, lingua: lingua2 }, [pcm.buffer])
  })
}
