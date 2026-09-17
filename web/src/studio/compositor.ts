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

import type { Fonte } from '../room/compositor'
import {
  type ConteudoDoPalco,
  ganhoValido,
  type LayoutDoPalco,
  MISTURA_INICIAL,
  type Mistura,
  type PerfilDeQualidade,
  QUALIDADES,
  rectsDoLayout,
  SOBREPOSICOES_INICIAIS,
  type Sobreposicoes,
} from './palco'
import { desenharCartaoDeMarca, desenharSobreposicoes, type MarcaDoPalco, type SondagemNoPalco } from './desenho'
import { desenharQuadroDaMesa, type FontesParaDesenho } from './tv/desenhoDaMesa'
import type { QuadroDaMesa } from './tv/mesa'

/**
 * A mesa de corte ligada ao compositor. Com ela, o palco deixa de ser «ecrã +
 * bolha» e passa a ser o PROGRAMA da mesa (a fonte ou o plano no ar, e a
 * transição a decorrer). As sobreposições, a gravação e o directo continuam
 * a ser as do compositor — a mesa escolhe a imagem, não reinventa a saída.
 */
export interface MesaNoCompositor {
  fontes: FontesParaDesenho
  quadro(agora: number): QuadroDaMesa
}

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

/**
 * Iluminação e imagem sobre a TUA câmara (nunca sobre ecrã, quadro ou
 * convidados — cada um tem a sua própria fonte, sem ganho artificial). As
 * três correm de -50 a 50, centradas em 0 = sem correcção; `filtroCss`
 * converte para o que o `canvas 2d` entende.
 */
export interface EstadoDaImagem {
  brilho: number
  contraste: number
  saturacao: number
}

export const IMAGEM_INICIAL: EstadoDaImagem = { brilho: 0, contraste: 0, saturacao: 0 }

/** -50..50 → 50%..150%, o intervalo que `brightness()`/`contrast()`/`saturate()` esperam. */
export function filtroCss(f: EstadoDaImagem): string {
  const pct = (v: number) => `${Math.round(100 + Math.max(-50, Math.min(50, v)))}%`
  if (f.brilho === 0 && f.contraste === 0 && f.saturacao === 0) return 'none'
  return `brightness(${pct(f.brilho)}) contrast(${pct(f.contraste)}) saturate(${pct(f.saturacao)})`
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

/**
 * As mesmas constraints, para um perfil de qualidade (2160p, 50 fps). O de
 * omissão continua a ser o `ECRA_PARA_GRAVACAO` acima: 1080p a 30.
 */
export function ecraParaGravacao(perfil: PerfilDeQualidade): DisplayMediaStreamOptions {
  if (perfil.altura === 1080 && perfil.fps === 30) return ECRA_PARA_GRAVACAO
  return {
    ...ECRA_PARA_GRAVACAO,
    video: {
      width: { ideal: perfil.largura, max: perfil.largura },
      height: { ideal: perfil.altura, max: perfil.altura },
      frameRate: { ideal: perfil.fps, max: perfil.fps },
    },
  }
}

const MARGEM = 0.03 // fracção da largura, entre a bolha e a borda

/** O que sai de uma gravação: o ficheiro pronto e as duas faixas isoladas. */
export interface ResultadoDaGravacao {
  completo: Blob
  video: Blob | null
  audio: Blob | null
  /** Câmara e ecrã em bruto, cada um no seu ficheiro — clipes próprios no editor. */
  camara?: Blob | null
  ecra?: Blob | null
}

export interface OpcoesDoCompositor {
  largura?: number
  altura?: number
  fps?: number
  bitrate?: number
}

/** Um convidado no palco: o vídeo que se desenha e a fonte que entra na mistura. */
interface ConvidadoNoPalco {
  nome: string
  stream: MediaStream | null
  video: HTMLVideoElement
  audio: MediaStreamAudioSourceNode | null
  /** Fader próprio, entre `audio` e `ganhoPalco` — existe só enquanto o grafo existe. */
  gain: GainNode | null
  /** O que o fader deve valer mal o grafo (re)nasça — sobrevive a entrar/sair do palco. */
  ganhoAlvo: number
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
   * Os três barramentos da mistura. Cada fonte liga-se a UM ganho, e é o
   * ganho que vai ao destino — mexer num fader não reconstrói o grafo nem
   * interrompe a gravação.
   */
  private ganhoPalco: GainNode | null = null
  private ganhoMusica: GainNode | null = null
  private ganhoVideo: GainNode | null = null
  /**
   * Nivelador + limitador da MISTURA inteira (os três barramentos somados),
   * não de uma fonte só — os mesmos valores do `Denoiser` (media.ts), a única
   * cadeia de dinâmica já revista neste código, aplicados aqui para nenhum
   * barramento sozinho conseguir saturar o que se grava ou emite.
   */
  private compressorMestre: DynamicsCompressorNode | null = null
  private limiterMestre: DynamicsCompressorNode | null = null
  /** Alimentado pela SAÍDA do limitador — o pico que `lerPicoMestre` lê. */
  private analiserMestre: AnalyserNode | null = null
  private mistura: Mistura = { ...MISTURA_INICIAL }
  private micStream: MediaStream | null = null
  /**
   * O som da MESA DE SOM, quando ligada. Substitui o microfone e os convidados
   * na mistura (eles já passam pela mesa, com EQ e dinâmica) — ligar os dois
   * dava cada voz em dobro, uma delas atrasada. A música e o som do ecrã
   * continuam nos barramentos do compositor.
   */
  private somExterno: MediaStream | null = null
  private somExternoFonte: MediaStreamAudioSourceNode | null = null
  private micFonte: MediaStreamAudioSourceNode | null = null
  private ecraFonte: MediaStreamAudioSourceNode | null = null
  /** Microfone escolhido; vazio = o de omissão do sistema. */
  microfoneId = ''
  /** Câmara escolhida; vazio = a de omissão do sistema. */
  camaraId = ''

  private musicaUrl: string | null = null
  private musicaEl: HTMLAudioElement | null = null
  private musicaFonte: MediaElementAudioSourceNode | null = null
  private musicaNoGrafo = false
  /** Chamado quando a música começa ou pára (fim, erro, troca). */
  aoMudarMusica: ((aTocar: boolean) => void) | null = null

  private convidados = new Map<string, ConvidadoNoPalco>()

  /** O que enche o palco e como se arruma. Lidos a cada frame. */
  layout: LayoutDoPalco = 'solo'
  conteudo: ConteudoDoPalco = 'fontes'
  /**
   * Em `layout: 'solo'`, qual convidado vai para o ecrã inteiro — o «corte»
   * da mesa de corte. `null`, ou um id que já não está no palco, cai para o
   * mais antigo ainda montado (o comportamento de sempre, antes de existir
   * escolha do operador).
   */
  programaId: string | null = null
  sobreposicoes: Sobreposicoes = { ...SOBREPOSICOES_INICIAIS }
  /** Momento em que o rodapé foi ligado — dá a animação de entrada. */
  rodapeDesde = 0
  /** Momento em que o cronómetro foi ligado. */
  cronometroDesde = 0
  /** A última frase das legendas ao vivo; vazia = nada no ecrã. */
  legenda = ''
  sondagem: SondagemNoPalco | null = null
  marca: MarcaDoPalco = { nome: '', titulo: '', aviso: '', deOrigem: true, logo: null }
  /** O quadro local: um canvas branco onde se escreve com o rato ou o dedo. */
  readonly quadro = document.createElement('canvas')

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
  private bytesCompleto = 0
  /**
   * Câmara e ecrã em BRUTO (antes da composição), para o editor os ter como
   * clipes separados em V1/V2. Custam um codificador cada; desliga-se com
   * `gravarFontesSeparadas = false` numa máquina que não aguente.
   */
  gravarFontesSeparadas = true
  private gravadorCamara: MediaRecorder | null = null
  private gravadorEcra: MediaRecorder | null = null
  private pedacosCamara: Blob[] = []
  private pedacosEcra: Blob[] = []
  private raf = 0
  private vivo = true

  private ecraStream: MediaStream | null = null
  private camaraStream: MediaStream | null = null

  recorte: Recorte = { ...RECORTE_INTEIRO }
  avatar: EstadoDoAvatar = { ...AVATAR_INICIAL }
  imagem: EstadoDaImagem = { ...IMAGEM_INICIAL }

  /**
   * Fonte da pessoa recortada, quando o modo `recorte` está ligado. É um
   * canvas com alfa vindo do `BackgroundEffect` — quem liga o efeito é a
   * página, para o compositor não ter de conhecer o pipeline de segmentação.
   */
  pessoaComAlfa: CanvasImageSource | null = null

  /** A mesa de corte, quando está ligada (ver `MesaNoCompositor`). */
  mesa: MesaNoCompositor | null = null
  private camadaDaMesa: CanvasRenderingContext2D | null = null
  /** Quanto custou compor o último frame, em ms — medido, não estimado. */
  custoDoFrameMs = 0

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
    this.quadro.width = this.canvas.width
    this.quadro.height = this.canvas.height
    this.limparQuadro()
    for (const v of [this.ecraVideo, this.camaraVideo]) {
      v.muted = true
      v.playsInline = true
    }
  }

  // ---------------------------------------------------------------- fontes

  /** Abre o seletor do browser (ecrã inteiro / janela / separador). */
  async escolherEcra(): Promise<void> {
    const s = await navigator.mediaDevices.getDisplayMedia(ecraParaGravacao(this.perfil))
    this.pararEcra()
    this.ecraStream = s
    this.ecraVideo.srcObject = s
    await this.ecraVideo.play().catch(() => {})
    // Com a mistura já montada (a gravar ou no ar), o som do ecrã novo entra
    // já — antes só entrava no próximo arranque, e trocar de separador a meio
    // de uma aula emudecia o vídeo sem aviso.
    this.ligarSomDoEcra()
    // O utilizador pode parar a partilha pelo aviso do browser: isso é um fim
    // de fonte, não um erro — quem usa o compositor decide o que fazer.
    s.getVideoTracks()[0]?.addEventListener('ended', () => this.aoPerderEcra?.())
  }

  /** Chamado quando o utilizador pára a partilha pelo aviso do browser. */
  aoPerderEcra: (() => void) | null = null

  async ligarCamara(): Promise<void> {
    const s = await navigator.mediaDevices.getUserMedia({
      video: {
        deviceId: this.camaraId ? { exact: this.camaraId } : undefined,
        width: { ideal: this.perfil.altura > 1080 ? 1920 : 1280 },
        height: { ideal: this.perfil.altura > 1080 ? 1080 : 720 },
      },
      audio: false,
    })
    this.pararCamara()
    this.camaraStream = s
    this.camaraVideo.srcObject = s
    await this.camaraVideo.play().catch(() => {})
  }

  /** Troca de câmara — se estiver ligada, reabre já na nova; senão só memoriza a escolha. */
  async trocarCamara(deviceId: string): Promise<void> {
    this.camaraId = deviceId
    if (this.temCamara) await this.ligarCamara()
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
  /** O vídeo do ecrã partilhado, para a mesa o ter como fonte. `null` sem ecrã. */
  get videoDoEcra(): HTMLVideoElement | null {
    return this.temEcra ? this.ecraVideo : null
  }
  get fluxoDoEcra(): MediaStream | null {
    return this.ecraStream
  }
  /** Dimensões do ecrã capturado — precisas para o seletor de recorte. */
  get dimensoesDoEcra(): { w: number; h: number } {
    return { w: this.ecraVideo.videoWidth, h: this.ecraVideo.videoHeight }
  }

  // ---------------------------------------------------------------- qualidade

  /** O perfil em uso (tamanho do canvas, fps e débito da gravação). */
  get perfil(): PerfilDeQualidade {
    return {
      largura: this.canvas.width,
      altura: this.canvas.height,
      fps: this.opcoes.fps ?? 30,
      bitrate: this.opcoes.bitrate ?? QUALIDADES['1080p30'].bitrate,
    }
  }

  /**
   * Muda a qualidade. RECUSA a meio de uma gravação ou de um directo: o fluxo
   * já foi capturado com o tamanho e os fps antigos, e trocar o canvas por
   * baixo dele dava um ficheiro com duas resoluções. Devolve se mudou.
   */
  definirQualidade(p: PerfilDeQualidade): boolean {
    if (this.consumidores > 0) return false
    this.opcoes = { ...this.opcoes, largura: p.largura, altura: p.altura, fps: p.fps, bitrate: p.bitrate }
    if (this.canvas.width !== p.largura || this.canvas.height !== p.altura) {
      // O quadro acompanha, com o que já tinha escrito esticado para o novo tamanho.
      const antigo = document.createElement('canvas')
      antigo.width = this.quadro.width
      antigo.height = this.quadro.height
      antigo.getContext('2d')?.drawImage(this.quadro, 0, 0)
      this.canvas.width = p.largura
      this.canvas.height = p.altura
      this.quadro.width = p.largura
      this.quadro.height = p.altura
      this.limparQuadro()
      this.quadro.getContext('2d')?.drawImage(antigo, 0, 0, p.largura, p.altura)
    }
    return true
  }

  /** Bytes do ficheiro completo gravados até agora (a gravação em curso). */
  get bytesGravados(): number {
    return this.bytesCompleto
  }

  // ---------------------------------------------------------------- convidados

  /**
   * Os convidados que estão NO PALCO (vêm da sala ligada ao Estúdio). Segue a
   * lista de forma incremental, como o `RoomCompositor`: quem entra ganha um
   * vídeo e uma fonte na mistura, quem sai larga os dois — sem recriar o
   * `AudioContext`, que emudeceria a gravação.
   */
  definirConvidados(fontes: Fonte[]): void {
    const ids = new Set(fontes.map((f) => f.id))
    for (const [id, c] of this.convidados) {
      if (ids.has(id)) continue
      c.video.pause()
      c.video.srcObject = null
      c.audio?.disconnect()
      c.gain?.disconnect()
      this.convidados.delete(id)
    }
    for (const f of fontes) {
      let c = this.convidados.get(f.id)
      if (!c) {
        const video = document.createElement('video')
        video.muted = true
        video.playsInline = true
        c = { nome: f.nome, stream: null, video, audio: null, gain: null, ganhoAlvo: 1 }
        this.convidados.set(f.id, c)
      }
      c.nome = f.nome
      if (c.stream !== f.stream) {
        c.audio?.disconnect()
        c.audio = null
        c.stream = f.stream
        c.video.srcObject = f.stream
        void c.video.play().catch(() => {})
      }
      this.ligarConvidadoAoGrafo(c)
    }
  }

  get quantosConvidados(): number {
    return this.convidados.size
  }

  private ligarConvidadoAoGrafo(c: ConvidadoNoPalco): void {
    if (!this.audioCtx || !this.ganhoPalco) return
    // O fader do convidado nasce uma vez por geração do grafo e sobrevive a
    // uma troca de stream (reconexão) — só o `audio` (a fonte) é que se
    // recria; recriar o `gain` também apagaria a posição do fader sem razão.
    if (!c.gain) {
      c.gain = this.audioCtx.createGain()
      c.gain.gain.value = c.ganhoAlvo
      c.gain.connect(this.ganhoPalco)
    }
    if (c.audio || this.somExterno || !c.stream?.getAudioTracks().length) return
    c.audio = this.audioCtx.createMediaStreamSource(new MediaStream(c.stream.getAudioTracks()))
    c.audio.connect(c.gain)
  }

  /** Fader de UM convidado, entre a sua fonte e o barramento "Palco". */
  definirGanhoConvidado(id: string, v: number): void {
    const c = this.convidados.get(id)
    if (!c) return
    c.ganhoAlvo = ganhoValido(v, 1)
    const agora = this.audioCtx?.currentTime ?? 0
    c.gain?.gain.setTargetAtTime(c.ganhoAlvo, agora, 0.02)
  }

  // ---------------------------------------------------------------- quadro

  limparQuadro(): void {
    const g = this.quadro.getContext('2d')
    if (!g) return
    g.fillStyle = '#ffffff'
    g.fillRect(0, 0, this.quadro.width, this.quadro.height)
  }

  /** Um traço em fracções (0–1) do quadro. A espessura é em fracção da altura. */
  riscarNoQuadro(de: [number, number], ate: [number, number], cor: string, espessura: number): void {
    const g = this.quadro.getContext('2d')
    if (!g) return
    const { width: W, height: H } = this.quadro
    g.strokeStyle = cor
    g.lineWidth = Math.max(1, espessura * H)
    g.lineCap = 'round'
    g.lineJoin = 'round'
    g.beginPath()
    g.moveTo(de[0] * W, de[1] * H)
    g.lineTo(ate[0] * W, ate[1] * H)
    g.stroke()
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
    const t0 = performance.now()
    const { width: W, height: H } = this.canvas
    this.ctx.fillStyle = '#0d1117'
    this.ctx.fillRect(0, 0, W, H)

    if (this.conteudo === 'marca') {
      desenharCartaoDeMarca(this.ctx, this.marca, W, H)
    } else if (this.mesa && this.conteudo === 'fontes') {
      this.camadaDaMesa ??= document.createElement('canvas').getContext('2d', { alpha: false })
      if (this.camadaDaMesa) {
        desenharQuadroDaMesa(this.ctx, this.camadaDaMesa, this.mesa.quadro(Date.now()), this.mesa.fontes, this.marca, W, H)
      }
    } else if (this.layout === 'solo') {
      // O conteúdo a ecrã inteiro e a bolha por cima — o Estúdio de sempre.
      const todo = { x: 0, y: 0, w: W, h: H }
      if (this.conteudo === 'quadro') this.desenharConteudo(this.quadro, todo, 'contain')
      else if (this.temEcra) this.desenharEcraEm(todo)
      else {
        const alvo = (this.programaId && this.convidados.get(this.programaId)) || [...this.convidados.values()][0]
        if (alvo) this.desenharConvidado(alvo, todo)
      }
      if (this.avatar.visivel && this.temCamara) this.desenharAvatar(W, H)
    } else {
      this.desenharMosaico(W, H)
    }

    desenharSobreposicoes(this.ctx, {
      sobreposicoes: this.sobreposicoes,
      rodapeDesde: this.rodapeDesde,
      cronometroDesde: this.cronometroDesde,
      legenda: this.legenda,
      sondagem: this.sondagem,
      marca: this.marca,
    }, W, H)
    this.custoDoFrameMs = performance.now() - t0
  }

  /** As fontes pela ordem em que ocupam os lugares: conteúdo, tu, convidados. */
  private desenharMosaico(W: number, H: number): void {
    const pecas: ((r: { x: number; y: number; w: number; h: number }) => void)[] = []
    if (this.conteudo === 'quadro') pecas.push((r) => this.desenharConteudo(this.quadro, r, 'contain'))
    else if (this.temEcra) pecas.push((r) => this.desenharEcraEm(r))
    if (this.avatar.visivel && this.temCamara) pecas.push((r) => this.desenharCamaraEm(r))
    for (const c of this.convidados.values()) pecas.push((r) => this.desenharConvidado(c, r))
    const rects = rectsDoLayout(this.layout, pecas.length, W, H)
    const pad = rects.length > 1 ? Math.round(H * 0.004) : 0
    rects.forEach((r, i) => {
      const dentro = { x: r.x + pad, y: r.y + pad, w: r.w - pad * 2, h: r.h - pad * 2 }
      pecas[i](dentro)
    })
  }

  private desenharEcraEm(r: { x: number; y: number; w: number; h: number }): void {
    const vw = this.ecraVideo.videoWidth
    const vh = this.ecraVideo.videoHeight
    const sx = this.recorte.x * vw
    const sy = this.recorte.y * vh
    const sw = Math.max(1, this.recorte.w * vw)
    const sh = Math.max(1, this.recorte.h * vh)
    // `contain`: a região escolhida cabe inteira, com barras se o rácio
    // não bater certo. Cortar aqui seria recortar duas vezes — o utilizador
    // já escolheu o que quer ver.
    const escala = Math.min(r.w / sw, r.h / sh)
    const dw = sw * escala
    const dh = sh * escala
    this.ctx.drawImage(this.ecraVideo, sx, sy, sw, sh, r.x + (r.w - dw) / 2, r.y + (r.h - dh) / 2, dw, dh)
  }

  /** Uma imagem num rectângulo: `contain` (conteúdo) ou `cover` (pessoas). */
  private desenharConteudo(
    fonte: CanvasImageSource & { width?: number; height?: number; videoWidth?: number; videoHeight?: number },
    r: { x: number; y: number; w: number; h: number },
    modo: 'contain' | 'cover',
    espelhar = false,
  ): void {
    const fw = (fonte.videoWidth ?? fonte.width) as number
    const fh = (fonte.videoHeight ?? fonte.height) as number
    if (!fw || !fh) {
      this.ctx.fillStyle = '#1a1d24'
      this.ctx.fillRect(r.x, r.y, r.w, r.h)
      return
    }
    const escala = modo === 'contain' ? Math.min(r.w / fw, r.h / fh) : Math.max(r.w / fw, r.h / fh)
    const dw = fw * escala
    const dh = fh * escala
    this.ctx.save()
    this.ctx.beginPath()
    this.ctx.rect(r.x, r.y, r.w, r.h)
    this.ctx.clip()
    if (espelhar) {
      this.ctx.translate(r.x + r.w / 2 + dw / 2, r.y + (r.h - dh) / 2)
      this.ctx.scale(-1, 1)
      this.ctx.drawImage(fonte, 0, 0, dw, dh)
    } else {
      this.ctx.drawImage(fonte, r.x + (r.w - dw) / 2, r.y + (r.h - dh) / 2, dw, dh)
    }
    this.ctx.restore()
  }

  private desenharCamaraEm(r: { x: number; y: number; w: number; h: number }): void {
    this.ctx.save()
    this.ctx.filter = filtroCss(this.imagem)
    this.desenharConteudo(this.camaraVideo, r, 'cover', true)
    this.ctx.restore()
  }

  private desenharConvidado(c: ConvidadoNoPalco, r: { x: number; y: number; w: number; h: number }): void {
    if (c.video.readyState >= 2 && c.video.videoWidth > 0) this.desenharConteudo(c.video, r, 'cover')
    else {
      this.ctx.fillStyle = '#1a1d24'
      this.ctx.fillRect(r.x, r.y, r.w, r.h)
    }
    if (!c.nome) return
    const H = this.canvas.height
    const tam = Math.max(14, Math.round(H * 0.022))
    this.ctx.font = `600 ${tam}px Archivo, system-ui, sans-serif`
    const largura = Math.min(r.w - tam, this.ctx.measureText(c.nome).width + tam)
    this.ctx.fillStyle = 'rgba(0,0,0,0.6)'
    this.ctx.fillRect(r.x + tam * 0.5, r.y + r.h - tam * 2.1, largura, tam * 1.6)
    this.ctx.fillStyle = '#ffffff'
    this.ctx.fillText(c.nome, r.x + tam, r.y + r.h - tam * 0.9, Math.max(0, largura - tam))
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
        this.ctx.filter = filtroCss(this.imagem)
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
    this.ctx.filter = filtroCss(this.imagem)
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

    this.ganhoPalco = this.audioCtx.createGain()
    this.ganhoMusica = this.audioCtx.createGain()
    this.ganhoVideo = this.audioCtx.createGain()
    this.ganhoPalco.gain.value = this.mistura.palco
    this.ganhoMusica.gain.value = this.mistura.musica
    this.ganhoVideo.gain.value = this.mistura.video

    // Valores iguais aos do `Denoiser` (media.ts) — nivelador suave, depois
    // um limitador com o cotovelo a zero mesmo antes do tecto (-3 dBFS).
    this.compressorMestre = this.audioCtx.createDynamicsCompressor()
    this.compressorMestre.threshold.value = -24
    this.compressorMestre.knee.value = 12
    this.compressorMestre.ratio.value = 3
    this.compressorMestre.attack.value = 0.01
    this.compressorMestre.release.value = 0.25
    this.limiterMestre = this.audioCtx.createDynamicsCompressor()
    this.limiterMestre.threshold.value = -3
    this.limiterMestre.knee.value = 0
    this.limiterMestre.ratio.value = 20
    this.limiterMestre.attack.value = 0.003
    this.limiterMestre.release.value = 0.1
    this.analiserMestre = this.audioCtx.createAnalyser()
    this.analiserMestre.fftSize = 512

    for (const g of [this.ganhoPalco, this.ganhoMusica, this.ganhoVideo]) g.connect(this.compressorMestre)
    this.compressorMestre.connect(this.limiterMestre)
    this.limiterMestre.connect(this.destino)
    this.limiterMestre.connect(this.analiserMestre)

    if (micDeviceId !== undefined) this.microfoneId = micDeviceId
    await this.ligarMicrofone()
    this.ligarSomDoEcra()
    for (const c of this.convidados.values()) this.ligarConvidadoAoGrafo(c)
    this.ligarMusicaAoGrafo()
    for (const t of this.destino.stream.getAudioTracks()) stream.addTrack(t)
    this.fluxoComposto = stream
    this.iniciarPreVisualizacao()
    return stream
  }

  // ---------------------------------------------------------------- mistura

  /** Muda um fader. Aplica-se já ao grafo, se existir, com uma rampa curta (sem estalidos). */
  definirMistura(m: Mistura): void {
    this.mistura = { ...m }
    const agora = this.audioCtx?.currentTime ?? 0
    this.ganhoPalco?.gain.setTargetAtTime(m.palco, agora, 0.02)
    this.ganhoMusica?.gain.setTargetAtTime(m.musica, agora, 0.02)
    this.ganhoVideo?.gain.setTargetAtTime(m.video, agora, 0.02)
    // Sem grafo, a música ouve-se na pré-escuta ao volume do fader.
    if (this.musicaEl && !this.musicaNoGrafo) this.musicaEl.volume = Math.min(1, m.musica)
  }

  /** O fluxo do microfone que entra na mistura — as legendas ouvem este. */
  get fluxoDoMicrofone(): MediaStream | null {
    return this.somExterno ?? this.micStream
  }

  /** Liga (ou desliga, com `null`) o som da mesa de som. Não reconstrói o grafo. */
  definirSomExterno(s: MediaStream | null): void {
    if (s === this.somExterno) return
    this.somExterno = s
    this.somExternoFonte?.disconnect()
    this.somExternoFonte = null
    if (!this.audioCtx || !this.ganhoPalco) return
    if (s) {
      this.micFonte?.disconnect()
      this.micFonte = null
      this.micStream?.getTracks().forEach((t) => t.stop())
      this.micStream = null
      for (const c of this.convidados.values()) {
        c.audio?.disconnect()
        c.audio = null
      }
      this.ligarSomExterno()
    } else {
      void this.ligarMicrofone()
      for (const c of this.convidados.values()) this.ligarConvidadoAoGrafo(c)
    }
  }

  private ligarSomExterno(): void {
    if (!this.audioCtx || !this.ganhoPalco || !this.somExterno?.getAudioTracks().length || this.somExternoFonte) return
    this.somExternoFonte = this.audioCtx.createMediaStreamSource(this.somExterno)
    this.somExternoFonte.connect(this.ganhoPalco)
  }

  /** Troca o microfone. A meio de uma gravação troca a fonte, não o grafo. */
  async trocarMicrofone(deviceId: string): Promise<void> {
    this.microfoneId = deviceId
    if (this.audioCtx) await this.ligarMicrofone()
  }

  private async ligarMicrofone(): Promise<void> {
    if (!this.audioCtx || !this.ganhoPalco) return
    if (this.somExterno) {
      this.ligarSomExterno()
      return
    }
    try {
      const mic = await navigator.mediaDevices.getUserMedia({
        audio: {
          deviceId: this.microfoneId ? { exact: this.microfoneId } : undefined,
          echoCancellation: true,
          noiseSuppression: true,
        },
        video: false,
      })
      // A mesa de som pode ter sido ligada enquanto o browser pedia o microfone.
      if (!this.audioCtx || !this.ganhoPalco || this.somExterno) {
        mic.getTracks().forEach((t) => t.stop())
        return
      }
      this.micFonte?.disconnect()
      this.micStream?.getTracks().forEach((t) => t.stop())
      this.micStream = mic
      this.micFonte = this.audioCtx.createMediaStreamSource(mic)
      this.micFonte.connect(this.ganhoPalco)
    } catch {
      // Sem microfone grava-se na mesma — só com o som do ecrã, se houver.
    }
  }

  private ligarSomDoEcra(): void {
    if (!this.audioCtx || !this.ganhoVideo) return
    this.ecraFonte?.disconnect()
    this.ecraFonte = null
    if (this.ecraStream && this.ecraStream.getAudioTracks().length) {
      this.ecraFonte = this.audioCtx.createMediaStreamSource(new MediaStream(this.ecraStream.getAudioTracks()))
      this.ecraFonte.connect(this.ganhoVideo)
    }
  }

  /** A música de fundo: um ficheiro do dispositivo, em ciclo. `null` tira-a. */
  definirMusica(ficheiro: Blob | null): void {
    this.pararMusica()
    this.musicaFonte?.disconnect()
    this.musicaFonte = null
    this.musicaEl = null
    this.musicaNoGrafo = false
    if (this.musicaUrl) URL.revokeObjectURL(this.musicaUrl)
    this.musicaUrl = ficheiro ? URL.createObjectURL(ficheiro) : null
  }

  get temMusica(): boolean {
    return !!this.musicaUrl
  }
  get musicaATocar(): boolean {
    return !!this.musicaEl && !this.musicaEl.paused
  }

  async tocarMusica(): Promise<void> {
    if (!this.musicaUrl) return
    if (!this.musicaEl) this.criarElementoDeMusica(0)
    this.ligarMusicaAoGrafo()
    await this.musicaEl?.play().catch(() => this.aoMudarMusica?.(false))
  }

  pararMusica(): void {
    this.musicaEl?.pause()
  }

  private criarElementoDeMusica(desde: number): void {
    if (!this.musicaUrl) return
    const el = new Audio(this.musicaUrl)
    el.loop = true
    el.currentTime = desde
    el.volume = Math.min(1, this.mistura.musica)
    el.onplay = () => this.aoMudarMusica?.(true)
    el.onpause = () => this.aoMudarMusica?.(false)
    this.musicaEl = el
    this.musicaNoGrafo = false
  }

  /**
   * Um `<audio>` só pode alimentar UM `MediaElementAudioSourceNode` na vida
   * inteira, e o grafo é recriado a cada gravação. Por isso, ao entrar num
   * grafo novo, a música muda para um elemento novo no mesmo ponto — em vez de
   * rebentar com `InvalidStateError` à segunda gravação.
   */
  private ligarMusicaAoGrafo(): void {
    if (!this.audioCtx || !this.ganhoMusica || !this.musicaUrl || !this.musicaEl || this.musicaNoGrafo) return
    const aTocar = !this.musicaEl.paused
    const ponto = this.musicaEl.currentTime
    this.musicaEl.onpause = null
    this.musicaEl.pause()
    this.criarElementoDeMusica(ponto)
    const el = this.musicaEl as HTMLAudioElement
    el.volume = 1
    this.musicaFonte = this.audioCtx.createMediaElementSource(el)
    this.musicaFonte.connect(this.ganhoMusica)
    this.musicaNoGrafo = true
    if (aTocar) void el.play().catch(() => {})
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
    this.bytesCompleto = 0
    const bitrate = this.opcoes.bitrate ?? 6_000_000
    this.gravador = new MediaRecorder(stream, { mimeType: mime, videoBitsPerSecond: bitrate })
    this.gravador.ondataavailable = (e) => {
      if (!e.data.size) return
      this.pedacos.push(e.data)
      this.bytesCompleto += e.data.size
    }
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

    this.pedacosCamara = []
    this.pedacosEcra = []
    if (this.gravarFontesSeparadas) {
      const soImagem = MediaRecorder.isTypeSupported('video/webm;codecs=vp9') ? 'video/webm;codecs=vp9' : 'video/webm'
      const camara = this.camaraStream?.getVideoTracks() ?? []
      if (camara.length) {
        this.gravadorCamara = new MediaRecorder(new MediaStream(camara), { mimeType: soImagem, videoBitsPerSecond: 1_500_000 })
        this.gravadorCamara.ondataavailable = (e) => e.data.size && this.pedacosCamara.push(e.data)
        this.gravadorCamara.start(1000)
      }
      const ecra = this.ecraStream?.getVideoTracks() ?? []
      if (ecra.length) {
        this.gravadorEcra = new MediaRecorder(new MediaStream(ecra), { mimeType: soImagem, videoBitsPerSecond: 2_500_000 })
        this.gravadorEcra.ondataavailable = (e) => e.data.size && this.pedacosEcra.push(e.data)
        this.gravadorEcra.start(1000)
      }
    }
    this.inicioMs = Date.now()
    this.iniciarPreVisualizacao()
  }

  private get todos(): MediaRecorder[] {
    return [this.gravador, this.gravadorVideo, this.gravadorAudio, this.gravadorCamara, this.gravadorEcra].filter(Boolean) as MediaRecorder[]
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
    await Promise.all([parar(this.gravador), parar(this.gravadorVideo), parar(this.gravadorAudio), parar(this.gravadorCamara), parar(this.gravadorEcra)])
    this.gravador = null
    this.gravadorVideo = null
    this.gravadorAudio = null
    this.gravadorCamara = null
    this.gravadorEcra = null
    this.inicioMs = 0
    await this.largarFluxo()
    if (!this.pedacos.length) return null
    const tipo = g.mimeType || 'video/webm'
    return {
      completo: new Blob(this.pedacos, { type: tipo }),
      video: this.pedacosVideo.length ? new Blob(this.pedacosVideo, { type: tipo }) : null,
      audio: this.pedacosAudio.length ? new Blob(this.pedacosAudio, { type: 'audio/webm' }) : null,
      camara: this.pedacosCamara.length ? new Blob(this.pedacosCamara, { type: 'video/webm' }) : null,
      ecra: this.pedacosEcra.length ? new Blob(this.pedacosEcra, { type: 'video/webm' }) : null,
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
    this.desmontarMistura()
    await this.audioCtx?.close().catch(() => {})
    this.audioCtx = null
    this.destino = null
    this.fluxoComposto = null
  }

  /** Larga as fontes do grafo que vai fechar; a música volta à pré-escuta. */
  private desmontarMistura(): void {
    this.somExternoFonte?.disconnect()
    this.somExternoFonte = null
    this.micFonte?.disconnect()
    this.micFonte = null
    this.micStream?.getTracks().forEach((t) => t.stop())
    this.micStream = null
    this.ecraFonte?.disconnect()
    this.ecraFonte = null
    for (const c of this.convidados.values()) {
      c.audio?.disconnect()
      c.audio = null
      c.gain?.disconnect()
      c.gain = null
    }
    if (this.musicaNoGrafo && this.musicaEl) {
      const aTocar = !this.musicaEl.paused
      const ponto = this.musicaEl.currentTime
      this.musicaEl.onpause = null
      this.musicaEl.pause()
      this.musicaFonte?.disconnect()
      this.musicaFonte = null
      this.criarElementoDeMusica(ponto)
      if (aTocar) void this.musicaEl?.play().catch(() => {})
    }
    this.ganhoPalco = null
    this.ganhoMusica = null
    this.ganhoVideo = null
    this.compressorMestre?.disconnect()
    this.compressorMestre = null
    this.limiterMestre?.disconnect()
    this.limiterMestre = null
    this.analiserMestre?.disconnect()
    this.analiserMestre = null
  }

  /**
   * Pico da MISTURA final (pós-limitador), em dBFS: -60 é silêncio, 0 é o
   * máximo. `-60` também é o valor de repouso quando o grafo não existe
   * (antes de gravar/emitir) — silêncio genuíno, não um erro.
   */
  lerPicoMestre(): number {
    if (!this.analiserMestre) return -60
    const buf = new Float32Array(this.analiserMestre.fftSize)
    this.analiserMestre.getFloatTimeDomainData(buf)
    let pico = 0
    for (let i = 0; i < buf.length; i++) pico = Math.max(pico, Math.abs(buf[i]))
    return pico > 0 ? Math.max(-60, 20 * Math.log10(pico)) : -60
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
    this.gravadorCamara = null
    this.gravadorEcra = null
    this.pararEcra()
    this.pararCamara()
    for (const n of this.fontesAudio) n.disconnect()
    this.fontesAudio = []
    this.desmontarMistura()
    this.musicaEl?.pause()
    this.musicaEl = null
    if (this.musicaUrl) URL.revokeObjectURL(this.musicaUrl)
    this.musicaUrl = null
    this.definirConvidados([])
    void this.audioCtx?.close().catch(() => {})
    this.audioCtx = null
  }
}
