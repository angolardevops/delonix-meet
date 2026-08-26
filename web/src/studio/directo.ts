/**
 * Emissão em directo a partir do Estúdio — o lado do browser do ADR-0003.
 *
 * O QUE ACONTECE AQUI, E PORQUÊ AQUI: pela decisão do ADR (opção C), é o
 * BROWSER que compõe e codifica. O servidor só remultiplexa para RTMP com
 * `-c:v copy`, e é isso que faz um directo caber no core que o pod tem. Se
 * este módulo enviasse VP8, o servidor teria de reencodificar e a decisão toda
 * caía por terra — por isso o codec não é uma preferência, é um contrato.
 *
 * MEDIDO (2026-08-26): o `MediaRecorder` do Chromium aceita
 * `video/webm;codecs=h264,opus`, e o que sai é Matroska com H.264 lá dentro —
 * `ffprobe` diz `format_name=matroska,webm`. O servidor declara `-f matroska`
 * por causa disso (R76). Este módulo e o `broadcast.rs` têm de concordar sobre
 * isto, e é a razão de os dois o dizerem por escrito.
 */

/** O contrato com o servidor. Mudar isto sem mudar o `broadcast.rs` parte o directo. */
export const MIME_DIRECTO = 'video/webm;codecs=h264,opus'
/** O que se declara ao servidor, para ele poder recusar ANTES de gastar um ffmpeg. */
export const CODEC_DIRECTO = 'video/h264'

export interface Destino {
  /** URL base, sem a chave. Ex.: `rtmp://a.rtmp.youtube.com/live2` */
  url: string
  chave: string
  rotulo?: string
}

export type EstadoDoDirecto =
  | { fase: 'parado' }
  | { fase: 'a-ligar' }
  | { fase: 'no-ar'; desde: number; bytes: number }
  | { fase: 'erro'; motivo: string }

/** O browser sabe fazer H.264? Sem isto o directo não pode arrancar. */
export function directoSuportado(): boolean {
  return typeof MediaRecorder !== 'undefined' && MediaRecorder.isTypeSupported(MIME_DIRECTO)
}

/** Monta o URL do WebSocket. Separado para ser testável sem rede. */
export function urlDoDirecto(
  base: { protocol: string; host: string },
  codigo: string,
  token: string,
  destino: Destino,
): string {
  const esquema = base.protocol === 'https:' ? 'wss:' : 'ws:'
  const q = new URLSearchParams({
    token,
    destino: destino.url.trim(),
    chave: destino.chave.trim(),
    codec: CODEC_DIRECTO,
  })
  if (destino.rotulo) q.set('rotulo', destino.rotulo)
  return `${esquema}//${base.host}/api/rooms/${encodeURIComponent(codigo)}/broadcast?${q}`
}

export interface OpcoesDoDirecto {
  /** Débito de vídeo. 4,5 Mbps é o que o YouTube pede para 1080p30. */
  bitrate?: number
  /** Tamanho dos pedaços, em ms. Menor = menos latência, mais overhead. */
  fatia?: number
}

/**
 * Uma emissão a decorrer.
 *
 * O ciclo é: abre o socket, espera que ELE aceite, e só então começa a gravar.
 * Ao contrário — gravar primeiro e enviar depois — os primeiros pedaços
 * perdiam-se, e um WebM sem o cabeçalho inicial é ilegível para o ffmpeg: o
 * directo arrancava e não saía imagem nenhuma do outro lado.
 */
export class Directo {
  private socket: WebSocket | null = null
  private gravador: MediaRecorder | null = null
  private bytes = 0
  private desde = 0

  estado: EstadoDoDirecto = { fase: 'parado' }
  /** Chamado sempre que o estado muda — a interface liga-se aqui. */
  aoMudar: ((e: EstadoDoDirecto) => void) | null = null

  private anunciar(e: EstadoDoDirecto) {
    this.estado = e
    this.aoMudar?.(e)
  }

  /**
   * Vai para o ar. `stream` é a composição do Estúdio (canvas + áudio).
   *
   * Rejeita com a razão que o SERVIDOR deu — «esta sala tem cifra
   * ponta-a-ponta…» chega assim à interface, sem ser reescrita aqui. Uma
   * mensagem traduzida duas vezes acaba a dizer «erro».
   */
  async comecar(
    stream: MediaStream,
    codigo: string,
    token: string,
    destino: Destino,
    opcoes: OpcoesDoDirecto = {},
  ): Promise<void> {
    if (this.socket) throw new Error('já está no ar')
    if (!directoSuportado()) {
      throw new Error('este browser não sabe codificar H.264, que é o que as plataformas de directo aceitam')
    }
    this.anunciar({ fase: 'a-ligar' })

    const url = urlDoDirecto(location, codigo, token, destino)
    const socket = new WebSocket(url)
    socket.binaryType = 'arraybuffer'
    this.socket = socket

    await new Promise<void>((resolve, reject) => {
      socket.onopen = () => resolve()
      // Um `close` ANTES do `open` é o servidor a recusar. O código 1006 não
      // traz razão nenhuma — o handler recusa com HTTP antes do upgrade, por
      // isso a razão fica no `reason` quando há, e num texto genérico quando
      // não há.
      socket.onclose = (e) => reject(new Error(e.reason || 'o servidor recusou a emissão'))
      socket.onerror = () => reject(new Error('não foi possível ligar ao servidor de emissão'))
    })

    // A partir daqui o socket está aceite: os handlers passam a ser os de
    // regime, e um fecho deixa de ser uma recusa para passar a ser um fim.
    socket.onclose = () => this.terminarPorFora('a ligação ao servidor caiu')
    socket.onerror = () => this.terminarPorFora('a ligação ao servidor falhou')

    const gravador = new MediaRecorder(stream, {
      mimeType: MIME_DIRECTO,
      videoBitsPerSecond: opcoes.bitrate ?? 4_500_000,
    })
    this.gravador = gravador
    gravador.ondataavailable = (e) => {
      if (!e.data.size || socket.readyState !== WebSocket.OPEN) return
      this.bytes += e.data.size
      void e.data.arrayBuffer().then((b) => {
        // Entre o `then` e aqui o socket pode ter fechado.
        if (socket.readyState === WebSocket.OPEN) socket.send(b)
      })
      if (this.estado.fase === 'no-ar') {
        this.anunciar({ fase: 'no-ar', desde: this.desde, bytes: this.bytes })
      }
    }
    this.desde = Date.now()
    this.bytes = 0
    gravador.start(opcoes.fatia ?? 500)
    this.anunciar({ fase: 'no-ar', desde: this.desde, bytes: 0 })
  }

  /** Fim pedido pelo utilizador. */
  async parar(): Promise<void> {
    const g = this.gravador
    this.gravador = null
    if (g && g.state !== 'inactive') {
      await new Promise<void>((r) => {
        g.onstop = () => r()
        g.stop()
      })
    }
    this.socket?.close(1000, 'fim')
    this.socket = null
    this.anunciar({ fase: 'parado' })
  }

  /** Fim imposto de fora (socket caiu, servidor fechou). */
  private terminarPorFora(motivo: string) {
    if (this.estado.fase === 'parado') return
    try {
      this.gravador?.stop()
    } catch {
      /* já parado */
    }
    this.gravador = null
    this.socket = null
    this.anunciar({ fase: 'erro', motivo })
  }

  get noAr(): boolean {
    return this.estado.fase === 'no-ar'
  }
}
