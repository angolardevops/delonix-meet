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
  /**
   * Destino GUARDADO da organização (`/api/orgs/{org}/stream-destinations`).
   * Com `id`, o servidor lê o URL e decifra a chave do lado dele: `url` e
   * `chave` NÃO vão na query — a chave guardada nunca volta ao browser.
   */
  id?: string
  /**
   * O servidor tem uma chave para este destino (`has_key`). É diferente de
   * `chave` estar preenchida: a de um destino guardado nunca volta ao browser,
   * e sem este campo um destino carregado lia-se como «sem chave».
   */
  temChaveGuardada?: boolean
}

/** O estado de UM destino, como o servidor o manda (`broadcast.rs`). */
export type FaseDoDestino = 'a-ligar' | 'no-ar' | 'parado' | 'erro'
export const FASES_DO_DESTINO: readonly FaseDoDestino[] = ['a-ligar', 'no-ar', 'parado', 'erro']

export interface EstadoDoDestino {
  /** Índice do destino no array enviado em `comecar`. */
  dest: number
  /** Presente quando o destino veio de um destino guardado. */
  id?: string
  rotulo: string
  /** `youtube | facebook | linkedin | twitch | rtmp` — derivado pelo servidor. */
  platform: string
  /**
   * `a-ligar`: o processo arrancou e ainda não saiu nada. `no-ar`: sai media.
   * `erro`: caiu e vai tentar outra vez (com `motivo`). `parado`: terminado —
   * pela pessoa (sem `motivo`) ou porque desistiu (com `motivo`).
   */
  estado: FaseDoDestino
  /** Débito de saída medido no servidor, em kbit/s; 0 fora do ar. */
  kbps: number
  /** Percentagem dos bytes enviados pelo browser que não chegaram a este destino. */
  perdas: number
  motivo: string | null
  tentativas: number
  framesDescartados: number
  bytesEnviados: number
}

/** O que pode chegar do servidor numa trama de texto do directo. */
export type MensagemDoDirecto =
  | { tipo: 'destinos'; destinos: EstadoDoDestino[] }
  | { tipo: 'erro'; motivo: string }

const numero = (v: unknown, omissao = 0): number =>
  typeof v === 'number' && Number.isFinite(v) && v >= 0 ? v : omissao

function lerDestino(v: unknown): EstadoDoDestino | null {
  if (!v || typeof v !== 'object') return null
  const o = v as Record<string, unknown>
  if (typeof o.dest !== 'number' || !Number.isInteger(o.dest) || o.dest < 0) return null
  if (!FASES_DO_DESTINO.includes(o.estado as FaseDoDestino)) return null
  return {
    dest: o.dest,
    ...(typeof o.id === 'string' ? { id: o.id } : {}),
    rotulo: typeof o.rotulo === 'string' ? o.rotulo : '',
    platform: typeof o.platform === 'string' ? o.platform : 'rtmp',
    estado: o.estado as FaseDoDestino,
    kbps: numero(o.kbps),
    perdas: numero(o.perdas),
    motivo: typeof o.motivo === 'string' && o.motivo ? o.motivo : null,
    tentativas: numero(o.tentativas),
    framesDescartados: numero(o.frames_descartados),
    bytesEnviados: numero(o.bytes_enviados),
  }
}

/**
 * Lê uma trama de texto do WebSocket do directo. Devolve `null` para o que não
 * reconhece — um servidor mais novo pode mandar mensagens que este cliente
 * ainda não conhece, e isso não pode derrubar a emissão.
 *
 * Um destino malformado dentro da lista é descartado sozinho: os outros
 * continuam a ser mostrados.
 */
export function lerMensagemDoDirecto(texto: string): MensagemDoDirecto | null {
  let m: unknown
  try {
    m = JSON.parse(texto)
  } catch {
    return null
  }
  if (!m || typeof m !== 'object') return null
  const o = m as Record<string, unknown>
  if (typeof o.erro === 'string') return { tipo: 'erro', motivo: o.erro }
  if (o.tipo === 'destinos' && Array.isArray(o.destinos)) {
    const destinos = o.destinos.map(lerDestino).filter((d): d is EstadoDoDestino => d !== null)
    return { tipo: 'destinos', destinos }
  }
  return null
}

/** «4 no ar · 1 parado» e o débito total — contas puras sobre o estado. */
export function resumirDestinos(destinos: readonly EstadoDoDestino[]): Record<FaseDoDestino, number> & {
  total: number
  kbps: number
} {
  const r = { 'a-ligar': 0, 'no-ar': 0, parado: 0, erro: 0, total: destinos.length, kbps: 0 }
  for (const d of destinos) {
    r[d.estado]++
    if (d.estado === 'no-ar') r.kbps += d.kbps
  }
  return r
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

/**
 * Monta o URL do WebSocket. Separado para ser testável sem rede.
 *
 * O URL leva o token de sala e o codec — e MAIS NADA. Os destinos iam aqui,
 * em JSON, e com eles a chave de emissão de cada um: um URL fica escrito nos
 * logs de acesso de todos os proxies pelo caminho, e a chave do YouTube de
 * alguém não é coisa para lá ficar. Vão agora na primeira trama de texto
 * (`pedidoDeInicio`), depois de a ligação estar aberta.
 */
export function urlDoDirecto(base: { protocol: string; host: string }, codigo: string, token: string): string {
  const esquema = base.protocol === 'https:' ? 'wss:' : 'ws:'
  const q = new URLSearchParams({ token, codec: CODEC_DIRECTO })
  return `${esquema}//${base.host}/api/rooms/${encodeURIComponent(codigo)}/live?${q}`
}

/**
 * A primeira trama que o servidor espera: os destinos da emissão.
 *
 * Um ARRAY, não um só — é o multi-canal tipo StreamYard: uma só ligação, um
 * só `MediaRecorder` a codificar uma vez, e é o `ffmpeg` do servidor que
 * reparte para N plataformas. Um destino guardado vai só pelo `id`: a chave
 * dele nunca volta ao browser.
 */
export function pedidoDeInicio(destinos: Destino[]): string {
  return JSON.stringify({
    tipo: 'iniciar',
    destinos: destinos.map((d) =>
      d.id
        ? { id: d.id, ...(d.rotulo ? { rotulo: d.rotulo } : {}) }
        : {
            url: d.url.trim(),
            chave: d.chave.trim(),
            ...(d.rotulo ? { rotulo: d.rotulo } : {}),
          },
    ),
  })
}

/**
 * Quanto se espera pela primeira resposta do servidor ao pedido de início
 * antes de dar a emissão por aceite. O servidor valida os destinos depois de
 * receber o pedido (base de dados, resolução de nomes) e responde com o estado
 * deles ou com a recusa; isto é só o tecto para o caso de nenhuma das duas
 * chegar.
 */
const ESPERA_DO_ACEITE_MS = 2000

export interface OpcoesDoDirecto {
  /** Débito de vídeo. Omisso: `DEBITO_OMISSAO`. Ver `debitoDoDirecto`. */
  bitrate?: number
  /** Débito de áudio. Omisso: `AUDIO_BPS`. */
  audioBitrate?: number
  /** Tamanho dos pedaços, em ms. Menor = menos latência, mais overhead. */
  fatia?: number
}

/** O que se usava para TUDO antes de o perfil chegar aqui (1080p30). */
export const DEBITO_OMISSAO = 4_500_000
/**
 * Áudio a 128 kbps, o mesmo da gravação. Declarado porque o que não se declara
 * fica ao critério do browser, e um valor que não se escolheu não se defende.
 */
export const AUDIO_BPS = 128_000
export const DEBITO_MINIMO = 1_000_000
/**
 * Tecto do que se EMITE. Não é o do que se grava: a gravação vai para disco e
 * o perfil 2160p50 pede 35 Mbps, que nenhum uplink destes dá nem nenhuma
 * plataforma precisa.
 */
export const DEBITO_MAXIMO = 20_000_000

/**
 * O débito de EMISSÃO de um perfil de imagem.
 *
 * Até 2026-10-14 era 4,5 Mbps para tudo: um estúdio a compor 2160p50 emitia
 * com o débito de 1080p30 (imagem desfeita em movimento) e um 1080p30 numa
 * ligação fraca gastava o mesmo que o 4K. O perfil é o que decide o tamanho e
 * os fps da imagem, logo é ele que tem de decidir quantos bits ela precisa.
 *
 * A conta é bits por pixel e por quadro: 0,08 até 1080p, 0,05 a partir de
 * 2160p (quadros maiores precisam de menos bits por pixel para a mesma
 * qualidade aparente), com tecto e chão. É uma regra NOSSA, derivada do
 * tamanho — a faixa recomendada por cada plataforma varia e tem de ser lida na
 * documentação dela antes de ser citada como facto.
 *
 * O tecto REAL de uma emissão não é este número: é o uplink. É para isso que
 * existe o `saudeDaFila` — este valor é o pedido, aquele é a resposta.
 */
export function debitoDoDirecto(perfil: { largura: number; altura: number; fps: number }): number {
  const bpp = perfil.altura >= 2160 ? 0.05 : 0.08
  const bruto = perfil.largura * perfil.altura * perfil.fps * bpp
  return Math.round(Math.min(DEBITO_MAXIMO, Math.max(DEBITO_MINIMO, bruto)))
}

/**
 * A saúde do ENVIO, lida pela fila do socket.
 *
 * `ok` — a fila drena. `apertada` — há mais de um segundo de emissão à espera:
 * a rede está no limite. `a-acumular` — mais de três segundos: o uplink não dá
 * vazão, a emissão está a atrasar-se e vai continuar a atrasar-se.
 *
 * PORQUE É SÓ UM DIAGNÓSTICO: baixar o débito a meio exigiria recomeçar o
 * `MediaRecorder`, e um segundo cabeçalho Matroska no meio do fluxo é lixo
 * para o `ffmpeg` do servidor (`-c:v copy`, R76). A adaptação a sério pede o
 * `VideoEncoder` do WebCodecs, que aceita `configure({bitrate})` a quente —
 * está no caminho do produto, não nesta função. O que esta função não faz,
 * diz: e dizer «a rede não dá vazão, baixa a qualidade» é mais do que o
 * estúdio fazia, que era não dizer nada e acumular memória em silêncio.
 */
export type SaudeDaRede = 'ok' | 'apertada' | 'a-acumular'

export interface Rede {
  /** Bytes à espera no socket na última fatia. */
  fila: number
  /** A fila em segundos de emissão, ao débito pedido. */
  segundos: number
  saude: SaudeDaRede
  /** O débito pedido ao codificador, em bits/s (vídeo + áudio). */
  alvo: number
}

export const REDE_ZERO: Rede = { fila: 0, segundos: 0, saude: 'ok', alvo: 0 }

export function saudeDaFila(fila: number, alvoBps: number): Rede {
  const alvo = alvoBps > 0 ? alvoBps : DEBITO_OMISSAO
  const segundos = (Math.max(0, fila) * 8) / alvo
  const saude: SaudeDaRede = segundos > 3 ? 'a-acumular' : segundos > 1 ? 'apertada' : 'ok'
  return { fila: Math.max(0, fila), segundos, saude, alvo }
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
  /**
   * O último estado POR DESTINO que o servidor mandou (a cada 2 s e a cada
   * mudança). Vazio até à primeira mensagem, e fica com o último conhecido
   * depois de a emissão acabar.
   */
  destinos: EstadoDoDestino[] = []
  aoMudarDestinos: ((d: EstadoDoDestino[]) => void) | null = null
  /** A saúde do envio, lida a cada fatia. Ver `saudeDaFila`. */
  rede: Rede = REDE_ZERO
  aoMudarRede: ((r: Rede) => void) | null = null
  /** A razão que o servidor deu antes de fechar, para o fecho a poder mostrar. */
  private motivoDoServidor: string | null = null

  /** Trata uma trama de texto do servidor. Devolve o que leu. */
  private receber(dados: unknown): MensagemDoDirecto | null {
    if (typeof dados !== 'string') return null
    const m = lerMensagemDoDirecto(dados)
    if (m?.tipo === 'destinos') {
      this.destinos = m.destinos
      this.aoMudarDestinos?.(m.destinos)
    } else if (m?.tipo === 'erro') {
      this.motivoDoServidor = m.motivo
    }
    return m
  }

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
    destinos: Destino[],
    opcoes: OpcoesDoDirecto = {},
  ): Promise<void> {
    if (this.socket) throw new Error('já está no ar')
    if (!directoSuportado()) {
      throw new Error('este browser não sabe codificar H.264, que é o que as plataformas de directo aceitam')
    }
    this.anunciar({ fase: 'a-ligar' })
    this.destinos = []
    this.motivoDoServidor = null

    const url = urlDoDirecto(location, codigo, token)
    const socket = new WebSocket(url)
    socket.binaryType = 'arraybuffer'
    this.socket = socket

    // O servidor ACEITA sempre o upgrade e recusa depois, com uma trama de
    // texto `{"erro": "..."}`. É a única forma de uma frase inteira chegar
    // aqui: um erro HTTP antes do upgrade não expõe estado nem corpo à API de
    // WebSocket, e o `reason` do close está limitado a 123 bytes.
    //
    // Medido antes desta mudança: a recusa de E2EE — a mais importante do
    // ADR-0003 — chegava como «não foi possível ligar».
    await new Promise<void>((resolve, reject) => {
      let recusa: string | null = null
      socket.onmessage = (e) => {
        const m = this.receber(e.data)
        if (m?.tipo === 'erro') recusa = m.motivo
        // O estado dos destinos só vem de uma emissão ACEITE: não é preciso
        // esperar pelos 250 ms.
        else if (m?.tipo === 'destinos') resolve()
      }
      socket.onopen = () => {
        // Os destinos — e as chaves — só saem daqui, numa trama, nunca no URL.
        socket.send(pedidoDeInicio(destinos))
        // Não resolve já: o servidor valida o pedido e responde com o estado
        // dos destinos (aceite) ou com a recusa. O temporizador é só o tecto.
        setTimeout(() => (recusa ? reject(new Error(recusa)) : resolve()), ESPERA_DO_ACEITE_MS)
      }
      socket.onclose = () => reject(new Error(recusa ?? 'o servidor recusou a emissão'))
      socket.onerror = () => reject(new Error(recusa ?? 'não foi possível ligar ao servidor de emissão'))
    })
    // A partir daqui o socket está aceite: os handlers passam a ser os de
    // regime, e um fecho deixa de ser uma recusa para passar a ser um fim. Um
    // fecho depois de `{"erro": …}` (ex.: nenhum destino ficou no ar) mostra a
    // razão do servidor, não um «caiu» genérico.
    socket.onmessage = (e) => void this.receber(e.data)
    socket.onclose = () => this.terminarPorFora(this.motivoDoServidor ?? 'a ligação ao servidor caiu')
    socket.onerror = () => this.terminarPorFora(this.motivoDoServidor ?? 'a ligação ao servidor falhou')

    const video = opcoes.bitrate ?? DEBITO_OMISSAO
    const audio = opcoes.audioBitrate ?? AUDIO_BPS
    const gravador = new MediaRecorder(stream, {
      mimeType: MIME_DIRECTO,
      videoBitsPerSecond: video,
      audioBitsPerSecond: audio,
    })
    this.gravador = gravador
    gravador.ondataavailable = (e) => {
      if (!e.data.size || socket.readyState !== WebSocket.OPEN) return
      this.bytes += e.data.size
      void e.data.arrayBuffer().then((b) => {
        // Entre o `then` e aqui o socket pode ter fechado.
        if (socket.readyState === WebSocket.OPEN) socket.send(b)
      })
      // A FILA do socket, a cada fatia. Os pedaços enviam-se sempre: um
      // pedaço saltado é um buraco no Matroska, e o `ffmpeg` do servidor não
      // o sabe remendar. O que se faz com a fila é DIZÊ-LA.
      this.medirRede(socket, video + audio)
      if (this.estado.fase === 'no-ar') {
        this.anunciar({ fase: 'no-ar', desde: this.desde, bytes: this.bytes })
      }
    }
    this.desde = Date.now()
    this.bytes = 0
    this.rede = saudeDaFila(0, video + audio)
    gravador.start(opcoes.fatia ?? 500)
    this.anunciar({ fase: 'no-ar', desde: this.desde, bytes: 0 })
  }

  private medirRede(socket: WebSocket, alvoBps: number): void {
    const r = saudeDaFila(socket.bufferedAmount, alvoBps)
    const antes = this.rede
    this.rede = r
    // Só se anuncia o que muda alguma coisa no ecrã: a saúde, ou um décimo de
    // segundo de fila. A cada 500 ms, anunciar sempre era uma renderização por
    // fatia para mostrar o mesmo número.
    if (r.saude !== antes.saude || Math.abs(r.segundos - antes.segundos) >= 0.1) this.aoMudarRede?.(r)
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
    this.rede = REDE_ZERO
    this.aoMudarRede?.(this.rede)
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
    this.rede = REDE_ZERO
    this.aoMudarRede?.(this.rede)
    this.anunciar({ fase: 'erro', motivo })
  }

  get noAr(): boolean {
    return this.estado.fase === 'no-ar'
  }
}
