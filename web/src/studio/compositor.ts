/**
 * Compositor do Estúdio — grava uma vídeo-aula compondo ECRÃ + CÂMARA num
 * canvas, e grava o canvas.
 *
 * PORQUE NÃO REUSA O `MeetingRecorder` (media.ts): esse existe para gravar uma
 * REUNIÃO — grelha de participantes, 1280×720, 15 fps, fontes que entram e saem
 * pela rede. Uma aula é outra coisa: uma fonte de ecrã que manda no
 * enquadramento, uma câmara pequena por cima, e ninguém a entrar a meio. Forçar
 * os dois no mesmo objecto dava um terceiro que não serve bem nenhum.
 *
 * O que NÃO se herda de propósito: as `SCREEN_CONSTRAINTS` do webrtc.ts pedem
 * `frameRate: { ideal: 5, max: 15 }`. Está certo para partilhar slides numa
 * chamada, onde poupar banda ganha; numa aula gravada dá um resultado aos
 * solavancos. Aqui pede-se 30.
 */

/** Onde fica a bolha da câmara. `livre` = posição arrastada pelo utilizador. */
export type CantoDoAvatar = 'inferior-direito' | 'inferior-esquerdo' | 'superior-direito' | 'superior-esquerdo' | 'livre'

export type FormaDoAvatar = 'circulo' | 'rectangulo'

/**
 * `bolha` = a câmara dentro de uma forma. `recorte` = a PESSOA sem fundo,
 * sobreposta aos slides — o efeito que se reconhece do Loom. O recorte usa o
 * `BackgroundEffect` que já existe no repo (media.ts): RVM com fallback para
 * MediaPipe, com a borda já suavizada. Custa CPU/GPU enquanto grava.
 */
export type ModoDoAvatar = 'bolha' | 'recorte'

/** Rectângulo de recorte, em fracções (0–1) do ecrã capturado. */
export interface Recorte {
  x: number
  y: number
  w: number
  h: number
}

export const RECORTE_INTEIRO: Recorte = { x: 0, y: 0, w: 1, h: 1 }

export interface EstadoDoAvatar {
  visivel: boolean
  canto: CantoDoAvatar
  /** Fracção da ALTURA do canvas ocupada pela bolha (0,10–0,45). */
  tamanho: number
  forma: FormaDoAvatar
  modo: ModoDoAvatar
  /** Só usado com `canto: 'livre'` — centro da bolha, em fracções do canvas. */
  x: number
  y: number
}

export const AVATAR_INICIAL: EstadoDoAvatar = {
  visivel: true,
  canto: 'inferior-direito',
  tamanho: 0.22,
  forma: 'circulo',
  modo: 'bolha',
  x: 0.82,
  y: 0.78,
}

/** Constraints do ecrã para GRAVAÇÃO — ver a nota no topo. */
export const ECRA_PARA_GRAVACAO: DisplayMediaStreamOptions = {
  video: {
    width: { ideal: 1920, max: 1920 },
    height: { ideal: 1080, max: 1080 },
    frameRate: { ideal: 30, max: 30 },
  },
  // O áudio do sistema/separador é opcional e o utilizador escolhe no picker.
  // Sem `echoCancellation` — não é voz, é o som da aula.
  audio: { echoCancellation: false, noiseSuppression: false, autoGainControl: false },
}

const MARGEM = 0.03 // fracção da largura, entre a bolha e a borda

/** O que sai de uma gravação: o ficheiro pronto e as duas faixas isoladas. */
export interface ResultadoDaGravacao {
  completo: Blob
  video: Blob | null
  audio: Blob | null
}

export interface OpcoesDoCompositor {
  largura?: number
  altura?: number
  fps?: number
  bitrate?: number
}

/**
 * Compõe e grava. O ciclo de desenho corre em `requestAnimationFrame` — um
 * `setInterval` a 30 Hz desalinha-se do vsync e produz frames duplicados.
 */
export class CompositorDeAula {
  readonly canvas = document.createElement('canvas')
  private ctx: CanvasRenderingContext2D
  private ecraVideo = document.createElement('video')
  private camaraVideo = document.createElement('video')

  private audioCtx: AudioContext | null = null
  private destino: MediaStreamAudioDestinationNode | null = null
  /** O fluxo composto, partilhado entre a gravação e o directo. */
  private fluxoComposto: MediaStream | null = null
  /**
   * Quantos consumidores estão agarrados ao fluxo (gravação, directo).
   *
   * Existe porque parar a gravação FECHAVA o `AudioContext` — e um directo a
   * decorrer sobre o mesmo fluxo emudecia nesse instante, sem erro nenhum. Um
   * recurso partilhado só se desmonta quando o último o larga.
   */
  private consumidores = 0
  private fontesAudio: MediaStreamAudioSourceNode[] = []

  /**
   * DOIS gravadores, não um (pedido: «separar o áudio do vídeo e depois
   * juntar»). Separar depois obrigava a desmultiplexar um WebM já fechado;
   * gravar separado desde o início faz da separação uma propriedade do
   * desenho, e o «juntar» é só voltar a dar as duas faixas ao mesmo elemento.
   * O terceiro gravador continua a produzir o ficheiro combinado, que é o que
   * a maioria das pessoas quer descarregar sem pensar em faixas.
   */
  private gravador: MediaRecorder | null = null
  private gravadorVideo: MediaRecorder | null = null
  private gravadorAudio: MediaRecorder | null = null
  private pedacos: Blob[] = []
  private pedacosVideo: Blob[] = []
  private pedacosAudio: Blob[] = []
  private raf = 0
  private vivo = true

  private ecraStream: MediaStream | null = null
  private camaraStream: MediaStream | null = null

  recorte: Recorte = { ...RECORTE_INTEIRO }
  avatar: EstadoDoAvatar = { ...AVATAR_INICIAL }

  /**
   * Fonte da pessoa recortada, quando o modo `recorte` está ligado. É um
   * canvas com alfa vindo do `BackgroundEffect` — quem liga o efeito é a
   * página, para o compositor não ter de conhecer o pipeline de segmentação.
   */
  pessoaComAlfa: CanvasImageSource | null = null

  /** Segundos gravados. Lido pelo painel; não dispara render por si. */
  get segundos(): number {
    return this.inicioMs ? Math.floor((Date.now() - this.inicioMs) / 1000) : 0
  }
  private inicioMs = 0

  constructor(private opcoes: OpcoesDoCompositor = {}) {
    this.canvas.width = opcoes.largura ?? 1920
    this.canvas.height = opcoes.altura ?? 1080
    const ctx = this.canvas.getContext('2d', { alpha: false })
    if (!ctx) throw new Error('canvas 2d indisponível')
    this.ctx = ctx
    for (const v of [this.ecraVideo, this.camaraVideo]) {
      v.muted = true
      v.playsInline = true
    }
  }

  // ---------------------------------------------------------------- fontes

  /** Abre o seletor do browser (ecrã inteiro / janela / separador). */
  async escolherEcra(): Promise<void> {
    const s = await navigator.mediaDevices.getDisplayMedia(ECRA_PARA_GRAVACAO)
    this.pararEcra()
    this.ecraStream = s
    this.ecraVideo.srcObject = s
    await this.ecraVideo.play().catch(() => {})
    // O utilizador pode parar a partilha pelo aviso do browser: isso é um fim
    // de fonte, não um erro — quem usa o compositor decide o que fazer.
    s.getVideoTracks()[0]?.addEventListener('ended', () => this.aoPerderEcra?.())
  }

  /** Chamado quando o utilizador pára a partilha pelo aviso do browser. */
  aoPerderEcra: (() => void) | null = null

  async ligarCamara(deviceId?: string): Promise<void> {
    const s = await navigator.mediaDevices.getUserMedia({
      video: { deviceId: deviceId ? { exact: deviceId } : undefined, width: { ideal: 1280 }, height: { ideal: 720 } },
      audio: false,
    })
    this.pararCamara()
    this.camaraStream = s
    this.camaraVideo.srcObject = s
    await this.camaraVideo.play().catch(() => {})
  }

  desligarCamara(): void {
    this.pararCamara()
    this.camaraVideo.srcObject = null
  }

  get temEcra(): boolean {
    return !!this.ecraStream && this.ecraVideo.videoWidth > 0
  }
  get temCamara(): boolean {
    return !!this.camaraStream && this.camaraVideo.videoWidth > 0
  }
  /** A track de vídeo da câmara — o `BackgroundEffect` precisa dela crua. */
  get trackDaCamara(): MediaStreamTrack | null {
    return this.camaraStream?.getVideoTracks()[0] ?? null
  }
  /** Dimensões do ecrã capturado — precisas para o seletor de recorte. */
  get dimensoesDoEcra(): { w: number; h: number } {
    return { w: this.ecraVideo.videoWidth, h: this.ecraVideo.videoHeight }
  }

  // ---------------------------------------------------------------- desenho

  /** Arranca o ciclo de desenho (pré-visualização), sem gravar. */
  iniciarPreVisualizacao(): void {
    if (this.raf) return
    const passo = () => {
      if (!this.vivo) return
      this.desenhar()
      this.raf = requestAnimationFrame(passo)
    }
    this.raf = requestAnimationFrame(passo)
  }

  private desenhar(): void {
    const { width: W, height: H } = this.canvas
    this.ctx.fillStyle = '#0d1117'
    this.ctx.fillRect(0, 0, W, H)

    if (this.temEcra) {
      const vw = this.ecraVideo.videoWidth
      const vh = this.ecraVideo.videoHeight
      const sx = this.recorte.x * vw
      const sy = this.recorte.y * vh
      const sw = Math.max(1, this.recorte.w * vw)
      const sh = Math.max(1, this.recorte.h * vh)
      // `contain`: a região escolhida cabe inteira, com barras se o rácio
      // não bater certo. Cortar aqui seria recortar duas vezes — o utilizador
      // já escolheu o que quer ver.
      const escala = Math.min(W / sw, H / sh)
      const dw = sw * escala
      const dh = sh * escala
      this.ctx.drawImage(this.ecraVideo, sx, sy, sw, sh, (W - dw) / 2, (H - dh) / 2, dw, dh)
    }

    if (this.avatar.visivel && this.temCamara) this.desenharAvatar(W, H)
  }

  private desenharAvatar(W: number, H: number): void {
    const lado = Math.max(48, this.avatar.tamanho * H)
    const margem = MARGEM * W
    let cx: number
    let cy: number
    switch (this.avatar.canto) {
      case 'inferior-direito':
        cx = W - margem - lado / 2
        cy = H - margem - lado / 2
        break
      case 'inferior-esquerdo':
        cx = margem + lado / 2
        cy = H - margem - lado / 2
        break
      case 'superior-direito':
        cx = W - margem - lado / 2
        cy = margem + lado / 2
        break
      case 'superior-esquerdo':
        cx = margem + lado / 2
        cy = margem + lado / 2
        break
      default:
        cx = this.avatar.x * W
        cy = this.avatar.y * H
    }
    // Não deixa a bolha sair do enquadramento, seja qual for o modo.
    cx = Math.min(W - lado / 2, Math.max(lado / 2, cx))
    cy = Math.min(H - lado / 2, Math.max(lado / 2, cy))

    // MODO RECORTE: a pessoa sem fundo, sem forma à volta. Desenha-se maior
    // que a bolha (a silhueta ocupa só parte do frame) e assente na borda de
    // baixo, que é onde uma pessoa a apresentar naturalmente fica.
    if (this.avatar.modo === 'recorte' && this.pessoaComAlfa) {
      const fonte = this.pessoaComAlfa as HTMLCanvasElement
      const fw = fonte.width
      const fh = fonte.height
      if (fw > 0 && fh > 0) {
        const alturaAlvo = Math.min(H, lado * 2.6)
        const esc = alturaAlvo / fh
        const dw = fw * esc
        const dh = fh * esc
        // Espelhado, como a pessoa se vê.
        this.ctx.save()
        this.ctx.translate(cx + dw / 2, H - dh)
        this.ctx.scale(-1, 1)
        this.ctx.drawImage(fonte, 0, 0, dw, dh)
        this.ctx.restore()
      }
      return
    }

    const vw = this.camaraVideo.videoWidth
    const vh = this.camaraVideo.videoHeight
    // `cover` dentro da bolha: recorta o lado maior em vez de espremer a cara.
    const escala = Math.max(lado / vw, lado / vh)
    const dw = vw * escala
    const dh = vh * escala

    this.ctx.save()
    this.ctx.beginPath()
    if (this.avatar.forma === 'circulo') {
      this.ctx.arc(cx, cy, lado / 2, 0, Math.PI * 2)
    } else {
      const r = lado * 0.08
      const x = cx - lado / 2
      const y = cy - lado / 2
      this.ctx.roundRect(x, y, lado, lado, r)
    }
    this.ctx.closePath()
    this.ctx.clip()
    // Espelhado, como a pessoa se vê na pré-visualização.
    this.ctx.translate(cx + dw / 2, cy - dh / 2)
    this.ctx.scale(-1, 1)
    this.ctx.drawImage(this.camaraVideo, 0, 0, dw, dh)
    this.ctx.restore()

    // Aro: separa a bolha de um slide branco, que sem isto a engole.
    this.ctx.save()
    this.ctx.beginPath()
    if (this.avatar.forma === 'circulo') this.ctx.arc(cx, cy, lado / 2, 0, Math.PI * 2)
    else this.ctx.roundRect(cx - lado / 2, cy - lado / 2, lado, lado, lado * 0.08)
    this.ctx.strokeStyle = 'rgba(255,255,255,0.55)'
    this.ctx.lineWidth = Math.max(2, lado * 0.012)
    this.ctx.stroke()
    this.ctx.restore()
  }

  // ---------------------------------------------------------------- gravação

  /**
   * Começa a gravar. `micDeviceId` opcional; o áudio do ecrã entra sozinho se
   * o utilizador o tiver autorizado no seletor.
   */
  /**
   * Monta o fluxo composto: a imagem do canvas mais o áudio misturado.
   *
   * Existe separado porque a GRAVAÇÃO e o DIRECTO precisam exactamente do
   * mesmo fluxo. Duplicá-lo daria duas montagens que divergiriam à primeira
   * correcção feita só numa — e o áudio é o sítio onde isso doeria: a fonte
   * silenciosa abaixo é uma armadilha que já custou uma gravação vazia.
   */
  async montarFluxo(micDeviceId?: string): Promise<MediaStream> {
    this.consumidores++
    if (this.fluxoComposto) return this.fluxoComposto
    const fps = this.opcoes.fps ?? 30
    const stream = this.canvas.captureStream(fps)

    this.audioCtx = new AudioContext()
    this.destino = this.audioCtx.createMediaStreamDestination()
    // Fonte silenciosa sempre ligada: um destino sem entradas não produz
    // amostras e o muxer do MediaRecorder bloqueia — a gravação sai vazia.
    // É a mesma armadilha que o MeetingRecorder documenta.
    const silencio = this.audioCtx.createConstantSource()
    silencio.offset.value = 0
    silencio.connect(this.destino)
    silencio.start()

    try {
      const mic = await navigator.mediaDevices.getUserMedia({
        audio: { deviceId: micDeviceId ? { exact: micDeviceId } : undefined, echoCancellation: true, noiseSuppression: true },
        video: false,
      })
      this.ligarAudio(mic)
    } catch {
      // Sem microfone grava-se na mesma — só com o som do ecrã, se houver.
    }
    if (this.ecraStream && this.ecraStream.getAudioTracks().length) {
      this.ligarAudio(new MediaStream(this.ecraStream.getAudioTracks()))
    }
    for (const t of this.destino.stream.getAudioTracks()) stream.addTrack(t)
    this.fluxoComposto = stream
    this.iniciarPreVisualizacao()
    return stream
  }

  async iniciarGravacao(micDeviceId?: string): Promise<void> {
    if (this.gravador) return
    const stream = await this.montarFluxo(micDeviceId)

    const mime = MediaRecorder.isTypeSupported('video/webm;codecs=vp9,opus')
      ? 'video/webm;codecs=vp9,opus'
      : MediaRecorder.isTypeSupported('video/webm;codecs=vp8,opus')
        ? 'video/webm;codecs=vp8,opus'
        : 'video/webm'
    this.pedacos = []
    this.pedacosVideo = []
    this.pedacosAudio = []
    const bitrate = this.opcoes.bitrate ?? 6_000_000
    this.gravador = new MediaRecorder(stream, { mimeType: mime, videoBitsPerSecond: bitrate })
    this.gravador.ondataavailable = (e) => e.data.size && this.pedacos.push(e.data)
    this.gravador.start(1000)

    // Faixas isoladas. Partilham as MESMAS tracks do combinado, por isso não
    // há segunda captura de canvas nem segunda mistura de áudio — o custo
    // extra é o do encoder, não o da composição.
    const soVideo = new MediaStream(stream.getVideoTracks())
    this.gravadorVideo = new MediaRecorder(soVideo, { mimeType: mime, videoBitsPerSecond: bitrate })
    this.gravadorVideo.ondataavailable = (e) => e.data.size && this.pedacosVideo.push(e.data)
    this.gravadorVideo.start(1000)

    const faixasAudio = stream.getAudioTracks()
    if (faixasAudio.length) {
      const soAudio = new MediaStream(faixasAudio)
      const mimeA = MediaRecorder.isTypeSupported('audio/webm;codecs=opus') ? 'audio/webm;codecs=opus' : 'audio/webm'
      this.gravadorAudio = new MediaRecorder(soAudio, { mimeType: mimeA, audioBitsPerSecond: 128_000 })
      this.gravadorAudio.ondataavailable = (e) => e.data.size && this.pedacosAudio.push(e.data)
      this.gravadorAudio.start(1000)
    }
    this.inicioMs = Date.now()
    this.iniciarPreVisualizacao()
  }

  private ligarAudio(s: MediaStream): void {
    if (!this.audioCtx || !this.destino) return
    const n = this.audioCtx.createMediaStreamSource(s)
    n.connect(this.destino)
    this.fontesAudio.push(n)
  }

  private get todos(): MediaRecorder[] {
    return [this.gravador, this.gravadorVideo, this.gravadorAudio].filter(Boolean) as MediaRecorder[]
  }
  pausar(): void {
    for (const g of this.todos) if (g.state === 'recording') g.pause()
  }
  retomar(): void {
    for (const g of this.todos) if (g.state === 'paused') g.resume()
  }
  get aGravar(): boolean {
    return this.gravador?.state === 'recording'
  }
  get emPausa(): boolean {
    return this.gravador?.state === 'paused'
  }

  /** Termina e devolve as faixas. `null` se não havia nada a gravar. */
  async terminarGravacao(): Promise<ResultadoDaGravacao | null> {
    const g = this.gravador
    if (!g) return null
    const parar = (r: MediaRecorder | null) =>
      r
        ? new Promise<void>((res) => {
            r.onstop = () => res()
            r.stop()
          })
        : Promise.resolve()
    await Promise.all([parar(this.gravador), parar(this.gravadorVideo), parar(this.gravadorAudio)])
    this.gravador = null
    this.gravadorVideo = null
    this.gravadorAudio = null
    this.inicioMs = 0
    await this.largarFluxo()
    if (!this.pedacos.length) return null
    const tipo = g.mimeType || 'video/webm'
    return {
      completo: new Blob(this.pedacos, { type: tipo }),
      video: this.pedacosVideo.length ? new Blob(this.pedacosVideo, { type: tipo }) : null,
      audio: this.pedacosAudio.length ? new Blob(this.pedacosAudio, { type: 'audio/webm' }) : null,
    }
  }

  /**
   * Um consumidor larga o fluxo. O áudio só se desmonta quando sai o ÚLTIMO —
   * senão parar a gravação emudecia um directo a decorrer.
   */
  async largarFluxo(): Promise<void> {
    this.consumidores = Math.max(0, this.consumidores - 1)
    if (this.consumidores > 0) return
    for (const n of this.fontesAudio) n.disconnect()
    this.fontesAudio = []
    await this.audioCtx?.close().catch(() => {})
    this.audioCtx = null
    this.destino = null
    this.fluxoComposto = null
  }

  // ---------------------------------------------------------------- limpeza

  private pararEcra(): void {
    this.ecraStream?.getTracks().forEach((t) => t.stop())
    this.ecraStream = null
  }
  private pararCamara(): void {
    this.camaraStream?.getTracks().forEach((t) => t.stop())
    this.camaraStream = null
  }

  destruir(): void {
    this.vivo = false
    cancelAnimationFrame(this.raf)
    this.raf = 0
    for (const g of this.todos) {
      try {
        g.stop()
      } catch {
        /* já parado */
      }
    }
    this.gravador = null
    this.gravadorVideo = null
    this.gravadorAudio = null
    this.pararEcra()
    this.pararCamara()
    for (const n of this.fontesAudio) n.disconnect()
    this.fontesAudio = []
    void this.audioCtx?.close().catch(() => {})
    this.audioCtx = null
  }
}
