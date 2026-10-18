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
 * `participantesVisiveis`, `ordenarPorFoco` e `calcularRects`. É aí que vive a
 * lógica que um `git revert` distraído partiria — a classe à volta só liga essa
 * matemática a APIs de browser.
 *
 * O `ordenarPorFoco` já NÃO NASCE AQUI — desceu para `cena/foco.ts` quando o
 * Estúdio passou a precisar dele, e é re-exportado mais abaixo. Continua a
 * testar-se no `compositor.test.ts` desta pasta, que é onde estão os casos.
 */

import {
  caber,
  COR_DA_MARCA,
  COR_DO_FUNDO,
  desenharMarcaDaMoldura,
  enquadrar,
  type MarcaDaMoldura,
} from '../cena/desenhoDaMoldura'
import { ordenarPorFoco } from '../cena/foco'
import { MAXIMO_DE_CAMARAS, rectsDaMoldura, type Rect } from '../cena/moldura'

/**
 * `moldura` é a cena de marca — faixa a rolar, coluna com logótipo, câmaras e
 * assinatura, e o conteúdo grande ao lado. Ao contrário das outras três, tem um
 * HERÓI explícito (`heroiId`), que é normalmente a partilha de ecrã: é o que a
 * torna útil para uma aula em directo, onde o que importa são os slides e não
 * os retratos.
 */
export type Cena = 'solo' | 'lado-a-lado' | 'grelha' | 'moldura'

/**
 * O `Rect` deste módulo passou a vir de `cena/moldura.ts`, e é re-exportado
 * daqui.
 *
 * PORQUÊ: a moldura de transmissão vai ser desenhada por este compositor e pelo
 * do Estúdio, logo vive num módulo à parte. Se esse módulo importasse o `Rect`
 * daqui, e este importasse a moldura de lá, fechava-se um ciclo. A cena é
 * folha — não importa de nenhuma outra pasta de `src/` — e quem o impõe é a
 * regra `no-restricted-imports` para `src/cena/**` no `web/eslint.config.js`,
 * que corre no `check-frontend-lint.sh` do CI.
 *
 * Fica re-exportado para que nada a jusante (`studio/palco.ts`, entre outros)
 * tenha de mudar de import por causa de uma mudança que não lhes diz respeito.
 *
 * O QUE ISTO NÃO RESOLVEU: há uma TERCEIRA declaração igual em
 * `studio/tv/mesa.ts`, consumida pelo `desenhoDaMesa.ts`. Não se unificou aqui
 * para não arrastar a mesa de corte para dentro desta fatia, mas fica dito: são
 * dois locais de nascimento, não um, e quem acrescentar um campo ao `Rect` tem
 * de o fazer nos dois.
 */
export type { Rect }

/**
 * O `ordenarPorFoco` desceu para `cena/foco.ts`, pela MESMA razão que o `Rect`
 * acima, e é re-exportado daqui pelo mesmo motivo.
 *
 * PORQUÊ DESCEU: deixou de ser só da Sala. O Estúdio precisa dele para a coluna
 * da moldura obedecer ao `programaId`, e `room/` e `studio/` já se importam nos
 * DOIS sentidos — o Estúdio chamá-lo daqui acrescentava uma aresta a um
 * emaranhado que já existe. A cena é folha, logo os dois a podem importar sem
 * ciclo.
 *
 * A re-exportação não é cortesia: o `room/compositor.test.ts` e qualquer coisa
 * a jusante continuam a importá-lo daqui, e uma mudança de arrumação interna não
 * tem de lhes custar um `git diff`.
 */
export { ordenarPorFoco }

export interface Fonte {
  id: string
  nome: string
  stream: MediaStream | null
}

/**
 * Quantas fontes cada cena põe no ar — e portanto quantas o anfitrião pode
 * escolher nas fichas de foco.
 *
 * UMA TABELA, e não um ternário em cada sítio que precisa do número. O número
 * estava escrito em quatro: aqui, no `toggleFoco` do `useMulticam`, na dica do
 * `MulticamPanel` e no tecto da coluna. Um `Record` sobre a união recusa-se a
 * compilar quando alguém acrescenta uma cena; um ternário com ramo por omissão
 * aceita-a em silêncio e dá-lhe o tecto de outra — era assim que a `moldura`
 * tinha fichas limitadas a dois para uma coluna de três.
 *
 * A `grelha` mostra toda a gente, e `Infinity` diz isso sem precisar de um caso
 * especial em quem lê a tabela.
 */
export const LUGARES_DA_CENA: Record<Cena, number> = {
  solo: 1,
  'lado-a-lado': 2,
  grelha: Number.POSITIVE_INFINITY,
  moldura: MAXIMO_DE_CAMARAS,
}

/**
 * Quais fontes se veem, dada a cena e quem está em foco. Em `solo`/
 * `lado-a-lado`, se faltar gente no foco (ninguém escolhido, ou só um em
 * `lado-a-lado`), completa-se com quem sobrar — a emissão não pode ficar
 * com um ecrã vazio só porque o anfitrião ainda não escolheu ninguém.
 *
 * NÃO SERVE A `moldura`, e o `desenhar` nunca a manda por aqui: esta função
 * CORTA no número de lugares, e a moldura precisa da lista inteira para contar
 * quem ficou de fora. Lá usa-se o `ordenarPorFoco` cru.
 */
export function participantesVisiveis<T>(cena: Cena, todos: T[], focoIds: string[], idDe: (t: T) => string): T[] {
  if (cena === 'grelha') return todos
  // O corpo é o `ordenarPorFoco` mais um corte. Era escrito à mão aqui, e a
  // cópia tinha um defeito que a outra não tem: sem a de-duplicação, um
  // `focoIds` com o mesmo id duas vezes punha A MESMA câmara nas duas metades
  // de um `lado-a-lado` e deixava duas pessoas fora do ar. Agora há uma
  // implementação só, e a guarda vale para as duas cenas.
  return ordenarPorFoco(todos, focoIds, idDe).slice(0, LUGARES_DA_CENA[cena])
}

/**
 * O foco aparado ao tecto da cena — e A MESMA LISTA quando já cabe.
 *
 * Devolver a mesma referência quando nada muda não é poupança de memória: é o
 * que impede um `setFocoIds` num efeito de disparar outra renderização, e outra,
 * a cada troca de cena. O React compara por identidade.
 *
 * Apara-se o FIM, como o `toggleFoco` faz com o seu `slice(-max)`: o que fica é
 * o que o anfitrião escolheu mais recentemente.
 */
export function aparadoAoTecto(focoIds: string[], cena: Cena): string[] {
  const tecto = LUGARES_DA_CENA[cena]
  return focoIds.length > tecto ? focoIds.slice(-tecto) : focoIds
}

/**
 * Quem ocupa o rectângulo grande da `moldura`.
 *
 * Por ordem: o DESIGNADO (`heroiId`, normalmente a partilha de ecrã), se ainda
 * cá estiver; senão a primeira fonte que o anfitrião NÃO escolheu para a
 * coluna; senão a primeira que houver.
 *
 * O meio dessa ordem é o que faltava, e falhava assim: sem partilha de ecrã o
 * herói caía em `ids[0]`, que é o próprio anfitrião, e o `desenharMoldura` tira
 * o herói da lista da coluna. Carregar na própria ficha gastava um dos três
 * lugares e não mudava nada — a ficha ficava acesa e o ar igual, e um dos três
 * lugares que a dica promete ia para quem o anfitrião não escolheu. Preferir um
 * NÃO-escolhido para o herói faz as três escolhas chegarem todas à coluna.
 *
 * Nunca devolve `null` com fontes na sala: uma emissão com o maior rectângulo do
 * ecrã em negro é pior do que uma com a cara de alguém — e se o anfitrião
 * escolher TODOS, o herói volta a ser `ids[0]`, porque não há sobras.
 */
export function heroiDaMoldura(ids: string[], heroiId: string | null, focoIds: string[]): string | null {
  if (heroiId && ids.includes(heroiId)) return heroiId
  return ids.find((id) => !focoIds.includes(id)) ?? ids[0] ?? null
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
  /**
   * Quem o anfitrião escolheu, e o que isso significa muda com a cena: em
   * `solo`/`lado-a-lado` é QUEM APARECE (o resto não entra no quadro); na
   * `moldura` é QUEM VEM PRIMEIRO na coluna, porque lá todos entram na lista e
   * é o tecto da coluna que decide onde ela acaba.
   */
  focoIds: string[] = []
  /**
   * Quem ocupa o herói da `moldura` — normalmente a partilha de ecrã.
   *
   * `null`, ou um id que já não está na sala, NÃO deixa o herói preto: cai para
   * a primeira fonte que houver. Uma emissão com o maior rectângulo do ecrã em
   * negro é pior do que uma emissão com a cara de alguém lá — e quem pára de
   * partilhar o ecrã a meio de uma aula não pode derrubar a imagem.
   */
  heroiId: string | null = null
  /** A marca desenhada na moldura. Quem monta o compositor é que a sabe. */
  marca: MarcaDaMoldura = { logo: null, faixa: '', assinatura: '', cor: COR_DA_MARCA }

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
    // O FOCO NÃO SE PODA AQUI, e já se podava: `this.focoIds` era filtrado para
    // os ids presentes, com a intenção boa de não deixar quem saiu preso no
    // foco. O efeito era pior do que o problema.
    //
    // O foco é estado do React (`useMulticam`), que o empurra para cá num
    // `useEffect([focoIds])`. Podá-lo deste lado fazia as duas cópias
    // divergirem SEM caminho de volta: o array do React não mudava de
    // referência, logo o efeito não voltava a correr, e o compositor ficava com
    // a versão podada para sempre. Medido como falha de produto: uma câmara
    // cujo stream pisca (uma renegociação) sai e volta à lista; o `definir`
    // corria no meio, limpava-a do foco, e ela nunca regressava à coluna — com
    // a ficha do painel a continuar acesa. Desde que a moldura obedece ao foco,
    // isto decidia quem ia ao ar.
    //
    // E a poda era DISPENSÁVEL: tanto o `participantesVisiveis` como o
    // `ordenarPorFoco` ignoram um id que não esteja na lista. Um fantasma no
    // foco não desenha rectângulo vazio nenhum — só não encontra ninguém.
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
    if (this.cena === 'moldura') {
      this.desenharMoldura(W, H)
      return
    }
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

  /**
   * A cena de marca: o herói grande, as câmaras na coluna, e a marca por cima.
   *
   * A GEOMETRIA e a marca vêm de `cena/`, partilhadas com o Estúdio; as FONTES
   * são desenhadas aqui, com o recorte e o nome que esta sala já sabe pôr. É a
   * mesma divisão que o compositor do Estúdio faz, e pela mesma razão: a cena
   * não tem de saber o que é um nome de participante.
   */
  private desenharMoldura(W: number, H: number): void {
    this.ctx.fillStyle = COR_DO_FUNDO
    this.ctx.fillRect(0, 0, W, H)

    const ids = [...this.videos.keys()]
    const heroi = heroiDaMoldura(ids, this.heroiId, this.focoIds)
    // A ordem da coluna é a do FOCO — as fichas do `MulticamPanel` são o que o
    // anfitrião tem para decidir quem ocupa os lugares que há. A lista vai
    // INTEIRA para a geometria: é ela que corta no tecto e que diz, no
    // `omitidas`, quantas ficaram de fora.
    const camaras = ordenarPorFoco(
      ids.filter((id) => id !== heroi),
      this.focoIds,
      (id) => id,
    )
    // As opções da geometria seguem a marca que REALMENTE existe. Sem isto, a
    // Sala — que nunca tem logótipo — reservava 26% da altura da coluna para
    // uma imagem que nunca chega, e as câmaras ficavam espremidas por baixo de
    // um bloco preto morto. O `OpcoesDaMoldura.logo` existe exactamente para
    // devolver esse espaço a quem o usa.
    const r = rectsDaMoldura(camaras.length, W, H, {
      lado: 'esquerda',
      logo: !!this.marca.logo,
      assinatura: !!this.marca.assinatura.trim(),
    })

    // O herói DESIGNADO é a partilha de ecrã, e um ecrã desenha-se `contain`:
    // cortar slides para encher o rectângulo perde a primeira e a última
    // coluna de texto. Medido: com `cover`, um ecrã 16:9 num herói de 1,376
    // perdia 11% de cada lado, e um 21:9 perdia 41%. O Estúdio já desenhava o
    // ecrã `contain` (`desenharEcraEm`) pela mesma razão.
    //
    // Quando o herói é o RECURSO (não há ecrã, entrou uma câmara), volta a ser
    // `cover`: um retrato com barras pretas dos lados é feio sem ser útil.
    const heroiEhOEcra = !!heroi && heroi === this.heroiId
    if (heroi) this.pintarFonte(heroi, r.heroi, false, heroiEhOEcra ? 'contain' : 'cover')
    r.camaras.forEach((rect, i) => {
      const id = camaras[i]
      if (id) this.pintarFonte(id, rect, true)
    })
    desenharMarcaDaMoldura(this.ctx, r, this.marca, Date.now(), W, H)
  }

  /** Uma fonte no rect, com o nome por baixo se se pedir. */
  private pintarFonte(id: string, r: Rect, comNome: boolean, modo: 'cover' | 'contain' = 'cover'): void {
    const v = this.videos.get(id)
    if (!v || r.w <= 0 || r.h <= 0) return
    if (v.readyState >= 2 && v.videoWidth > 0) {
      const d = modo === 'contain' ? caber(v.videoWidth, v.videoHeight, r) : enquadrar(v.videoWidth, v.videoHeight, r)
      this.ctx.save()
      this.ctx.beginPath()
      this.ctx.rect(r.x, r.y, r.w, r.h)
      this.ctx.clip()
      this.ctx.drawImage(v, d.x, d.y, d.w, d.h)
      this.ctx.restore()
    } else {
      this.ctx.fillStyle = '#1a1d24'
      this.ctx.fillRect(r.x, r.y, r.w, r.h)
    }
    const nome = comNome ? this.nomes.get(id) : null
    if (nome) {
      this.ctx.font = `${Math.max(12, Math.round(r.h * 0.09))}px sans-serif`
      this.ctx.fillStyle = 'rgba(255,255,255,0.92)'
      this.ctx.fillText(nome, r.x + 8, r.y + r.h - 10, Math.max(0, r.w - 16))
    }
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
