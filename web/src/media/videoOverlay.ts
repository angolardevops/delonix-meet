// Um <canvas> desenhado por cima de um <video> que já existe na página, frame a
// frame, com o custo MEDIDO.
//
// Porque é que o canvas é posto ao lado do vídeo e não é um componente React no
// retrato: o retrato e o palco pertencem a outra parte da sala, que muda sem
// saber que isto existe. O canvas entra como irmão imediato do `<video>`, com
// `inset: 0` e o mesmo `object-fit`, e segue-lhe o `transform` (o zoom da
// apresentação). As etiquetas do retrato vêm depois no DOM e ficam por cima.
// Se o vídeo sair da página (o retrato remontou), o overlay avisa e sai.

import { budgetVerdict, fitCanvasSize, summarizeCost, type Budget, type BudgetVerdict, type FrameSample, type ObjectFit } from './sharpen'

export interface OverlayRenderer {
  /** Desenha o frame actual. Tem de ser barato; o custo é medido por fora. */
  render(video: HTMLVideoElement, width: number, height: number, now: number): void
  /** Espera pela GPU — só nos frames cronometrados. */
  finish(): void
  /** O contexto foi perdido (GPU reiniciou, separador descartado). */
  readonly lost: boolean
  destroy(): void
}

export interface OverlayStats {
  fps: number
  sourceFps: number
  p50Ms: number
  p95Ms: number
  width: number
  height: number
}

export type OverlayStop = 'budget' | 'lost' | 'detached'

export interface OverlayOptions {
  video: HTMLVideoElement
  renderer: OverlayRenderer
  canvas: HTMLCanvasElement
  budget: Budget
  maxPixels?: number
  /** Chamado com o trabalho extra por frame (ex.: o recorte) — entra na medida. */
  beforeRender?: (video: HTMLVideoElement, now: number, frame: number) => void
  onStats?: (s: OverlayStats) => void
  onStop?: (why: OverlayStop, last: OverlayStats | null, verdict: BudgetVerdict | null) => void
}

/** Um frame em cada N é cronometrado com `gl.finish()` — o resto não paga a espera. */
const TIME_EVERY = 10

type VideoWithRvfc = HTMLVideoElement & {
  requestVideoFrameCallback?: (cb: (now: number, meta: { presentedFrames: number }) => void) => number
  cancelVideoFrameCallback?: (h: number) => void
}

export class VideoOverlay {
  private running = false
  private handle = 0
  private usingRvfc = false
  private frame = 0
  private samples: FrameSample[] = []
  private lastSummaryAt = 0
  private overSince: number | null = null
  private lastStats: OverlayStats | null = null
  private sourceFrames = { at: 0, count: 0 }
  private sourceFps = 0
  private sizeDirty = true
  private lastSizeCheck = 0
  private ro: ResizeObserver | null = null
  private hidden = false

  constructor(private o: OverlayOptions) {}

  get stats(): OverlayStats | null {
    return this.lastStats
  }

  start(): void {
    const { video, canvas } = this.o
    canvas.classList.add('dx-enh-canvas')
    Object.assign(canvas.style, {
      position: 'absolute',
      inset: '0',
      width: '100%',
      height: '100%',
      pointerEvents: 'none',
      transformOrigin: 'center center',
    })
    video.after(canvas)
    this.ro = new ResizeObserver(() => {
      this.sizeDirty = true
    })
    this.ro.observe(video)
    this.running = true
    this.schedule()
  }

  /** Mostra o vídeo original por baixo (comparar) sem parar a medida. */
  setHidden(hidden: boolean): void {
    this.hidden = hidden
    this.o.canvas.style.visibility = hidden ? 'hidden' : 'visible'
  }

  stop(): void {
    if (!this.running) return
    this.running = false
    const v = this.o.video as VideoWithRvfc
    if (this.usingRvfc) v.cancelVideoFrameCallback?.(this.handle)
    else cancelAnimationFrame(this.handle)
    this.ro?.disconnect()
    this.ro = null
    this.o.canvas.remove()
    this.o.renderer.destroy()
  }

  private halt(why: OverlayStop, verdict: BudgetVerdict | null = null) {
    const last = this.lastStats
    this.stop()
    this.o.onStop?.(why, last, verdict)
  }

  private schedule() {
    if (!this.running) return
    const v = this.o.video as VideoWithRvfc
    if (v.requestVideoFrameCallback) {
      this.usingRvfc = true
      this.handle = v.requestVideoFrameCallback((now, meta) => this.tick(now, meta.presentedFrames))
    } else {
      this.usingRvfc = false
      this.handle = requestAnimationFrame((now) => this.tick(now, null))
    }
  }

  private tick(now: number, presentedFrames: number | null) {
    if (!this.running) return
    const { video, canvas, renderer } = this.o
    if (!video.isConnected || !canvas.isConnected) return this.halt('detached')
    if (renderer.lost) return this.halt('lost')

    // Ritmo da FONTE: o que o descodificador entrega, para saber se o efeito o acompanha.
    if (presentedFrames != null) {
      if (this.sourceFrames.at === 0) this.sourceFrames = { at: now, count: presentedFrames }
      else if (now - this.sourceFrames.at >= 1000) {
        this.sourceFps = Math.round(((presentedFrames - this.sourceFrames.count) * 1000) / (now - this.sourceFrames.at))
        this.sourceFrames = { at: now, count: presentedFrames }
      }
    }

    const visible = video.readyState >= 2 && video.videoWidth > 0 && getComputedStyle(video).display !== 'none'
    canvas.style.display = visible ? '' : 'none'
    if (visible) {
      if (this.sizeDirty || now - this.lastSizeCheck > 1000) this.resize(now)
      const tf = video.style.transform
      if (canvas.style.transform !== tf) canvas.style.transform = tf

      const timed = this.frame % TIME_EVERY === 0
      const t0 = performance.now()
      this.o.beforeRender?.(video, now, this.frame)
      renderer.render(video, canvas.width, canvas.height, now)
      if (timed) renderer.finish()
      const ms = timed ? performance.now() - t0 : null
      this.frame++
      this.samples.push({ t: now, ms })
      if (this.samples.length > 400) this.samples.splice(0, this.samples.length - 400)
    }

    if (now - this.lastSummaryAt >= 1000) {
      this.lastSummaryAt = now
      const cost = summarizeCost(this.samples, now)
      const stats: OverlayStats = { ...cost, sourceFps: this.sourceFps, width: canvas.width, height: canvas.height }
      this.lastStats = stats
      this.o.onStats?.(stats)
      // Em segundo plano ou comparação o efeito não é julgado: não está a correr a sério.
      if (visible && !this.hidden && document.visibilityState === 'visible') {
        const v = budgetVerdict(cost, this.sourceFps, this.overSince, now, this.o.budget)
        this.overSince = v.overSince
        if (v.disable) return this.halt('budget', v)
      } else {
        this.overSince = null
      }
    }
    this.schedule()
  }

  private resize(now: number) {
    const { video, canvas } = this.o
    this.sizeDirty = false
    this.lastSizeCheck = now
    const fit: ObjectFit = getComputedStyle(video).objectFit === 'contain' ? 'contain' : 'cover'
    canvas.style.objectFit = fit
    const s = fitCanvasSize(video.videoWidth, video.videoHeight, video.clientWidth, video.clientHeight, window.devicePixelRatio || 1, fit, this.o.maxPixels)
    if (canvas.width !== s.width) canvas.width = s.width
    if (canvas.height !== s.height) canvas.height = s.height
  }
}
