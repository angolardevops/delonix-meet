/**
 * Macros da mesa de corte (F1–F6): sequências LOCAIS de acções que a mesa já
 * sabe fazer uma a uma. Uma macro não inventa capacidades — cada passo é
 * executado por quem a corre, e um passo que depende de algo que não existe
 * (a luz sem agente, a fonte 5 que não está ligada) fica marcado como
 * indisponível, com a razão, e a macro continua.
 *
 * Sem DOM e sem Web Audio: o executor é injectado, por isso testa-se em Node.
 */
import type { LayoutDoPlano, TipoDeTransicao } from './mesa'

export type Sobreposicao = 'legenda' | 'logotipo' | 'relogio' | 'sondagem'
export const SOBREPOSICOES_DA_MESA: readonly Sobreposicao[] = ['legenda', 'logotipo', 'relogio', 'sondagem']

export type Passo =
  /** Pré-visualização de um plano feito das fontes do barramento `numeros` (1–6). */
  | { tipo: 'previa'; numeros: number[]; layout: LayoutDoPlano }
  | { tipo: 'transicao'; transicao: TipoDeTransicao; duracaoMs?: number }
  | { tipo: 'sobreposicao'; qual: Sobreposicao; ligada: boolean }
  /** `marca` = o cartão de espera com a marca; `fontes` = volta às câmaras. */
  | { tipo: 'conteudo'; conteudo: 'fontes' | 'marca' }
  | { tipo: 'musica'; db: number }
  | { tipo: 'microfones'; mudos: boolean }
  | { tipo: 'esperar'; ms: number }
  | { tipo: 'luz'; cena: string }
  | { tipo: 'terminarEmissao' }
  | { tipo: 'pararGravacao' }

export interface Macro {
  /** F1–F6. */
  tecla: number
  /** Chave i18n do nome e da nota (`studio.tv.macros.<id>.nome`). */
  id: string
  passos: Passo[]
}

/**
 * As macros de partida, as do template, feitas só de passos que existem. A
 * «entrevista a dois» usa CAM 2 e CAM 3; os «diapositivos» o ecrã (fonte 5)
 * em grande com a apresentadora (fonte 2) numa janela — a numeração é a do
 * barramento, que a pessoa arruma nas Fontes.
 */
export const MACROS_INICIAIS: readonly Macro[] = [
  {
    tecla: 1,
    id: 'abertura',
    passos: [
      { tipo: 'conteudo', conteudo: 'fontes' },
      { tipo: 'musica', db: -18 },
      { tipo: 'sobreposicao', qual: 'legenda', ligada: true },
      { tipo: 'transicao', transicao: 'stinger' },
    ],
  },
  {
    tecla: 2,
    id: 'entrevista',
    passos: [
      { tipo: 'conteudo', conteudo: 'fontes' },
      { tipo: 'previa', numeros: [2, 3], layout: 'lado-a-lado' },
      { tipo: 'luz', cena: 'entrevista' },
      { tipo: 'transicao', transicao: 'misturar' },
    ],
  },
  {
    tecla: 3,
    id: 'diapositivos',
    passos: [
      { tipo: 'conteudo', conteudo: 'fontes' },
      { tipo: 'previa', numeros: [5, 2], layout: 'janela' },
      { tipo: 'transicao', transicao: 'misturar' },
    ],
  },
  {
    tecla: 4,
    id: 'intervalo',
    passos: [
      { tipo: 'conteudo', conteudo: 'marca' },
      { tipo: 'musica', db: -12 },
      { tipo: 'microfones', mudos: true },
    ],
  },
  {
    tecla: 6,
    id: 'encerrar',
    passos: [
      { tipo: 'conteudo', conteudo: 'marca' },
      { tipo: 'microfones', mudos: true },
      { tipo: 'esperar', ms: 3000 },
      { tipo: 'terminarEmissao' },
      { tipo: 'pararGravacao' },
    ],
  },
]

/** O que aconteceu a cada passo. `indisponivel` leva a razão (chave i18n). */
export type ResultadoDoPasso = { estado: 'feito' } | { estado: 'indisponivel'; razao: string }

export type Executor = (passo: Passo, sinal: AbortSignal) => Promise<ResultadoDoPasso> | ResultadoDoPasso

export interface ProgressoDaMacro {
  tecla: number
  indice: number
  total: number
  resultados: ResultadoDoPasso[]
}

export const esperar = (ms: number, sinal: AbortSignal) =>
  new Promise<void>((res, rej) => {
    if (sinal.aborted) return rej(new DOMException('abortada', 'AbortError'))
    const t = setTimeout(res, ms)
    sinal.addEventListener(
      'abort',
      () => {
        clearTimeout(t)
        rej(new DOMException('abortada', 'AbortError'))
      },
      { once: true },
    )
  })

/**
 * Corre uma macro passo a passo. Os passos `esperar` são do próprio corredor;
 * o resto vai ao executor, que só resolve uma transição quando ela ACABA —
 * senão a macro «Encerrar» parava a emissão a meio do fundido.
 */
export async function correrMacro(
  macro: Macro,
  executar: Executor,
  sinal: AbortSignal,
  aoAvancar?: (p: ProgressoDaMacro) => void,
): Promise<ResultadoDoPasso[]> {
  const resultados: ResultadoDoPasso[] = []
  for (let i = 0; i < macro.passos.length; i++) {
    if (sinal.aborted) break
    const passo = macro.passos[i]
    aoAvancar?.({ tecla: macro.tecla, indice: i, total: macro.passos.length, resultados: [...resultados] })
    let r: ResultadoDoPasso
    if (passo.tipo === 'esperar') {
      await esperar(passo.ms, sinal)
      r = { estado: 'feito' }
    } else {
      r = await executar(passo, sinal)
    }
    resultados.push(r)
  }
  aoAvancar?.({ tecla: macro.tecla, indice: macro.passos.length, total: macro.passos.length, resultados: [...resultados] })
  return resultados
}

export const macroDaTecla = (macros: readonly Macro[], n: number) => macros.find((m) => m.tecla === n) ?? null

/** dB → ganho linear, com −∞ para o silêncio. */
export const dbParaLinear = (db: number) => (db <= -120 ? 0 : Math.pow(10, db / 20))
