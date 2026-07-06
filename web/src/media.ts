/**
 * Utilitários de media: dispositivos, qualidade, desfoque de fundo (IA),
 * medidores de nível de voz e gravação composta da reunião.
 */

export interface DeviceSets {
  mics: MediaDeviceInfo[]
  cams: MediaDeviceInfo[]
  speakers: MediaDeviceInfo[]
}

export async function listDevices(): Promise<DeviceSets> {
  const all = await navigator.mediaDevices.enumerateDevices()
  return {
    mics: all.filter((d) => d.kind === 'audioinput'),
    cams: all.filter((d) => d.kind === 'videoinput'),
    speakers: all.filter((d) => d.kind === 'audiooutput'),
  }
}

/** Constraints de áudio com supressão de ruído opcional. */
export function audioConstraints(deviceId?: string, noiseSuppression = true): MediaTrackConstraints {
  return {
    ...(deviceId ? { deviceId: { exact: deviceId } } : {}),
    echoCancellation: true,
    noiseSuppression,
    autoGainControl: true,
  }
}

/** Pede a resolução máxima da câmara (o browser dá o melhor que o hardware tiver). */
export function videoConstraints(deviceId?: string): MediaTrackConstraints {
  return {
    ...(deviceId ? { deviceId: { exact: deviceId } } : {}),
    width: { ideal: 3840 },
    height: { ideal: 2160 },
    frameRate: { ideal: 30 },
  }
}

/* ------------------------------------------------------------------ */
/* Efeitos de fundo (MediaPipe Selfie Segmentation)                    */
/* ------------------------------------------------------------------ */

export type BackgroundMode = 'blur' | 'image'

/**
 * Recorte pessoa/fundo com qualidade de produção:
 * - máscara de confiança com curva suave (sem contorno binário/halo);
 * - suavização temporal da máscara (sem cintilação nem "flutuar");
 * - pena (feather) na borda ao compor;
 * - fundo em vidro fosco (blur+saturação+véu) ou imagem em cover-fit,
 *   sempre desenhado ATRÁS da pessoa — nunca sobreposto.
 * Compatibilidade: resolução lida do próprio vídeo (não do getSettings),
 * segue mudanças de resolução da câmara, GPU com fallback para CPU e
 * sincronização com os frames reais via requestVideoFrameCallback.
 */
export class BackgroundEffect {
  private raw: MediaStreamTrack | null = null
  private video = document.createElement('video')
  private canvas = document.createElement('canvas')
  private ctx = this.canvas.getContext('2d')!
  private person = document.createElement('canvas')
  private personCtx = this.person.getContext('2d')!
  /** Máscara na resolução do modelo. */
  private maskSmall = document.createElement('canvas')
  private maskSmallCtx = this.maskSmall.getContext('2d')!
  private maskPixels: ImageData | null = null
  /** Acumulador com suavização temporal. */
  private maskSmooth = document.createElement('canvas')
  private maskSmoothCtx = this.maskSmooth.getContext('2d')!
  private segmenter: import('@mediapipe/tasks-vision').ImageSegmenter | null = null
  private running = false
  private out: MediaStreamTrack | null = null
  private bgImage: HTMLImageElement | null = null

  mode: BackgroundMode = 'blur'

  /** Define a imagem de fundo (dataURL/objectURL) e muda para modo imagem. */
  async setImage(url: string): Promise<void> {
    const img = new Image()
    img.crossOrigin = 'anonymous'
    await new Promise<void>((resolve, reject) => {
      img.onload = () => resolve()
      img.onerror = () => reject(new Error('imagem inválida'))
      img.src = url
    })
    this.bgImage = img
    this.mode = 'image'
  }

  get started(): boolean {
    return this.running
  }

  /** Track de saída atual (null se parado). */
  get output(): MediaStreamTrack | null {
    return this.out
  }

  /** Track processada; a original fica intacta para poder ser reposta. */
  async start(cameraTrack: MediaStreamTrack): Promise<MediaStreamTrack> {
    if (!this.segmenter) {
      const { ImageSegmenter, FilesetResolver } = await import('@mediapipe/tasks-vision')
      const fileset = await FilesetResolver.forVisionTasks('/mediapipe-wasm')
      const make = (delegate: 'GPU' | 'CPU') =>
        ImageSegmenter.createFromOptions(fileset, {
          baseOptions: { modelAssetPath: '/models/selfie_segmenter.tflite', delegate },
          runningMode: 'VIDEO',
          outputConfidenceMasks: true,
        })
      // Nem todas as GPUs/drivers aguentam o delegate GPU — cair para CPU.
      this.segmenter = await make('GPU').catch(() => make('CPU'))
    }

    this.raw = cameraTrack
    this.video.srcObject = new MediaStream([cameraTrack])
    this.video.muted = true
    this.video.playsInline = true
    await this.video.play()
    // Dimensões reais do fluxo (getSettings pode mentir em algumas câmaras).
    if (!this.video.videoWidth) {
      await new Promise<void>((r) => this.video.addEventListener('loadedmetadata', () => r(), { once: true }))
    }
    this.resize(this.video.videoWidth || 1280, this.video.videoHeight || 720)

    this.running = true
    this.scheduleFrame()
    const fps = cameraTrack.getSettings().frameRate ?? 30
    const stream = this.canvas.captureStream(fps)
    this.out = stream.getVideoTracks()[0]
    this.out.contentHint = 'motion'
    return this.out
  }

  private resize(w: number, h: number) {
    this.canvas.width = w
    this.canvas.height = h
    this.person.width = w
    this.person.height = h
  }

  private scheduleFrame() {
    if (!this.running) return
    const anyVideo = this.video as HTMLVideoElement & {
      requestVideoFrameCallback?: (cb: () => void) => void
    }
    // Sincronizar com os frames reais da câmara poupa CPU e evita judder
    // em câmaras de 15/24 fps; rAF é o fallback universal.
    if (anyVideo.requestVideoFrameCallback) anyVideo.requestVideoFrameCallback(() => this.renderFrame())
    else requestAnimationFrame(() => this.renderFrame())
  }

  private renderFrame() {
    if (!this.running || !this.segmenter) return
    if (this.video.readyState < 2) {
      this.scheduleFrame()
      return
    }
    if (this.video.videoWidth && this.video.videoWidth !== this.canvas.width) {
      this.resize(this.video.videoWidth, this.video.videoHeight)
    }
    try {
      this.segmenter.segmentForVideo(this.video, performance.now(), (result) => {
        const mask = result.confidenceMasks?.[0]
        if (!mask) return
        this.compose(mask)
        mask.close()
      })
    } catch {
      /* frame perdido — segue para o próximo */
    }
    this.scheduleFrame()
  }

  private compose(mask: import('@mediapipe/tasks-vision').MPMask) {
    const mw = mask.width
    const mh = mask.height
    if (this.maskSmall.width !== mw) {
      this.maskSmall.width = mw
      this.maskSmall.height = mh
      this.maskSmooth.width = mw
      this.maskSmooth.height = mh
      this.maskPixels = null
    }
    // 1) Confiança -> alfa com curva suave e LIGEIRA EROSÃO: só >0.5 conta
    //    como pessoa, rampa 0.5..0.8. Puxa a borda para dentro (esconde o
    //    halo do fundo a "vazar") e feather do lado da pessoa, sem serrilhar.
    const conf = mask.getAsFloat32Array()
    if (!this.maskPixels) this.maskPixels = this.maskSmallCtx.createImageData(mw, mh)
    const px = this.maskPixels.data
    for (let i = 0; i < conf.length; i++) {
      const a = Math.min(1, Math.max(0, (conf[i] - 0.5) / 0.3))
      px[i * 4 + 3] = a * a * (3 - 2 * a) * 255 // smoothstep
    }
    this.maskSmallCtx.putImageData(this.maskPixels, 0, 0)

    // 2) Suavização temporal LEVE (85% frame novo + 15% histórico): estabiliza
    //    a borda sem deixar rasto/fantasma quando a pessoa ou a câmara mexem.
    this.maskSmoothCtx.globalAlpha = 0.85
    this.maskSmoothCtx.drawImage(this.maskSmall, 0, 0)
    this.maskSmoothCtx.globalAlpha = 1

    const W = this.canvas.width
    const H = this.canvas.height

    // 3) Pessoa recortada com pena na borda (blur aplicado à máscara
    //    ampliada, não à imagem — borda macia sem fantasma).
    this.personCtx.save()
    this.personCtx.clearRect(0, 0, W, H)
    this.personCtx.drawImage(this.video, 0, 0, W, H)
    this.personCtx.globalCompositeOperation = 'destination-in'
    this.personCtx.filter = `blur(${Math.max(2, W / 480)}px)`
    this.personCtx.imageSmoothingEnabled = true
    this.personCtx.imageSmoothingQuality = 'high'
    this.personCtx.drawImage(this.maskSmooth, 0, 0, W, H)
    this.personCtx.restore()

    // 4) Fundo por baixo, sempre a cobrir 100% do frame.
    this.ctx.save()
    if (this.mode === 'image' && this.bgImage) {
      drawCover(this.ctx, this.bgImage, W, H)
    } else {
      // Vidro fosco: desenho ligeiramente ampliado (o blur não "vaza" nas
      // margens), desfocado e saturado, com um véu subtil por cima.
      this.ctx.filter = 'blur(24px) saturate(1.2) brightness(1.05)'
      this.ctx.drawImage(this.video, -W * 0.06, -H * 0.06, W * 1.12, H * 1.12)
      this.ctx.filter = 'none'
      this.ctx.fillStyle = 'rgba(255,255,255,0.07)'
      this.ctx.fillRect(0, 0, W, H)
    }
    this.ctx.restore()

    // 5) Pessoa por cima do fundo.
    this.ctx.drawImage(this.person, 0, 0)
  }

  /** Para o processamento e devolve a track original da câmara. */
  stop(): MediaStreamTrack | null {
    this.running = false
    this.out?.stop()
    this.out = null
    this.video.srcObject = null
    this.maskSmoothCtx.clearRect(0, 0, this.maskSmooth.width, this.maskSmooth.height)
    this.segmenter?.close()
    this.segmenter = null
    return this.raw
  }
}

function drawCover(
  ctx: CanvasRenderingContext2D,
  img: HTMLImageElement,
  W: number,
  H: number,
) {
  const scale = Math.max(W / img.naturalWidth, H / img.naturalHeight)
  const dw = img.naturalWidth * scale
  const dh = img.naturalHeight * scale
  ctx.drawImage(img, (W - dw) / 2, (H - dh) / 2, dw, dh)
}

/* ------------------------------------------------------------------ */
/* Head-tracking para efeito de sala 3D (paralaxe head-coupled)        */
/* ------------------------------------------------------------------ */

/**
 * Segue a posição da cabeça do próprio utilizador (via deteção facial na
 * câmara local) e devolve deslocamentos normalizados [-1..1]. A UI usa isto
 * para deslocar/rodar os tiles em 3D — ao mover a cabeça, a "janela" para a
 * sala acompanha o movimento (parallax), como olhar por uma janela real.
 */
export class HeadTracker {
  private video = document.createElement('video')
  private detector: import('@mediapipe/tasks-vision').FaceDetector | null = null
  private running = false
  /** Suavizado para não tremer. */
  x = 0
  y = 0
  onUpdate: ((x: number, y: number) => void) | null = null

  async start(cameraStream: MediaStream) {
    const { FaceDetector, FilesetResolver } = await import('@mediapipe/tasks-vision')
    const fileset = await FilesetResolver.forVisionTasks('/mediapipe-wasm')
    const make = (delegate: 'GPU' | 'CPU') =>
      FaceDetector.createFromOptions(fileset, {
        baseOptions: { modelAssetPath: '/models/blaze_face_short_range.tflite', delegate },
        runningMode: 'VIDEO',
      })
    this.detector = await make('GPU').catch(() => make('CPU'))
    this.video.srcObject = cameraStream
    this.video.muted = true
    this.video.playsInline = true
    await this.video.play()
    this.running = true
    this.loop()
  }

  private loop = () => {
    if (!this.running || !this.detector) return
    if (this.video.readyState >= 2 && this.video.videoWidth) {
      try {
        const res = this.detector.detectForVideo(this.video, performance.now())
        const box = res.detections[0]?.boundingBox
        if (box) {
          const cx = (box.originX + box.width / 2) / this.video.videoWidth
          const cy = (box.originY + box.height / 2) / this.video.videoHeight
          // Centro da cabeça -> offset [-1..1]. Câmara é espelhada => invert x.
          const tx = -(cx - 0.5) * 2
          const ty = (cy - 0.5) * 2
          // Suavização exponencial.
          this.x += (tx - this.x) * 0.25
          this.y += (ty - this.y) * 0.25
          this.onUpdate?.(this.x, this.y)
        }
      } catch {
        /* frame perdido */
      }
    }
    requestAnimationFrame(this.loop)
  }

  stop() {
    this.running = false
    this.video.srcObject = null
    this.detector?.close()
    this.detector = null
    this.x = 0
    this.y = 0
  }
}

/* ------------------------------------------------------------------ */
/* Transcrição AI (Web Speech API) para notas / MoM                    */
/* ------------------------------------------------------------------ */

type SpeechRecognitionCtor = new () => SpeechRecognitionLike
interface SpeechRecognitionLike {
  continuous: boolean
  interimResults: boolean
  lang: string
  onresult: ((e: { resultIndex: number; results: ArrayLike<{ 0: { transcript: string }; isFinal: boolean }> }) => void) | null
  onerror: ((e: unknown) => void) | null
  onend: (() => void) | null
  start(): void
  stop(): void
}

export function speechSupported(): boolean {
  return 'webkitSpeechRecognition' in window || 'SpeechRecognition' in window
}

/**
 * Transcrição contínua do áudio local (Web Speech API do browser). Nota:
 * captura só o microfone deste dispositivo, não o áudio remoto. Chrome/Edge
 * suportam; Firefox não. `onFinal` recebe cada frase confirmada.
 */
export class Transcriber {
  private rec: SpeechRecognitionLike | null = null
  private running = false
  onFinal: ((text: string) => void) | null = null
  onInterim: ((text: string) => void) | null = null

  start(lang = 'pt-PT') {
    const Ctor = ((window as unknown as { webkitSpeechRecognition?: SpeechRecognitionCtor; SpeechRecognition?: SpeechRecognitionCtor })
      .webkitSpeechRecognition ??
      (window as unknown as { SpeechRecognition?: SpeechRecognitionCtor }).SpeechRecognition) as SpeechRecognitionCtor | undefined
    if (!Ctor) return false
    const rec = new Ctor()
    rec.continuous = true
    rec.interimResults = true
    rec.lang = lang
    rec.onresult = (e) => {
      let interim = ''
      for (let i = e.resultIndex; i < e.results.length; i++) {
        const r = e.results[i]
        if (r.isFinal) this.onFinal?.(r[0].transcript.trim())
        else interim += r[0].transcript
      }
      if (interim) this.onInterim?.(interim)
    }
    // Reinicia sozinho (a API pára após silêncio prolongado).
    rec.onend = () => {
      if (this.running) {
        try {
          rec.start()
        } catch {
          /* já a correr */
        }
      }
    }
    rec.onerror = () => {}
    this.rec = rec
    this.running = true
    try {
      rec.start()
    } catch {
      /* já a correr */
    }
    return true
  }

  stop() {
    this.running = false
    this.rec?.stop()
    this.rec = null
  }
}

/** Fundos predefinidos gerados localmente (gradientes suaves). */
export function presetBackgrounds(): { name: string; url: string }[] {
  const make = (stops: [string, string], angleDeg: number) => {
    const c = document.createElement('canvas')
    c.width = 640
    c.height = 360
    const g = c.getContext('2d')!
    const a = (angleDeg * Math.PI) / 180
    const grad = g.createLinearGradient(
      320 - Math.cos(a) * 320, 180 - Math.sin(a) * 180,
      320 + Math.cos(a) * 320, 180 + Math.sin(a) * 180,
    )
    grad.addColorStop(0, stops[0])
    grad.addColorStop(1, stops[1])
    g.fillStyle = grad
    g.fillRect(0, 0, 640, 360)
    return c.toDataURL('image/jpeg', 0.85)
  }
  return [
    { name: 'Crepúsculo', url: make(['#1c2733', '#3d2a4f'], 25) },
    { name: 'Oceano', url: make(['#0d2b3e', '#1b5e74'], 160) },
    { name: 'Delonix', url: make(['#33150d', '#a3421f'], 65) },
  ]
}

/* ------------------------------------------------------------------ */
/* Níveis de voz (quem está a falar)                                   */
/* ------------------------------------------------------------------ */

const SPEAK_THRESHOLD = 0.02

export class LevelWatcher {
  private audioCtx = new AudioContext()
  private analysers = new Map<string, { analyser: AnalyserNode; source: MediaStreamAudioSourceNode; buf: Float32Array<ArrayBuffer> }>()
  private timer: number

  /** `onLevels` recebe, a cada tick, o conjunto de ids atualmente a falar. */
  constructor(private onLevels: (speaking: Set<string>) => void, intervalMs = 180) {
    this.timer = window.setInterval(() => this.tick(), intervalMs)
  }

  /** Regista/substitui a fonte de áudio de um participante. */
  watch(id: string, stream: MediaStream) {
    if (!stream.getAudioTracks().length) return
    this.unwatch(id)
    try {
      const source = this.audioCtx.createMediaStreamSource(stream)
      const analyser = this.audioCtx.createAnalyser()
      analyser.fftSize = 512
      source.connect(analyser)
      this.analysers.set(id, { analyser, source, buf: new Float32Array(analyser.fftSize) })
    } catch {
      /* stream sem áudio válido */
    }
  }

  unwatch(id: string) {
    const entry = this.analysers.get(id)
    if (entry) {
      entry.source.disconnect()
      this.analysers.delete(id)
    }
  }

  /** O AudioContext só arranca após um gesto do utilizador. */
  resume() {
    if (this.audioCtx.state === 'suspended') void this.audioCtx.resume()
  }

  private tick() {
    const speaking = new Set<string>()
    for (const [id, { analyser, buf }] of this.analysers) {
      analyser.getFloatTimeDomainData(buf)
      let sum = 0
      for (let i = 0; i < buf.length; i++) sum += buf[i] * buf[i]
      if (Math.sqrt(sum / buf.length) > SPEAK_THRESHOLD) speaking.add(id)
    }
    this.onLevels(speaking)
  }

  close() {
    clearInterval(this.timer)
    for (const id of [...this.analysers.keys()]) this.unwatch(id)
    void this.audioCtx.close()
  }
}

/* ------------------------------------------------------------------ */
/* Gravação da reunião (grelha composta + áudio misturado)             */
/* ------------------------------------------------------------------ */

export interface RecorderSource {
  id: string
  label: string
  stream: MediaStream | null
}

export class MeetingRecorder {
  private canvas = document.createElement('canvas')
  private ctx = this.canvas.getContext('2d')!
  private audioCtx = new AudioContext()
  private dest = this.audioCtx.createMediaStreamDestination()
  private recorder: MediaRecorder
  private chunks: Blob[] = []
  private videos = new Map<string, HTMLVideoElement>()
  private audioSrcs = new Map<string, MediaStreamAudioSourceNode>()
  private labels = new Map<string, string>()
  private running = true
  private drawTimer: number

  constructor(fps = 15) {
    this.canvas.width = 1280
    this.canvas.height = 720
    const stream = this.canvas.captureStream(fps)
    // Fonte silenciosa sempre ligada: sem ela, um destino de áudio sem
    // entradas não produz amostras e o muxer do MediaRecorder fica bloqueado
    // (gravação vazia quando ninguém tem microfone).
    const silent = this.audioCtx.createConstantSource()
    silent.offset.value = 0
    silent.connect(this.dest)
    silent.start()
    for (const t of this.dest.stream.getAudioTracks()) stream.addTrack(t)
    const mime = MediaRecorder.isTypeSupported('video/webm;codecs=vp8,opus')
      ? 'video/webm;codecs=vp8,opus'
      : 'video/webm'
    this.recorder = new MediaRecorder(stream, { mimeType: mime, videoBitsPerSecond: 2_500_000 })
    this.recorder.ondataavailable = (e) => e.data.size && this.chunks.push(e.data)
    this.recorder.start(1000)
    this.drawTimer = window.setInterval(() => this.draw(), 1000 / fps)
  }

  /** Sincroniza as fontes (chamar sempre que os participantes mudarem). */
  setSources(sources: RecorderSource[]) {
    const ids = new Set(sources.map((s) => s.id))
    for (const [id, v] of this.videos) {
      if (!ids.has(id)) {
        v.srcObject = null
        this.videos.delete(id)
      }
    }
    for (const [id, src] of this.audioSrcs) {
      if (!ids.has(id)) {
        src.disconnect()
        this.audioSrcs.delete(id)
      }
    }
    for (const s of sources) {
      this.labels.set(s.id, s.label)
      if (!s.stream) continue
      if (s.stream.getVideoTracks().length && !this.videos.has(s.id)) {
        const v = document.createElement('video')
        v.muted = true
        v.srcObject = s.stream
        void v.play().catch(() => {})
        this.videos.set(s.id, v)
      }
      if (s.stream.getAudioTracks().length && !this.audioSrcs.has(s.id)) {
        try {
          const src = this.audioCtx.createMediaStreamSource(s.stream)
          src.connect(this.dest)
          this.audioSrcs.set(s.id, src)
        } catch {
          /* sem áudio */
        }
      }
    }
  }

  private draw() {
    if (!this.running) return
    const entries = [...this.videos.entries()].filter(([, v]) => v.readyState >= 2)
    const { width: W, height: H } = this.canvas
    this.ctx.fillStyle = '#101418'
    this.ctx.fillRect(0, 0, W, H)
    const n = Math.max(entries.length, 1)
    const cols = n <= 1 ? 1 : n <= 4 ? 2 : 3
    const rows = Math.ceil(n / cols)
    const cw = W / cols
    const ch = H / rows
    entries.forEach(([id, v], i) => {
      const x = (i % cols) * cw
      const y = Math.floor(i / cols) * ch
      // object-fit: cover
      const scale = Math.max(cw / v.videoWidth, ch / v.videoHeight)
      const dw = v.videoWidth * scale
      const dh = v.videoHeight * scale
      this.ctx.save()
      this.ctx.beginPath()
      this.ctx.rect(x + 3, y + 3, cw - 6, ch - 6)
      this.ctx.clip()
      this.ctx.drawImage(v, x + (cw - dw) / 2, y + (ch - dh) / 2, dw, dh)
      this.ctx.fillStyle = 'rgba(0,0,0,0.55)'
      const label = this.labels.get(id) ?? ''
      this.ctx.font = '16px system-ui'
      const tw = this.ctx.measureText(label).width
      this.ctx.fillRect(x + 12, y + ch - 40, tw + 16, 26)
      this.ctx.fillStyle = '#fff'
      this.ctx.fillText(label, x + 20, y + ch - 21)
      this.ctx.restore()
    })
  }

  /** Termina e devolve o ficheiro webm final. */
  stop(): Promise<Blob> {
    this.running = false
    clearInterval(this.drawTimer)
    return new Promise((resolve) => {
      this.recorder.onstop = () => {
        for (const v of this.videos.values()) v.srcObject = null
        for (const s of this.audioSrcs.values()) s.disconnect()
        void this.audioCtx.close()
        resolve(new Blob(this.chunks, { type: 'video/webm' }))
      }
      this.recorder.stop()
    })
  }
}
