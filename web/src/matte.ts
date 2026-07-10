/**
 * Matting de vídeo com Robust Video Matting (RVM, MobileNetV3) via ONNX Runtime
 * Web. Produz um *alpha matte* contínuo (não uma máscara binária), captando
 * cabelo e bordas finas muito melhor que o segmentador leve do MediaPipe.
 *
 * - Corre na GPU do cliente (WebGPU) quando disponível; cai para WASM SIMD.
 * - Estado recorrente (r1..r4) realimentado a cada frame → estabilidade temporal.
 * - Modelo e runtime são self-hosted (nada sai da rede local).
 */
import * as ort from 'onnxruntime-web'

// Runtime WASM self-hosted em /ort-rvm (SEPARADO do /ort do Whisper, que usa
// outra versão do onnxruntime-web via transformers.js — não podem colidir).
// Sem threads fora de contexto cross-origin-isolated; o WebGPU não precisa.
ort.env.wasm.wasmPaths = '/ort-rvm/'
ort.env.wasm.numThreads = self.crossOriginIsolated ? Math.min(4, navigator.hardwareConcurrency || 2) : 1

const MODEL_URL = '/models/rvm/rvm_mobilenetv3_fp32.onnx'
/** Lado maior da entrada do modelo — equilíbrio qualidade/desempenho. */
const MAX_SIDE = 512

function roundTo(n: number, m: number): number {
  return Math.max(m, Math.round(n / m) * m)
}

export class RvmMatte {
  private session: ort.InferenceSession | null = null
  private rec: ort.Tensor[] = []
  private ratio!: ort.Tensor
  private mw = 0
  private mh = 0
  private src = document.createElement('canvas')
  private srcCtx = this.src.getContext('2d', { willReadFrequently: true })!
  private mask = document.createElement('canvas')
  private maskCtx = this.mask.getContext('2d')!
  private maskData: ImageData | null = null
  private nchw: Float32Array | null = null
  /** true enquanto uma inferência está em curso (o chamador deve saltar frames). */
  busy = false
  usingWebGpu = false

  /** Inicializa a sessão ONNX. Devolve false se o modelo/EP não carregar. */
  async init(): Promise<boolean> {
    try {
      const hasWebGpu = 'gpu' in navigator && !!(navigator as unknown as { gpu?: unknown }).gpu
      const executionProviders = hasWebGpu ? ['webgpu', 'wasm'] : ['wasm']
      this.session = await ort.InferenceSession.create(MODEL_URL, {
        executionProviders,
        graphOptimizationLevel: 'all',
      })
      this.usingWebGpu = hasWebGpu
      this.resetState()
      // Rácio de sub-amostragem interno do RVM (0.25–0.5 recomendado).
      this.ratio = new ort.Tensor('float32', new Float32Array([0.4]), [1])
      console.info(`[matte] RVM ativo em ${this.usingWebGpu ? 'WebGPU' : 'WASM'}`)
      return true
    } catch (e) {
      console.warn('[matte] RVM indisponível — fallback para MediaPipe', e)
      this.session = null
      return false
    }
  }

  private resetState() {
    // Estado recorrente inicial = tensores "zero" mínimos ([1,1,1,1]).
    this.rec = [0, 0, 0, 0].map(() => new ort.Tensor('float32', new Float32Array([0]), [1, 1, 1, 1]))
  }

  private setSize(vw: number, vh: number) {
    const scale = Math.min(1, MAX_SIDE / Math.max(vw, vh))
    this.mw = roundTo(vw * scale, 16)
    this.mh = roundTo(vh * scale, 16)
    this.src.width = this.mw
    this.src.height = this.mh
    this.mask.width = this.mw
    this.mask.height = this.mh
    this.maskData = this.maskCtx.createImageData(this.mw, this.mh)
    this.nchw = new Float32Array(3 * this.mw * this.mh)
    this.resetState() // resolução mudou → estado recorrente incompatível
  }

  /**
   * Corre o matting para o frame atual do vídeo. Devolve um canvas com o alpha
   * no canal A (à resolução do modelo), ou null se ocupado/indisponível.
   */
  async run(source: CanvasImageSource, vw: number, vh: number): Promise<HTMLCanvasElement | null> {
    if (!this.session || this.busy) return null
    if (!vw || !vh) return null
    if (this.mw === 0) this.setSize(vw, vh)
    this.busy = true
    try {
      const { mw, mh } = this
      this.srcCtx.drawImage(source, 0, 0, mw, mh)
      const img = this.srcCtx.getImageData(0, 0, mw, mh)
      const n = mw * mh
      const f = this.nchw!
      const d = img.data
      // RGBA (HWC) -> NCHW normalizado [0,1]
      for (let i = 0; i < n; i++) {
        f[i] = d[i * 4] / 255
        f[n + i] = d[i * 4 + 1] / 255
        f[2 * n + i] = d[i * 4 + 2] / 255
      }
      const srcT = new ort.Tensor('float32', f, [1, 3, mh, mw])
      const feeds: Record<string, ort.Tensor> = {
        src: srcT,
        r1i: this.rec[0],
        r2i: this.rec[1],
        r3i: this.rec[2],
        r4i: this.rec[3],
        downsample_ratio: this.ratio,
      }
      const out = await this.session.run(feeds)
      this.rec = [out.r1o, out.r2o, out.r3o, out.r4o]
      const pha = out.pha.data as Float32Array
      const md = this.maskData!
      for (let i = 0; i < n; i++) md.data[i * 4 + 3] = pha[i] * 255
      this.maskCtx.putImageData(md, 0, 0)
      return this.mask
    } catch (e) {
      console.warn('[matte] falha na inferência', e)
      return null
    } finally {
      this.busy = false
    }
  }

  dispose() {
    void this.session?.release()
    this.session = null
    this.rec = []
  }
}
