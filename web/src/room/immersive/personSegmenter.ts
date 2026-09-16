// Recorte pessoa/fundo para o palco imersivo — o MESMO segmentador do
// `BackgroundEffect` (MediaPipe Selfie Segmentation, `/models/selfie_segmenter.tflite`,
// GPU com fallback para CPU), aqui aplicado ao vídeo REMOTO que se está a ver.
//
// Não se reutiliza a classe `BackgroundEffect` porque ela produz uma TRACK
// composta a partir de uma track: desfoca e compõe o frame inteiro em canvas 2D
// a cada frame, e só no fim expõe a pessoa. Aqui só a máscara interessa — a
// composição é feita na GPU, com paralaxe — e pagar a do `BackgroundEffect`
// por cima era desperdício puro.

import { confidenceToAlpha } from './parallax'

export class PersonSegmenter {
  usingGpu = false
  /** Tempo da última segmentação, ms. */
  lastMs = 0
  masks = 0
  private seg: import('@mediapipe/tasks-vision').ImageSegmenter | null = null
  private lastTs = 0
  private prev: Uint8Array | null = null
  private out: Uint8Array | null = null

  async init(): Promise<void> {
    const { ImageSegmenter, FilesetResolver } = await import('@mediapipe/tasks-vision')
    const fileset = await FilesetResolver.forVisionTasks('/mediapipe-wasm')
    const make = (delegate: 'GPU' | 'CPU') =>
      ImageSegmenter.createFromOptions(fileset, {
        baseOptions: { modelAssetPath: '/models/selfie_segmenter.tflite', delegate },
        runningMode: 'VIDEO',
        outputConfidenceMasks: true,
        outputCategoryMask: false,
      })
    this.seg = await make('GPU')
      .then((s) => {
        this.usingGpu = true
        return s
      })
      .catch(() => make('CPU'))
  }

  /**
   * Segmenta o frame actual. Síncrono em modo VIDEO; devolve a máscara (0–255)
   * ou `null` se o frame falhou. Os timestamps têm de subir estritamente.
   */
  segment(video: HTMLVideoElement): { data: Uint8Array; w: number; h: number } | null {
    if (!this.seg || video.readyState < 2) return null
    const ts = Math.max(this.lastTs + 1, performance.now())
    this.lastTs = ts
    const t0 = performance.now()
    let res: { data: Uint8Array; w: number; h: number } | null = null
    try {
      this.seg.segmentForVideo(video, ts, (result) => {
        const mask = result.confidenceMasks?.[0]
        if (!mask) return
        const w = mask.width
        const h = mask.height
        if (!this.out || this.out.length !== w * h) {
          this.out = new Uint8Array(w * h)
          this.prev = null
        }
        const next = confidenceToAlpha(mask.getAsFloat32Array(), new Uint8Array(w * h), this.prev)
        this.prev = next
        this.out = next
        res = { data: next, w, h }
        mask.close()
      })
    } catch {
      return null
    }
    this.lastMs = performance.now() - t0
    if (res) this.masks++
    return res
  }

  close(): void {
    this.seg?.close()
    this.seg = null
    this.prev = null
    this.out = null
  }
}
