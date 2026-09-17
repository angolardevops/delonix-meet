/**
 * A mesa de corte, sem canvas nem DOM — o estado de PROGRAMA e PRÉ, as
 * transições (cortar, misturar, limpar, stinger), a T-bar e o AUTO.
 *
 * O compositor lê `quadroDaMesa(estado, agora)` a cada frame e desenha o que
 * isso disser; a interface só muda o estado com as funções daqui. Tudo é
 * imutável e testável em Node: uma transição é um intervalo de tempo, não um
 * `setInterval` a mexer em números.
 *
 * Um PLANO é o que vai para o ar: uma fonte sozinha, ou várias arrumadas num
 * layout (a «entrevista a dois» é um plano com duas fontes lado a lado). O
 * barramento põe planos de uma fonte; as macros podem pôr planos compostos.
 */

export type TipoDeTransicao = 'cortar' | 'misturar' | 'limpar' | 'stinger'

export const TRANSICOES: readonly TipoDeTransicao[] = ['cortar', 'misturar', 'limpar', 'stinger']

/** Como as fontes de um plano se arrumam. `janela` = a primeira inteira e a segunda num canto. */
export type LayoutDoPlano = 'solo' | 'lado-a-lado' | 'destaque' | 'grelha' | 'janela'

export interface Plano {
  fontes: string[]
  layout: LayoutDoPlano
}

export interface TransicaoEmCurso {
  tipo: Exclude<TipoDeTransicao, 'cortar'>
  de: Plano | null
  para: Plano
  /** AUTO: começa em `inicio` e dura `duracaoMs`. T-bar: `inicio` = -1 e o progresso é o da barra. */
  inicio: number
  duracaoMs: number
}

export interface EstadoDaMesa {
  programa: Plano | null
  previa: Plano | null
  /** A transição que o AUTO, o enter e a T-bar usam. */
  transicao: TipoDeTransicao
  duracaoMs: number
  /** Posição física da T-bar, 0 (em cima) a 1 (em baixo). */
  tbar: number
  /**
   * A barra acabou a última transição em baixo: a próxima faz-se a SUBIR.
   * É o que uma T-bar física faz — não salta de volta ao topo.
   */
  tbarEmBaixo: boolean
  emCurso: TransicaoEmCurso | null
  /** Quando o programa actual entrou no ar (para o cronómetro do monitor). */
  noArDesde: number
}

export const DURACAO_MINIMA_MS = 100
export const DURACAO_MAXIMA_MS = 5000
export const DURACAO_INICIAL_MS = 600

export const MESA_INICIAL: EstadoDaMesa = {
  programa: null,
  previa: null,
  transicao: 'cortar',
  duracaoMs: DURACAO_INICIAL_MS,
  tbar: 0,
  tbarEmBaixo: false,
  emCurso: null,
  noArDesde: 0,
}

export const planoDe = (id: string): Plano => ({ fontes: [id], layout: 'solo' })

export function mesmoPlano(a: Plano | null, b: Plano | null): boolean {
  if (!a || !b) return a === b
  return a.layout === b.layout && a.fontes.length === b.fontes.length && a.fontes.every((f, i) => f === b.fontes[i])
}

/** A fonte que dá nome a um plano (a primeira). */
export const fontePrincipal = (p: Plano | null): string | null => p?.fontes[0] ?? null

/** Uma fonte está no ar (tally vermelho) ou em pré (tally verde)? Conta a transição a meio. */
export function tallyDe(e: EstadoDaMesa, id: string): 'programa' | 'previa' | 'livre' {
  const noAr = e.programa?.fontes.includes(id) || e.emCurso?.para.fontes.includes(id)
  if (noAr) return 'programa'
  if (e.previa?.fontes.includes(id)) return 'previa'
  return 'livre'
}

export const limitarDuracao = (ms: number) =>
  Math.round(Math.min(DURACAO_MAXIMA_MS, Math.max(DURACAO_MINIMA_MS, Number.isFinite(ms) ? ms : DURACAO_INICIAL_MS)))

// ------------------------------------------------------------------ acções

/** Uma fonte (ou plano) em pré-visualização. Durante uma transição, a pré espera. */
export function porEmPrevia(e: EstadoDaMesa, plano: Plano): EstadoDaMesa {
  if (e.emCurso) return e
  return { ...e, previa: plano }
}

export function escolherTransicao(e: EstadoDaMesa, tipo: TipoDeTransicao): EstadoDaMesa {
  return { ...e, transicao: tipo }
}

export function mudarDuracao(e: EstadoDaMesa, ms: number): EstadoDaMesa {
  return { ...e, duracaoMs: limitarDuracao(ms) }
}

/** Troca programa e pré. Sem pré, não há nada para pôr no ar. */
function trocar(e: EstadoDaMesa, agora: number): EstadoDaMesa {
  if (!e.previa) return e
  return { ...e, programa: e.previa, previa: e.programa, emCurso: null, noArDesde: agora }
}

/** CORTAR: a pré vai para o ar já. Acaba uma transição a meio no destino dela. */
export function cortar(e: EstadoDaMesa, agora: number): EstadoDaMesa {
  if (e.emCurso) return concluir(e, agora)
  return trocar(e, agora)
}

/**
 * Uma fonte DIRECTA ao ar (⇧1–⇧6), sem passar pela pré. O que estava no ar
 * fica em pré — como numa mesa real, para se poder voltar com um corte.
 */
export function directoAoAr(e: EstadoDaMesa, plano: Plano, agora: number): EstadoDaMesa {
  if (mesmoPlano(e.programa, plano)) return e
  return { ...e, previa: e.programa ?? e.previa, programa: plano, emCurso: null, noArDesde: agora }
}

/**
 * AUTO: faz a transição escolhida na duração escolhida. Com CORTAR escolhido,
 * é um corte. `tipo` força outra transição só desta vez (o botão MISTURAR, o
 * enter, as macros).
 */
export function auto(e: EstadoDaMesa, agora: number, tipo: TipoDeTransicao = e.transicao, duracaoMs = e.duracaoMs): EstadoDaMesa {
  if (e.emCurso || !e.previa) return e
  if (tipo === 'cortar') return trocar(e, agora)
  return {
    ...e,
    emCurso: { tipo, de: e.programa, para: e.previa, inicio: agora, duracaoMs: limitarDuracao(duracaoMs) },
  }
}

function concluir(e: EstadoDaMesa, agora: number): EstadoDaMesa {
  const c = e.emCurso
  if (!c) return e
  const manual = c.inicio < 0
  return {
    ...e,
    programa: c.para,
    previa: c.de,
    emCurso: null,
    noArDesde: agora,
    // Uma transição feita pela barra deixa-a no fim; o AUTO também a leva ao
    // fim (é o que uma mesa real faz: a próxima puxada é no sentido inverso).
    tbarEmBaixo: manual ? e.tbar >= 1 : !e.tbarEmBaixo,
    tbar: manual ? e.tbar : e.tbarEmBaixo ? 0 : 1,
  }
}

/** Chamar a cada frame (ou a cada tique): acaba o AUTO que já passou da duração. */
export function avancar(e: EstadoDaMesa, agora: number): EstadoDaMesa {
  const c = e.emCurso
  if (!c || c.inicio < 0) return e
  if (agora - c.inicio >= c.duracaoMs) return concluir(e, agora)
  return e
}

/**
 * Mexer a T-bar para `posicao` (0–1). O progresso da transição é a distância
 * percorrida desde o lado onde a barra estava parada; chegar ao outro lado
 * conclui. Voltar ao ponto de partida anula a transição sem trocar nada.
 */
export function moverTbar(e: EstadoDaMesa, posicao: number, agora: number): EstadoDaMesa {
  const pos = Math.min(1, Math.max(0, Number.isFinite(posicao) ? posicao : 0))
  if (e.emCurso && e.emCurso.inicio >= 0) return e // o AUTO manda até acabar
  const p = e.tbarEmBaixo ? 1 - pos : pos
  const tipo: TransicaoEmCurso['tipo'] = e.transicao === 'cortar' ? 'misturar' : e.transicao
  if (p <= 0) return { ...e, tbar: pos, emCurso: null }
  if (!e.emCurso) {
    if (!e.previa) return { ...e, tbar: e.tbarEmBaixo ? 1 : 0 }
    const emCurso: TransicaoEmCurso = { tipo, de: e.programa, para: e.previa, inicio: -1, duracaoMs: 0 }
    const comeca = { ...e, tbar: pos, emCurso }
    return p >= 1 ? concluir(comeca, agora) : comeca
  }
  const seguinte = { ...e, tbar: pos }
  return p >= 1 ? concluir(seguinte, agora) : seguinte
}

/** Progresso (0–1) da transição em curso. */
export function progresso(e: EstadoDaMesa, agora: number): number {
  const c = e.emCurso
  if (!c) return 0
  if (c.inicio < 0) return e.tbarEmBaixo ? 1 - e.tbar : e.tbar
  return Math.min(1, Math.max(0, (agora - c.inicio) / Math.max(1, c.duracaoMs)))
}

/** O que o compositor desenha neste instante. */
export interface QuadroDaMesa {
  programa: Plano | null
  transicao: { tipo: TransicaoEmCurso['tipo']; de: Plano | null; para: Plano; p: number } | null
}

export function quadroDaMesa(e: EstadoDaMesa, agora: number): QuadroDaMesa {
  const c = e.emCurso
  if (!c) return { programa: e.programa, transicao: null }
  return { programa: c.de, transicao: { tipo: c.tipo, de: c.de, para: c.para, p: progresso(e, agora) } }
}

/**
 * Tira fontes que desapareceram (câmara desligada, convidado que saiu). Um
 * plano que fica sem fontes deixa de existir — o programa não fica a apontar
 * para uma câmara que já não há.
 */
export function limparFontes(e: EstadoDaMesa, existentes: ReadonlySet<string>): EstadoDaMesa {
  const filtra = (p: Plano | null): Plano | null => {
    if (!p) return null
    const fontes = p.fontes.filter((f) => existentes.has(f))
    if (!fontes.length) return null
    return fontes.length === p.fontes.length ? p : { ...p, fontes, layout: fontes.length === 1 ? 'solo' : p.layout }
  }
  const programa = filtra(e.programa)
  const previa = filtra(e.previa)
  const para = e.emCurso ? filtra(e.emCurso.para) : null
  const emCurso = e.emCurso && para ? { ...e.emCurso, de: filtra(e.emCurso.de), para } : null
  if (programa === e.programa && previa === e.previa && emCurso === e.emCurso) return e
  return { ...e, programa, previa, emCurso }
}

// ------------------------------------------------------------------ desenho

export interface Rect {
  x: number
  y: number
  w: number
  h: number
}

/**
 * O rectângulo da janela no canto (layout `janela`): um quarto da largura,
 * em baixo à direita, com a margem do avatar do Estúdio.
 */
export function rectDaJanela(W: number, H: number): Rect {
  const w = Math.round(W * 0.28)
  const h = Math.round((w * 9) / 16)
  const m = Math.round(W * 0.03)
  return { x: W - w - m, y: H - h - m, w, h }
}

/** A banda do stinger: entra pela esquerda e sai pela direita; tapa o corte a meio. */
export function bandaDoStinger(p: number, W: number): { x: number; w: number } {
  const w = W * 0.7
  const x = -w + (W + w) * Math.min(1, Math.max(0, p))
  return { x, w }
}

/** No stinger, a imagem troca quando a banda tapa o centro. */
export const stingerJaTrocou = (p: number) => p >= 0.5
