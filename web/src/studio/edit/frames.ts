/**
 * Frames EXACTOS de uma fonte, para a exportação.
 *
 * Caminho principal: WebM desmultiplexado (`webm.ts`) + `VideoDecoder`. Pede-se
 * o frame em `t`; o decodificador avança em sequência a partir de onde está e
 * só recua ao frame-chave anterior quando se salta para trás ou para longe.
 * Ler um troço inteiro custa uma decodificação por frame — não uma por pedido.
 *
 * Caminho de recurso: um `<video>` a procurar. É lento (cada procura recomeça
 * no frame-chave) mas lê o que o browser ler — um MP4 importado, por exemplo,
 * que o desmultiplexador não conhece.
 */
import { codecParaDecoder, lerWebm } from './webm'

export interface FornecedorDeFrames {
  readonly largura: number
  readonly altura: number
  /** O frame visível em `t` segundos (o último com tempo ≤ t), ou `null`. */
  frameEm(t: number): Promise<CanvasImageSource | null>
  fechar(): void
}

export async function abrirFornecedor(blob: Blob): Promise<FornecedorDeFrames> {
  if (typeof VideoDecoder === 'function') {
    try {
      const f = await PorDecoder.abrir(blob)
      if (f) return f
    } catch (e) {
      console.info('[editor] desmultiplexador recusou a fonte, a usar o <video>', e)
    }
  }
  return PorElemento.abrir(blob)
}

class PorDecoder implements FornecedorDeFrames {
  private decoder: VideoDecoder
  private fila: VideoFrame[] = []
  private actual: VideoFrame | null = null
  private proximo = 0
  private terminado = false
  private acordar: (() => void) | null = null
  private erro: Error | null = null

  private constructor(
    private readonly config: VideoDecoderConfig,
    private readonly blocos: { tempo: number; chave: boolean; dados: Uint8Array }[],
    readonly largura: number,
    readonly altura: number,
  ) {
    this.decoder = this.criar()
  }

  static async abrir(blob: Blob): Promise<PorDecoder | null> {
    const w = lerWebm(new Uint8Array(await blob.arrayBuffer()))
    const pista = w.pistas.find((p) => p.tipo === 'video')
    if (!pista) return null
    const c = codecParaDecoder(pista)
    if (!c) return null
    const config: VideoDecoderConfig = {
      codec: c.codec,
      ...(c.description ? { description: c.description } : {}),
      codedWidth: pista.largura,
      codedHeight: pista.altura,
    }
    const suporte = await VideoDecoder.isConfigSupported(config)
    if (!suporte.supported) return null
    const blocos = w.blocos.filter((b) => b.pista === pista.numero)
    if (!blocos.length || !blocos[0].chave) return null
    return new PorDecoder(config, blocos, pista.largura, pista.altura)
  }

  private criar(): VideoDecoder {
    const d = new VideoDecoder({
      output: (f) => {
        this.fila.push(f)
        this.acordar?.()
      },
      error: (e) => {
        this.erro = e instanceof Error ? e : new Error(String(e))
        this.acordar?.()
      },
    })
    d.configure(this.config)
    return d
  }

  private recomecar(tUs: number): void {
    for (const f of this.fila) f.close()
    this.fila = []
    this.actual?.close()
    this.actual = null
    let k = 0
    for (let i = 0; i < this.blocos.length && this.blocos[i].tempo <= tUs; i++) if (this.blocos[i].chave) k = i
    this.proximo = k
    this.terminado = false
    if (this.decoder.state === 'closed' || this.erro) {
      this.erro = null
      this.decoder = this.criar()
    } else {
      this.decoder.reset()
      this.decoder.configure(this.config)
    }
  }

  private esperar(ms: number): Promise<void> {
    return new Promise((r) => {
      const fim = () => {
        this.acordar = null
        r()
      }
      this.acordar = fim
      setTimeout(fim, ms)
    })
  }

  async frameEm(t: number): Promise<CanvasImageSource | null> {
    // Meio milissegundo de folga: os tempos do WebM são arredondados ao ms.
    const tUs = t * 1e6 + 500
    const ultimoPedido = this.proximo > 0 ? this.blocos[Math.min(this.proximo, this.blocos.length) - 1].tempo : -1
    const atras = this.actual ? tUs < this.actual.timestamp : false
    const longe = tUs > ultimoPedido + 2_000_000 && this.proximo > 0
    if (atras || longe || this.decoder.state === 'closed' || this.erro) this.recomecar(tUs)

    for (let voltas = 0; voltas < 100_000; voltas++) {
      while (this.fila.length && this.fila[0].timestamp <= tUs) {
        this.actual?.close()
        this.actual = this.fila.shift()!
      }
      if (this.fila.length) break
      if (this.erro) throw this.erro
      if (this.proximo >= this.blocos.length) {
        if (this.terminado) break
        this.terminado = true
        await this.decoder.flush().catch(() => undefined)
        continue
      }
      const b = this.blocos[this.proximo++]
      this.decoder.decode(new EncodedVideoChunk({ type: b.chave ? 'key' : 'delta', timestamp: b.tempo, data: b.dados }))
      if (this.decoder.decodeQueueSize > 2) await this.esperar(10)
      else if (this.proximo % 4 === 0) await this.esperar(0)
    }
    return this.actual
  }

  fechar(): void {
    for (const f of this.fila) f.close()
    this.fila = []
    this.actual?.close()
    this.actual = null
    if (this.decoder.state !== 'closed') this.decoder.close()
  }
}

class PorElemento implements FornecedorDeFrames {
  private constructor(
    private readonly v: HTMLVideoElement,
    private readonly url: string,
  ) {}

  get largura() {
    return this.v.videoWidth
  }
  get altura() {
    return this.v.videoHeight
  }

  static async abrir(blob: Blob): Promise<PorElemento> {
    const url = URL.createObjectURL(blob)
    const v = document.createElement('video')
    v.muted = true
    v.playsInline = true
    v.preload = 'auto'
    v.src = url
    await new Promise<void>((r, j) => {
      v.onloadeddata = () => r()
      v.onerror = () => j(new Error('fonte ilegível'))
    })
    return new PorElemento(v, url)
  }

  async frameEm(t: number): Promise<CanvasImageSource | null> {
    const alvo = Math.max(0, t)
    if (Math.abs(this.v.currentTime - alvo) > 0.001) {
      await new Promise<void>((r) => {
        this.v.onseeked = () => r()
        this.v.currentTime = alvo
      })
    }
    return this.v.videoWidth ? this.v : null
  }

  fechar(): void {
    this.v.removeAttribute('src')
    this.v.load()
    URL.revokeObjectURL(this.url)
  }
}
