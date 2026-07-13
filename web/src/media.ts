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

/**
 * Supressão de ruído por IA (RNNoise via AudioWorklet WASM) — remove teclado,
 * ventoinha, ruído de fundo muito melhor que a supressão nativa do browser.
 * Recebe a track do microfone e devolve uma track "limpa" para enviar à chamada.
 */
export class Denoiser {
  private ctx: AudioContext | null = null
  private node: import('@sapphi-red/web-noise-suppressor').RnnoiseWorkletNode | null = null
  private source: MediaStreamAudioSourceNode | null = null
  private dest: MediaStreamAudioDestinationNode | null = null

  get active(): boolean {
    return !!this.node
  }

  // Se o AudioContext for suspenso (política de autoplay, tab em 2º plano), a
  // track "limpa" fica MUDA e não recupera — os participantes deixam de ouvir.
  // Este handler retoma-o sempre que possível.
  private resume = () => {
    if (this.ctx && this.ctx.state === 'suspended') void this.ctx.resume().catch(() => {})
  }

  /** Encaminha a track do mic pelo RNNoise; devolve a track processada. */
  async process(track: MediaStreamTrack): Promise<MediaStreamTrack> {
    const [{ loadRnnoise, RnnoiseWorkletNode }, wasmUrl, wasmSimdUrl, workletUrl] = await Promise.all([
      import('@sapphi-red/web-noise-suppressor'),
      import('@sapphi-red/web-noise-suppressor/rnnoise.wasm?url').then((m) => m.default),
      import('@sapphi-red/web-noise-suppressor/rnnoise_simd.wasm?url').then((m) => m.default),
      import('@sapphi-red/web-noise-suppressor/rnnoiseWorklet.js?url').then((m) => m.default),
    ])
    // RNNoise trabalha a 48 kHz — fixa a taxa para não reamostrar mal.
    this.ctx = new AudioContext({ sampleRate: 48000 })
    const wasmBinary = await loadRnnoise({ url: wasmUrl, simdUrl: wasmSimdUrl })
    await this.ctx.audioWorklet.addModule(workletUrl)
    this.source = this.ctx.createMediaStreamSource(new MediaStream([track]))
    this.node = new RnnoiseWorkletNode(this.ctx, { maxChannels: 1, wasmBinary })
    this.dest = this.ctx.createMediaStreamDestination()
    this.source.connect(this.node).connect(this.dest)
    // Garante que arranca a processar (contextos criados sem gesto começam
    // 'suspended') e mantém-no vivo se o browser o suspender.
    await this.ctx.resume().catch(() => {})
    this.ctx.onstatechange = this.resume
    document.addEventListener('visibilitychange', this.resume)
    return this.dest.stream.getAudioTracks()[0]
  }

  stop() {
    document.removeEventListener('visibilitychange', this.resume)
    try {
      this.node?.destroy()
    } catch {
      /* já destruído */
    }
    this.source?.disconnect()
    this.node?.disconnect()
    if (this.ctx) this.ctx.onstatechange = null
    void this.ctx?.close()
    this.ctx = null
    this.node = null
    this.source = null
    this.dest = null
  }
}

/** Pede a resolução máxima da câmara (o browser dá o melhor que o hardware tiver). */
export function videoConstraints(deviceId?: string): MediaTrackConstraints {
  return {
    ...(deviceId ? { deviceId: { exact: deviceId } } : {}),
    width: { ideal: 3840 },
    height: { ideal: 2160 },
    // Só `ideal` (nunca `min`): pedir 60fps quando a câmara aguenta, mas SEM
    // restrição obrigatória — um `min` fazia o getUserMedia falhar em câmaras
    // que não garantem o framerate (pouca luz/driver) → caía para áudio-só,
    // deixando o vídeo preto e o mic aparentemente "muted".
    frameRate: { ideal: 60 },
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
  /** Instantâneo do frame (para o RVM: máscara e pessoa vêm do MESMO frame). */
  private frame = document.createElement('canvas')
  private frameCtx = this.frame.getContext('2d')!
  /** Máscara na resolução do modelo. */
  private maskSmall = document.createElement('canvas')
  private maskSmallCtx = this.maskSmall.getContext('2d')!
  private maskPixels: ImageData | null = null
  /** Acumulador com suavização temporal. */
  private maskSmooth = document.createElement('canvas')
  private maskSmoothCtx = this.maskSmooth.getContext('2d')!
  private segmenter: import('@mediapipe/tasks-vision').ImageSegmenter | null = null
  /** Matting de alta qualidade (RVM). Preferido; MediaPipe é o fallback. */
  private matte: import('./matte').RvmMatte | null = null
  private matteTried = false
  private running = false
  private out: MediaStreamTrack | null = null
  private bgImage: HTMLImageElement | null = null
  /** true quando a segmentação corre no delegate GPU do cliente. */
  usingGpu = false

  mode: BackgroundMode = 'blur'
  /** Intensidade do desfoque (px) — "leve" ≈ 10, "forte" ≈ 24. */
  blurPx = 24

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
    // 1ª escolha: matting dedicado (RVM) — cabelo/bordas muito melhores. Carrega
    // sob demanda (code-split do ONNX Runtime). Se falhar, usa-se o MediaPipe.
    if (!this.matte && !this.matteTried) {
      this.matteTried = true
      try {
        const { RvmMatte } = await import('./matte')
        const m = new RvmMatte()
        if (await m.init()) {
          this.matte = m
          this.usingGpu = m.usingWebGpu
        }
      } catch (e) {
        console.warn('[background] RVM indisponível', e)
      }
    }
    if (!this.matte && !this.segmenter) {
      const { ImageSegmenter, FilesetResolver } = await import('@mediapipe/tasks-vision')
      const fileset = await FilesetResolver.forVisionTasks('/mediapipe-wasm')
      const make = (delegate: 'GPU' | 'CPU') =>
        ImageSegmenter.createFromOptions(fileset, {
          baseOptions: { modelAssetPath: '/models/selfie_segmenter.tflite', delegate },
          runningMode: 'VIDEO',
          outputConfidenceMasks: true,
        })
      // Acelera na GPU do cliente quando disponível (render muito melhor); nem
      // todas as GPUs/drivers aguentam o delegate GPU — cai para CPU sem falhar.
      this.segmenter = await make('GPU')
        .then((s) => {
          this.usingGpu = true
          return s
        })
        .catch(() => make('CPU'))
      console.info(`[background] segmentação em ${this.usingGpu ? 'GPU' : 'CPU'}`)
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
    this.frame.width = w
    this.frame.height = h
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
    if (!this.running) return
    if (this.video.readyState < 2) {
      this.scheduleFrame()
      return
    }
    if (this.video.videoWidth && this.video.videoWidth !== this.canvas.width) {
      this.resize(this.video.videoWidth, this.video.videoHeight)
    }
    const W = this.canvas.width
    const H = this.canvas.height
    if (this.matte) {
      // RVM (assíncrono). Só arranca uma corrida de cada vez; congela o frame
      // atual para que a máscara e a pessoa venham do MESMO instante (sem
      // desalinhamento em movimento). Enquanto ocupado, o canvas mantém o
      // último resultado — a track de saída continua a fluir.
      if (!this.matte.busy) {
        this.frameCtx.drawImage(this.video, 0, 0, W, H)
        void this.matte.run(this.frame, W, H).then((alpha) => {
          if (alpha && this.running) this.composeMatte(alpha)
        })
      }
    } else if (this.segmenter) {
      try {
        this.segmenter.segmentForVideo(this.video, performance.now(), (result) => {
          const mask = result.confidenceMasks?.[0]
          if (!mask) return
          this.composeMediapipe(mask)
          mask.close()
        })
      } catch {
        /* frame perdido — segue para o próximo */
      }
    }
    this.scheduleFrame()
  }

  /** Caminho MediaPipe: converte a máscara de confiança em alfa e compõe. */
  private composeMediapipe(mask: import('@mediapipe/tasks-vision').MPMask) {
    const mw = mask.width
    const mh = mask.height
    if (this.maskSmall.width !== mw) {
      this.maskSmall.width = mw
      this.maskSmall.height = mh
      this.maskSmooth.width = mw
      this.maskSmooth.height = mh
      this.maskPixels = null
    }
    // Confiança -> alfa com curva suave. Rampa 0.30..0.62: inclusiva no limite
    // (recupera cabelo, a zona de menor confiança do modelo leve).
    const conf = mask.getAsFloat32Array()
    if (!this.maskPixels) this.maskPixels = this.maskSmallCtx.createImageData(mw, mh)
    const px = this.maskPixels.data
    for (let i = 0; i < conf.length; i++) {
      const a = Math.min(1, Math.max(0, (conf[i] - 0.3) / 0.32))
      px[i * 4 + 3] = a * a * (3 - 2 * a) * 255 // smoothstep
    }
    this.maskSmallCtx.putImageData(this.maskPixels, 0, 0)
    // Máscara rígida por frame (sem mistura temporal → sem rasto).
    this.maskSmoothCtx.clearRect(0, 0, mw, mh)
    this.maskSmoothCtx.drawImage(this.maskSmall, 0, 0)
    this.composite(this.video)
  }

  /** Caminho RVM: o alfa contínuo já vem do modelo; só o desenhamos na máscara. */
  private composeMatte(alpha: HTMLCanvasElement) {
    if (this.maskSmooth.width !== alpha.width) {
      this.maskSmooth.width = alpha.width
      this.maskSmooth.height = alpha.height
    }
    this.maskSmoothCtx.clearRect(0, 0, alpha.width, alpha.height)
    this.maskSmoothCtx.drawImage(alpha, 0, 0)
    this.composite(this.frame)
  }

  /**
   * Composição final partilhada: pessoa recortada (com feather) sobre o fundo
   * recuado (DoF + vinheta) e uma sombra de contacto para profundidade.
   * `personSrc` é a fonte de imagem do frame que originou a máscara atual.
   */
  private composite(personSrc: CanvasImageSource) {
    const W = this.canvas.width
    const H = this.canvas.height

    // Pessoa recortada com pena na borda (blur na máscara ampliada — o cabelo
    // funde-se suavemente no fundo em vez de um corte duro).
    this.personCtx.save()
    this.personCtx.clearRect(0, 0, W, H)
    this.personCtx.drawImage(personSrc, 0, 0, W, H)
    this.personCtx.globalCompositeOperation = 'destination-in'
    this.personCtx.filter = `blur(${Math.max(2, W / 480)}px)`
    this.personCtx.imageSmoothingEnabled = true
    this.personCtx.imageSmoothingQuality = 'high'
    this.personCtx.drawImage(this.maskSmooth, 0, 0, W, H)
    this.personCtx.restore()

    // Fundo recuado (DoF + escurecimento) para a pessoa saltar para a frente.
    this.ctx.save()
    if (this.mode === 'image' && this.bgImage) {
      this.ctx.filter = `blur(${Math.max(2, W / 340)}px) brightness(0.94)`
      drawCover(this.ctx, this.bgImage, W, H)
      this.ctx.filter = 'none'
    } else {
      this.ctx.filter = `blur(${this.blurPx}px) saturate(1.2) brightness(1.02)`
      this.ctx.drawImage(personSrc, -W * 0.06, -H * 0.06, W * 1.12, H * 1.12)
      this.ctx.filter = 'none'
      this.ctx.fillStyle = 'rgba(255,255,255,0.07)'
      this.ctx.fillRect(0, 0, W, H)
    }
    // Vinheta radial: escurece as margens e concentra o foco na pessoa.
    const vg = this.ctx.createRadialGradient(W / 2, H * 0.46, Math.min(W, H) * 0.22, W / 2, H * 0.52, Math.max(W, H) * 0.72)
    vg.addColorStop(0, 'rgba(0, 0, 0, 0)')
    vg.addColorStop(1, 'rgba(0, 0, 0, 0.32)')
    this.ctx.fillStyle = vg
    this.ctx.fillRect(0, 0, W, H)
    this.ctx.restore()

    // Pessoa em primeiro plano com sombra de contacto curta (profundidade).
    this.ctx.save()
    this.ctx.shadowColor = 'rgba(0, 0, 0, 0.4)'
    this.ctx.shadowBlur = Math.max(4, W / 260)
    this.ctx.shadowOffsetX = 0
    this.ctx.shadowOffsetY = Math.max(1, H / 400)
    this.ctx.drawImage(this.person, 0, 0)
    this.ctx.restore()
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
          // Suavização exponencial — 0.35 dá resposta mais direta ao movimento
          // da cabeça (0.25 sentia-se "a nadar") sem tremer.
          this.x += (tx - this.x) * 0.35
          this.y += (ty - this.y) * 0.35
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
 * Motor Whisper local (WASM em worker): transcreve blocos de fala do
 * microfone com deteção de silêncio. Funciona em QUALQUER browser — é o
 * fallback quando não há Web Speech API (Firefox) e não envia áudio a lado
 * nenhum (modelo self-hosted em /models, runtime em /ort).
 */
/**
 * Limpa a saída do whisper-tiny, que alucina em áudio pouco claro/música:
 * remove marcadores de não-fala ("(música)", "[aplausos]"…), créditos de
 * legendas fantasma, e colapsa repetições consecutivas ("É bom. - É bom.").
 */
function cleanWhisper(raw: string): string {
  const t = raw.trim()
  if (!t) return ''
  // Marcador de não-fala isolado (música/aplausos/risos/silêncio/inaudível).
  if (/^[([\-\s]*(m[úu]sica|music|aplausos|applause|risos|laughter|sil[êe]ncio|silence|inaud[íi]vel|ru[íi]do|noise)[)\]\-.\s]*$/i.test(t)) return ''
  // Alucinações típicas em silêncio (créditos de legendas).
  if (/amara\.org|legendas pela comunidade|subtitles? by|subscribe/i.test(t)) return ''
  // Colapsa segmentos consecutivos iguais (separados por " - " ou por frase).
  const norm = (s: string) => s.toLowerCase().replace(/[.!?…\s]+$/, '').trim()
  const parts = t.split(/\s*[-–—]\s*|(?<=[.!?])\s+/).map((s) => s.trim()).filter(Boolean)
  const out: string[] = []
  for (const p of parts) {
    if (!out.length || norm(out[out.length - 1]) !== norm(p)) out.push(p)
  }
  return out.join(' ').trim()
}

class WhisperEngine {
  private worker: Worker | null = null
  private ctx: AudioContext | null = null
  private proc: ScriptProcessorNode | null = null
  private source: MediaStreamAudioSourceNode | null = null
  private ownStream: MediaStream | null = null
  private buf: Float32Array[] = []
  private bufLen = 0
  private speechRun = 0
  private silenceRun = 0
  private lang = 'pt'
  onFinal: ((text: string) => void) | null = null
  onInterim: ((text: string) => void) | null = null

  async start(lang: string, stream?: MediaStream | null): Promise<boolean> {
    this.lang = lang.split('-')[0] // 'pt-PT' -> 'pt'
    this.worker = new Worker(new URL('./whisperWorker.ts', import.meta.url), { type: 'module' })
    this.worker.onmessage = (e) => {
      const m = e.data as { op: string; text?: string; message?: string }
      if (m.op === 'final' && m.text) {
        const t = cleanWhisper(m.text)
        if (t) this.onFinal?.(t)
      }
      if (m.op === 'ready') this.onInterim?.('')
      if (m.op === 'error') console.warn('[whisper]', m.message)
    }
    this.onInterim?.('(a carregar modelo de voz local…)')
    this.worker.postMessage({ op: 'warmup' })

    let audio = stream
    if (!audio || audio.getAudioTracks().length === 0) {
      this.ownStream = await navigator.mediaDevices.getUserMedia({ audio: true }).catch(() => null)
      audio = this.ownStream ?? undefined
    }
    if (!audio) return false

    // 16 kHz mono para o Whisper; blocos fecham por silêncio (~0,7 s) ou 8 s.
    this.ctx = new AudioContext({ sampleRate: 16000 })
    this.source = this.ctx.createMediaStreamSource(audio)
    this.proc = this.ctx.createScriptProcessor(4096, 1, 1)
    const SILENCE = 0.012
    this.proc.onaudioprocess = (ev) => {
      const data = ev.inputBuffer.getChannelData(0)
      let sum = 0
      for (const v of data) sum += v * v
      const rms = Math.sqrt(sum / data.length)
      this.buf.push(new Float32Array(data))
      this.bufLen += data.length
      if (rms > SILENCE) {
        this.speechRun += data.length
        this.silenceRun = 0
      } else {
        this.silenceRun += data.length
      }
      const SR = 16000
      const closeBySilence = this.speechRun > 0.6 * SR && this.silenceRun > 0.7 * SR
      const closeByLength = this.bufLen >= 8 * SR
      if (closeBySilence || closeByLength) this.flush()
      // Sem fala nenhuma há >6 s: descarta para não acumular ruído.
      if (this.speechRun === 0 && this.bufLen > 6 * SR) this.reset()
    }
    this.source.connect(this.proc)
    this.proc.connect(this.ctx.destination)
    return true
  }

  private flush() {
    if (this.speechRun === 0) {
      this.reset()
      return
    }
    const pcm = new Float32Array(this.bufLen)
    let o = 0
    for (const b of this.buf) {
      pcm.set(b, o)
      o += b.length
    }
    this.reset()
    this.worker?.postMessage({ op: 'chunk', pcm, lang: this.lang }, [pcm.buffer])
  }

  private reset() {
    this.buf = []
    this.bufLen = 0
    this.speechRun = 0
    this.silenceRun = 0
  }

  stop() {
    this.flush()
    this.proc?.disconnect()
    this.source?.disconnect()
    void this.ctx?.close()
    this.ownStream?.getTracks().forEach((t) => t.stop())
    // Deixa o último bloco terminar antes de matar o worker.
    const w = this.worker
    setTimeout(() => w?.terminate(), 20_000)
    this.worker = null
    this.proc = null
    this.ctx = null
  }
}

/**
 * Transcrição contínua do áudio local. Usa a Web Speech API quando existe
 * (Chrome/Edge — melhor latência) e cai para Whisper WASM local nos outros
 * browsers (Firefox/Safari). `onFinal` recebe cada frase confirmada.
 */
export class Transcriber {
  private rec: SpeechRecognitionLike | null = null
  private whisper: WhisperEngine | null = null
  private running = false
  private lang = 'pt-PT'
  private stream?: MediaStream | null
  private usedWhisper = false
  onFinal: ((text: string) => void) | null = null
  onInterim: ((text: string) => void) | null = null
  /** Erro legível (permissão negada, sem microfone, etc.). */
  onError: ((message: string) => void) | null = null

  /**
   * Arranca o melhor motor disponível. `stream` alimenta o Whisper (mic).
   * `preferLocal` força o Whisper WASM (100% no dispositivo, privado) em vez da
   * Web Speech do Chrome — que ENVIA áudio para servidores da Google, o que
   * quebra a soberania de dados. Sem `preferLocal`, usa Web Speech quando
   * existe e cai automaticamente no Whisper se a Google estiver inacessível
   * (erro `network`) — típico numa rede self-hosted/offline.
   */
  start(lang = 'pt-PT', stream?: MediaStream | null, preferLocal = false) {
    this.lang = lang
    this.stream = stream
    this.running = true
    const Ctor = ((window as unknown as { webkitSpeechRecognition?: SpeechRecognitionCtor; SpeechRecognition?: SpeechRecognitionCtor })
      .webkitSpeechRecognition ??
      (window as unknown as { SpeechRecognition?: SpeechRecognitionCtor }).SpeechRecognition) as SpeechRecognitionCtor | undefined
    if (!Ctor || preferLocal) {
      // Sem Web Speech (Firefox) ou modo 100% local: Whisper WASM.
      this.startWhisper()
      return true
    }
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
      if (this.running && this.rec) {
        try {
          rec.start()
        } catch {
          /* já a correr */
        }
      }
    }
    rec.onerror = (e) => {
      // `no-speech`/`aborted` são transitórios (a API reinicia sozinha via onend);
      // só se reporta/atua no que impede mesmo a captura.
      const err = (e as { error?: string }).error ?? ''
      if (err === 'not-allowed' || err === 'service-not-allowed') {
        this.onError?.('Permissão de microfone negada — autoriza o microfone para transcrever.')
      } else if (err === 'audio-capture') {
        this.onError?.('Microfone não encontrado — liga um microfone e tenta de novo.')
      } else if (err === 'network' || err === 'language-not-supported') {
        // Chrome Web Speech precisa dos servidores da Google — inacessíveis
        // numa rede self-hosted/offline. Cai para o Whisper local (privado).
        this.rec = null
        try {
          rec.stop()
        } catch {
          /* ignore */
        }
        this.onInterim?.('(rede sem acesso à Google — a mudar para transcrição local…)')
        this.startWhisper()
      }
    }
    this.rec = rec
    try {
      rec.start()
    } catch {
      /* já a correr */
    }
    return true
  }

  /** Arranca o motor Whisper WASM local (uma só vez). */
  private startWhisper() {
    if (this.usedWhisper || !this.running) return
    this.usedWhisper = true
    this.whisper = new WhisperEngine()
    this.whisper.onFinal = (t) => this.onFinal?.(t)
    this.whisper.onInterim = (t) => this.onInterim?.(t)
    void this.whisper.start(this.lang, this.stream).then((ok) => {
      if (!ok && this.running)
        this.onError?.('Sem acesso ao microfone para transcrição — verifica a permissão do browser.')
    })
  }

  stop() {
    this.running = false
    this.rec?.stop()
    this.rec = null
    this.whisper?.stop()
    this.whisper = null
  }
}

/** Toca um tom curto no altifalante escolhido — "Testar os altifalantes". */
export async function playTestTone(sinkId?: string): Promise<void> {
  const ctx = new AudioContext()
  const dest = ctx.createMediaStreamDestination()
  const gain = ctx.createGain()
  gain.gain.value = 0.15
  gain.connect(dest)
  // Duas notas (lá4 → mi5), como o teste do Meet.
  for (const [freq, at] of [[440, 0], [659, 0.35]] as const) {
    const osc = ctx.createOscillator()
    osc.frequency.value = freq
    osc.connect(gain)
    osc.start(ctx.currentTime + at)
    osc.stop(ctx.currentTime + at + 0.3)
  }
  const audio = new Audio()
  audio.srcObject = dest.stream
  const sinkable = audio as HTMLAudioElement & { setSinkId?: (id: string) => Promise<void> }
  if (sinkId && sinkable.setSinkId) await sinkable.setSinkId(sinkId).catch(() => {})
  await audio.play().catch(() => {})
  setTimeout(() => {
    audio.pause()
    void ctx.close()
  }, 900)
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
