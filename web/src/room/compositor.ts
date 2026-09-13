/**
 * Compositor multi-câmara da sala — pega nas fontes de vídeo (a local e as
 * dos participantes remotos, já ligadas via SFU) e compõe-nas num canvas
 * segundo uma cena escolhida, para ir em directo (`Directo`, ver
 * `studio/directo.ts`) ou gravar. É o irmão do `studio/compositor.ts`:
 * aquele compõe ecrã+câmara para uma aula solo; este compõe N câmaras para
 * um podcast/painel com convidados.
 *
 * PORQUÊ SEPARADO do `CompositorDeAula`: as fontes ali são FIXAS — uma vez
 * escolhidas, não mudam a meio de uma gravação. Aqui os participantes ENTRAM
 * E SAEM durante a emissão, porque é a SALA, não uma gravação solo.
 * `definirParticipantes()` chama-se sempre que a lista muda (a cada vez que
 * os peers do `Room.tsx` mudam), e o grafo de vídeo/áudio segue-a de forma
 * incremental — sem nunca recriar o `AudioContext` a meio, que é o que
 * emudeceria um directo a decorrer.
 *
 * O que é PURO e testável sem canvas nem `MediaStream` nenhum:
 * `participantesVisiveis` e `calcularRects`. É aí que vive a lógica que um
 * `git revert` distraído partiria — a classe à volta só liga essa
 * matemática a APIs de browser.
 */

export type Cena = 'solo' | 'lado-a-lado' | 'grelha'

export interface Rect {
  x: number
  y: number
  w: number
  h: number
}

export interface Fonte {
  id: string
  nome: string
  stream: MediaStream | null
}

/**
 * Quais fontes se veem, dada a cena e quem está em foco. Em `solo`/
 * `lado-a-lado`, se faltar gente no foco (ninguém escolhido, ou só um em
 * `lado-a-lado`), completa-se com quem sobrar — a emissão não pode ficar
 * com um ecrã vazio só porque o anfitrião ainda não escolheu ninguém.
 */
export function participantesVisiveis<T>(cena: Cena, todos: T[], focoIds: string[], idDe: (t: T) => string): T[] {
  if (cena === 'grelha') return todos
  const alvo = cena === 'solo' ? 1 : 2
  const escolhidos = focoIds
    .map((id) => todos.find((t) => idDe(t) === id))
    .filter((t): t is T => !!t)
    .slice(0, alvo)
  if (escolhidos.length >= alvo || todos.length === 0) return escolhidos
  const restantes = todos.filter((t) => !escolhidos.some((e) => idDe(e) === idDe(t)))
  return [...escolhidos, ...restantes].slice(0, alvo)
}

/**
 * Os rectângulos do canvas para N fontes visíveis. 1 = ecrã inteiro; 2 =
 * lado a lado; 3+ = grelha (colunas = raiz quadrada arredondada para cima,
 * como o `useGridLayout` da sala faz para os mosaicos reais).
 *
 * A ÚLTIMA linha estica para preencher a largura toda, em vez de deixar as
 * colunas que faltam como buracos pretos — 3 fontes numa grelha de raiz 2
 * dão uma linha de 2 e uma linha de 1 a ecrã inteiro, não uma linha de 2 e
 * um quadrado vazio. A área somada dos rectângulos bate sempre com a área
 * do canvas, para qualquer N — é a propriedade que os testes verificam.
 */
export function calcularRects(n: number, largura: number, altura: number): Rect[] {
  if (n <= 0) return []
  if (n === 1) return [{ x: 0, y: 0, w: largura, h: altura }]
  if (n === 2) {
    const w = largura / 2
    return [
      { x: 0, y: 0, w, h: altura },
      { x: w, y: 0, w, h: altura },
    ]
  }
  const cols = Math.ceil(Math.sqrt(n))
  const rows = Math.ceil(n / cols)
  const h = altura / rows
  const rects: Rect[] = []
  let restantes = n
  for (let linha = 0; linha < rows; linha++) {
    const colsNestaLinha = Math.min(cols, restantes)
    const w = largura / colsNestaLinha
    for (let c = 0; c < colsNestaLinha; c++) {
      rects.push({ x: c * w, y: linha * h, w, h })
    }
    restantes -= colsNestaLinha
  }
  return rects
}

/** Compõe e mistura. O desenho corre em `requestAnimationFrame`, como o do Estúdio. */
export class RoomCompositor {
  readonly canvas = document.createElement('canvas')
  private ctx: CanvasRenderingContext2D
  private videos = new Map<string, HTMLVideoElement>()
  private nomes = new Map<string, string>()
  private audioCtx: AudioContext
  private destino: MediaStreamAudioDestinationNode
  private fontesAudio = new Map<string, MediaStreamAudioSourceNode>()
  private raf = 0
  private vivo = true
  private fluxoComposto: MediaStream | null = null

  cena: Cena = 'grelha'
  focoIds: string[] = []

  constructor(largura = 1280, altura = 720) {
    this.canvas.width = largura
    this.canvas.height = altura
    const ctx = this.canvas.getContext('2d', { alpha: false })
    if (!ctx) throw new Error('canvas 2d indisponível')
    this.ctx = ctx
    this.audioCtx = new AudioContext()
    this.destino = this.audioCtx.createMediaStreamDestination()
    // Fonte silenciosa sempre ligada: um destino sem entradas não produz
    // amostras, e um consumidor a jusante (MediaRecorder, o directo) fica
    // sem faixa de áudio nenhuma — a mesma armadilha que o
    // `studio/compositor.ts` documenta e já pagou uma vez.
    const silencio = this.audioCtx.createConstantSource()
    silencio.offset.value = 0
    silencio.connect(this.destino)
    silencio.start()
  }

  /** Chamar sempre que a lista de participantes (local + remotos) mudar. */
  definirParticipantes(fontes: Fonte[]): void {
    for (const f of fontes) {
      this.nomes.set(f.id, f.nome)
      let v = this.videos.get(f.id)
      if (!v) {
        v = document.createElement('video')
        v.autoplay = true
        v.muted = true
        v.playsInline = true
        this.videos.set(f.id, v)
      }
      if (v.srcObject !== f.stream) {
        v.srcObject = f.stream
        void v.play().catch(() => {})
      }
      if (f.stream && f.stream.getAudioTracks().length && !this.fontesAudio.has(f.id)) {
        const n = this.audioCtx.createMediaStreamSource(new MediaStream(f.stream.getAudioTracks()))
        n.connect(this.destino)
        this.fontesAudio.set(f.id, n)
      }
    }
    const idsActuais = new Set(fontes.map((f) => f.id))
    for (const id of [...this.videos.keys()]) {
      if (idsActuais.has(id)) continue
      this.videos.get(id)?.pause()
      this.videos.delete(id)
      this.nomes.delete(id)
      this.fontesAudio.get(id)?.disconnect()
      this.fontesAudio.delete(id)
    }
    // Quem saiu não fica preso no foco — a próxima cena solo/lado-a-lado cai
    // para quem sobrar em vez de mostrar um rectângulo vazio para sempre.
    this.focoIds = this.focoIds.filter((id) => idsActuais.has(id))
  }

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

    const ids = [...this.videos.keys()]
    const visiveis = participantesVisiveis(this.cena, ids, this.focoIds, (id) => id)
    const rects = calcularRects(visiveis.length, W, H)
    const pad = visiveis.length > 1 ? 2 : 0

    visiveis.forEach((id, i) => {
      const v = this.videos.get(id)
      const r = rects[i]
      if (!v || !r) return
      const x = r.x + pad
      const y = r.y + pad
      const w = r.w - pad * 2
      const h = r.h - pad * 2
      if (v.readyState >= 2 && v.videoWidth > 0) {
        const vw = v.videoWidth
        const vh = v.videoHeight
        // `cover`: preenche o rect e corta o excesso — como um mosaico da sala.
        const escala = Math.max(w / vw, h / vh)
        const dw = vw * escala
        const dh = vh * escala
        this.ctx.save()
        this.ctx.beginPath()
        this.ctx.rect(x, y, w, h)
        this.ctx.clip()
        this.ctx.drawImage(v, x + (w - dw) / 2, y + (h - dh) / 2, dw, dh)
        this.ctx.restore()
      } else {
        this.ctx.fillStyle = '#1a1d24'
        this.ctx.fillRect(x, y, w, h)
      }
      const nome = this.nomes.get(id)
      if (nome) {
        this.ctx.font = `${Math.max(12, Math.round(h * 0.05))}px sans-serif`
        this.ctx.fillStyle = 'rgba(255,255,255,0.92)'
        this.ctx.fillText(nome, x + 10, y + h - 12, Math.max(0, w - 20))
      }
    })
  }

  /** Idempotente: chamadas seguintes devolvem o MESMO fluxo, não um novo. */
  montarFluxo(fps = 30): MediaStream {
    if (this.fluxoComposto) return this.fluxoComposto
    const stream = this.canvas.captureStream(fps)
    for (const t of this.destino.stream.getAudioTracks()) stream.addTrack(t)
    this.fluxoComposto = stream
    this.iniciarPreVisualizacao()
    return stream
  }

  destruir(): void {
    this.vivo = false
    cancelAnimationFrame(this.raf)
    this.raf = 0
    for (const v of this.videos.values()) v.pause()
    this.videos.clear()
    this.nomes.clear()
    for (const n of this.fontesAudio.values()) n.disconnect()
    this.fontesAudio.clear()
    void this.audioCtx.close().catch(() => {})
    this.fluxoComposto = null
  }
}
