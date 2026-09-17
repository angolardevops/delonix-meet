// Recorte pessoa/fundo para o palco imersivo — o MESMO segmentador do
// `BackgroundEffect` (MediaPipe Selfie Segmentation, `/models/selfie_segmenter.tflite`),
// aqui aplicado ao vídeo REMOTO que se está a ver.
//
// Porque não se reutiliza a classe `BackgroundEffect`: ela produz uma TRACK
// composta — desfoca e compõe o frame inteiro em canvas 2D — e só no fim expõe a
// pessoa. Aqui só a máscara interessa.
//
// Porque é que a máscara NÃO sai da GPU (medido a 2026-09-16, Radeon 610M, máquina
// carregada): o MediaPipe devolve a máscara à resolução do vídeo (1280×720 = 921 600
// floats), e ler isso para JavaScript com `getAsFloat32Array()` custava 17 ms de
// p50 só na leitura, mais o ciclo que a converte em alfa — o palco desligava-se
// com 104 ms por frame. Com o segmentador a desenhar no MESMO contexto WebGL do
// palco (`canvas` nas opções) e a máscara usada como textura, o `segmentForVideo`
// custa 3,4 ms de p50. Só o delegate CPU, que não tem textura, lê para JS.

export type MaskSink =
  | { kind: 'texture'; texture: WebGLTexture; width: number; height: number }
  | { kind: 'array'; data: Float32Array; width: number; height: number }

export class PersonSegmenter {
  usingGpu = false
  /** Tempo da última segmentação (inclui a cópia da máscara), ms. */
  lastMs = 0
  masks = 0
  private seg: import('@mediapipe/tasks-vision').ImageSegmenter | null = null
  private lastTs = 0

  /** `canvas` tem de ser o do renderer: é o contexto onde a máscara vai viver. */
  async init(canvas: HTMLCanvasElement): Promise<void> {
    const { ImageSegmenter, FilesetResolver } = await import('@mediapipe/tasks-vision')
    const fileset = await FilesetResolver.forVisionTasks('/mediapipe-wasm')
    const make = (delegate: 'GPU' | 'CPU') =>
      ImageSegmenter.createFromOptions(fileset, {
        canvas: delegate === 'GPU' ? canvas : undefined,
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
   * Segmenta o frame actual e entrega a máscara a `sink` DENTRO da chamada — a
   * textura do MediaPipe só é válida até a máscara ser fechada. Os timestamps
   * têm de subir estritamente.
   */
  segment(video: HTMLVideoElement, sink: (m: MaskSink) => void): boolean {
    if (!this.seg || video.readyState < 2) return false
    const ts = Math.max(this.lastTs + 1, performance.now())
    this.lastTs = ts
    const t0 = performance.now()
    let ok = false
    try {
      this.seg.segmentForVideo(video, ts, (result) => {
        const mask = result.confidenceMasks?.[0]
        if (!mask) return
        try {
          if (this.usingGpu && mask.hasWebGLTexture()) {
            sink({ kind: 'texture', texture: mask.getAsWebGLTexture(), width: mask.width, height: mask.height })
          } else {
            sink({ kind: 'array', data: mask.getAsFloat32Array(), width: mask.width, height: mask.height })
          }
          ok = true
        } finally {
          mask.close()
        }
      })
    } catch {
      return false
    }
    this.lastMs = performance.now() - t0
    if (ok) this.masks++
    return ok
  }

  close(): void {
    this.seg?.close()
    this.seg = null
  }
}
