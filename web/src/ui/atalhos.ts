/**
 * O catálogo único dos atalhos de teclado, com as duas funções puras que os
 * comparam com um evento (`combina`) e os escrevem para o ecrã
 * (`escreverAtalho`).
 *
 * Porque existe um catálogo: os atalhos estavam espalhados por quinze
 * `addEventListener('keydown', …)` e nenhum ecrã sabia dizer quais eram. Um
 * atalho que ninguém consegue descobrir não existe — e foi isso que se media
 * no Estúdio, onde as cinco vistas (emissão, edição, legendas, exportações,
 * TV) só se alcançavam com o rato.
 *
 * Agora cada atalho declara-se AQUI, com o escopo e a chave de tradução; quem
 * o executa pede-o por `id` (`useAtalhos`) e a folha de atalhos («?») lê o
 * MESMO catálogo. Não há segunda lista para derivar.
 *
 * A gramática de uma combinação é `mod+shift+alt+<tecla>`, pela ordem:
 * `mod` é o Ctrl fora do Mac e o ⌘ no Mac (aceitam-se os dois, como no resto
 * da app). A tecla pode ser uma letra (`k`), um dígito (`1`), `?`, `space`,
 * `enter`, `esc` ou `f1`–`f12`.
 *
 * Os dígitos lêem-se do `code` (`Digit1`), nunca da `key`: com ⇧ carregado a
 * `key` de «1» é «!» num teclado inglês e «+» num português (a mesma razão
 * que está escrita em `studio/tv/atalhos.ts`).
 */
import { isTypingTarget, type TypingProbe } from './hotkeys'

/** O mínimo de um `KeyboardEvent` que o comparador lê — testa-se sem DOM. */
export interface EventoDeTecla {
  key: string
  code?: string
  ctrlKey: boolean
  metaKey: boolean
  altKey: boolean
  shiftKey: boolean
  repeat?: boolean
  isComposing?: boolean
  defaultPrevented?: boolean
  target?: EventTarget | TypingProbe | null
}

interface Combinacao {
  mod: boolean
  shift: boolean
  alt: boolean
  tecla: string
}

const NOMEADAS: Record<string, string> = {
  space: ' ',
  enter: 'Enter',
  esc: 'Escape',
  tab: 'Tab',
}

/** `'mod+shift+1'` → `{ mod: true, shift: true, alt: false, tecla: '1' }`. */
export function analisarCombinacao(spec: string): Combinacao {
  const partes = spec.toLowerCase().split('+')
  const tecla = partes.pop() ?? ''
  return {
    mod: partes.includes('mod'),
    shift: partes.includes('shift'),
    alt: partes.includes('alt'),
    tecla,
  }
}

function teclaCombina(tecla: string, e: EventoDeTecla): boolean {
  // O «?» exige ⇧ em quase todos os teclados: a combinação escreve-se só `?`
  // e o ⇧ é indiferente (ver `combina`).
  if (tecla === '?') return e.key === '?' || (e.shiftKey && e.code === 'Slash')
  if (/^[0-9]$/.test(tecla)) {
    if (e.code) return e.code === `Digit${tecla}` || e.code === `Numpad${tecla}`
    return e.key === tecla
  }
  if (/^f([1-9]|1[0-2])$/.test(tecla)) return e.key.toLowerCase() === tecla
  const nomeada = NOMEADAS[tecla]
  if (nomeada) return e.key === nomeada || (nomeada === ' ' && e.code === 'Space')
  return e.key.toLowerCase() === tecla
}

/**
 * O evento é esta combinação? Falso para uma tecla repetida, para um evento já
 * tratado, a meio de uma composição (IME) ou quando o alvo recebe texto — um
 * atalho nunca rouba uma tecla a quem está a escrever.
 */
export function combina(spec: string, e: EventoDeTecla, activo: TypingProbe | Element | null = elementoActivo()): boolean {
  if (e.repeat || e.isComposing || e.defaultPrevented) return false
  if (isTypingTarget(e.target ?? null) || isTypingTarget(activo)) return false
  const c = analisarCombinacao(spec)
  if (c.mod !== (e.ctrlKey || e.metaKey)) return false
  if (c.alt !== e.altKey) return false
  if (c.tecla !== '?' && c.shift !== e.shiftKey) return false
  return teclaCombina(c.tecla, e)
}

function elementoActivo(): Element | null {
  return typeof document === 'undefined' ? null : document.activeElement
}

const ROTULOS_MAC: Record<string, string> = { mod: '⌘', shift: '⇧', alt: '⌥' }
const ROTULOS_PC: Record<string, string> = { mod: 'Ctrl', shift: 'Shift', alt: 'Alt' }
const TECLAS_LONGAS: Record<string, string> = { space: 'Space', enter: 'Enter', esc: 'Esc', tab: 'Tab' }

/** `true` num Mac, iPhone ou iPad — muda os símbolos dos modificadores. */
export function eMac(plataforma = typeof navigator !== 'undefined' ? navigator.platform : ''): boolean {
  return /mac|iphone|ipad/i.test(plataforma)
}

/**
 * A combinação escrita para o ecrã: `⌘⇧1` no Mac, `Ctrl+Shift+1` nos outros.
 * Não traduz nada — são símbolos de teclado, iguais nas quatro línguas.
 */
export function escreverAtalho(spec: string, mac = eMac()): string {
  const c = analisarCombinacao(spec)
  const rotulos = mac ? ROTULOS_MAC : ROTULOS_PC
  const mods: string[] = []
  if (c.mod) mods.push(rotulos.mod)
  if (c.shift) mods.push(rotulos.shift)
  if (c.alt) mods.push(rotulos.alt)
  const tecla = TECLAS_LONGAS[c.tecla] ?? (/^f([1-9]|1[0-2])$/.test(c.tecla) ? c.tecla.toUpperCase() : c.tecla.toUpperCase())
  return mac ? [...mods, tecla].join('') : [...mods, tecla].join('+')
}

/** A faixa escrita para o ecrã: `⇧1–6`, `Alt+1–4`, `F1–F6`. */
export function escreverFaixa(spec: string, ate: string, mac = eMac()): string {
  return `${escreverAtalho(spec, mac)}–${ate.toUpperCase()}`
}

// ------------------------------------------------------------------ catálogo

/**
 * Onde o atalho vale. `global` é toda a app; `sala` é dentro de uma reunião;
 * `estudio` é o Estúdio inteiro (as vistas); `mesa` é a mesa de corte do
 * estúdio de TV; `quadro` é o quadro branco e o editor de diagramas.
 */
export type EscopoDeAtalho = 'global' | 'sala' | 'estudio' | 'mesa' | 'quadro'

export interface AtalhoDoCatalogo {
  /** Identidade estável — é por aqui que `useAtalhos` pede o atalho. */
  readonly id: string
  readonly escopo: EscopoDeAtalho
  /** A combinação canónica, na gramática do topo do ficheiro. */
  readonly combinacao: string
  /**
   * Chave i18n do que o atalho faz. Onde já existe um rótulo neutro do botão
   * que o atalho aciona, é ESSA chave que se aponta: a folha e o botão dizem
   * a mesma coisa, e um rótulo que mude muda nos dois.
   */
  readonly rotulo: string
  /**
   * A última tecla, quando o atalho é uma FAIXA (`1`…`6`). A `combinacao`
   * continua a ser a primeira — a que se compara — e a folha escreve
   * `⇧1–6` a partir das duas. Não é texto traduzido: são teclas.
   */
  readonly ate?: string
  /** `true` quando o atalho é tratado noutro módulo e aqui só se documenta. */
  readonly documental?: boolean
}

export const CATALOGO_DE_ATALHOS = [
  // -------------------------------------------------------------- global
  { id: 'paleta', escopo: 'global', combinacao: 'mod+k', rotulo: 'ui.atalhos.paleta', documental: true },
  { id: 'rail', escopo: 'global', combinacao: 'mod+b', rotulo: 'ui.atalhos.rail', documental: true },
  { id: 'ajuda', escopo: 'global', combinacao: '?', rotulo: 'ui.atalhos.ajuda' },
  { id: 'fechar', escopo: 'global', combinacao: 'esc', rotulo: 'ui.atalhos.fechar', documental: true },

  // -------------------------------------------------------------- sala
  { id: 'micro', escopo: 'sala', combinacao: 'mod+d', rotulo: 'ui.atalhos.micro' },
  { id: 'camara', escopo: 'sala', combinacao: 'mod+e', rotulo: 'ui.atalhos.camara' },
  { id: 'chat', escopo: 'sala', combinacao: 'alt+c', rotulo: 'room.painel.chat' },
  { id: 'participantes', escopo: 'sala', combinacao: 'alt+p', rotulo: 'room.painel.participantes' },
  { id: 'perguntas', escopo: 'sala', combinacao: 'alt+q', rotulo: 'room.painel.perguntas' },
  { id: 'sondagens', escopo: 'sala', combinacao: 'alt+s', rotulo: 'room.painel.sondagens' },
  { id: 'notas', escopo: 'sala', combinacao: 'alt+n', rotulo: 'room.painel.notas' },

  // -------------------------------------------------------------- estúdio
  // ⌘⇧/Ctrl+⇧ e um dígito: a mesa de corte ignora dígitos com ⇧ E um
  // modificador (ver `accaoDaTecla`), por isso as vistas continuam a mudar
  // com a mesa ao ar — e o portão de `atalhos.test.ts` guarda-o. Ctrl+1–5
  // sem ⇧ nunca chegaria à página: o browser muda de separador antes.
  { id: 'vistaEmissao', escopo: 'estudio', combinacao: 'mod+shift+1', rotulo: 'studio.vistas.emissao' },
  { id: 'vistaEdicao', escopo: 'estudio', combinacao: 'mod+shift+2', rotulo: 'studio.vistas.edicao' },
  { id: 'vistaLegendas', escopo: 'estudio', combinacao: 'mod+shift+3', rotulo: 'editor.topo.legendas' },
  { id: 'vistaExportacoes', escopo: 'estudio', combinacao: 'mod+shift+4', rotulo: 'editor.topo.exportacoes' },
  { id: 'vistaTv', escopo: 'estudio', combinacao: 'mod+shift+5', rotulo: 'studio.vistas.tv' },

  // -------------------------------------------------------------- mesa de corte
  // Tratados em `studio/tv/atalhos.ts`; aqui só se documentam. O portão de
  // `atalhos.test.ts` compara cada um com o que `accaoDaTecla` devolve, para
  // que a folha nunca prometa uma tecla que a mesa não tem.
  { id: 'mesaPrevia', escopo: 'mesa', combinacao: '1', ate: '6', rotulo: 'tv.atalhos.previa', documental: true },
  { id: 'mesaAr', escopo: 'mesa', combinacao: 'shift+1', ate: '6', rotulo: 'tv.atalhos.ar', documental: true },
  { id: 'mesaCortar', escopo: 'mesa', combinacao: 'space', rotulo: 'tv.atalhos.cortar', documental: true },
  { id: 'mesaMisturar', escopo: 'mesa', combinacao: 'enter', rotulo: 'tv.atalhos.misturar', documental: true },
  { id: 'mesaLimpar', escopo: 'mesa', combinacao: 'w', rotulo: 'ui.atalhos.mesaLimpar', documental: true },
  { id: 'mesaStinger', escopo: 'mesa', combinacao: 's', rotulo: 'ui.atalhos.mesaStinger', documental: true },
  { id: 'mesaSobreposicao', escopo: 'mesa', combinacao: 'alt+1', ate: '4', rotulo: 'tv.atalhos.sobreposicoes', documental: true },
  { id: 'mesaMacro', escopo: 'mesa', combinacao: 'f1', ate: 'f6', rotulo: 'ui.atalhos.mesaMacro', documental: true },

  // -------------------------------------------------------------- quadro e diagramas
  { id: 'quadroDesfazer', escopo: 'quadro', combinacao: 'mod+z', rotulo: 'ui.seleccao.desfazer', documental: true },
  { id: 'quadroRefazer', escopo: 'quadro', combinacao: 'mod+shift+z', rotulo: 'ui.seleccao.refazer', documental: true },
  { id: 'quadroAgrupar', escopo: 'quadro', combinacao: 'mod+g', rotulo: 'ui.seleccao.agrupar', documental: true },
  { id: 'quadroDesagrupar', escopo: 'quadro', combinacao: 'mod+shift+g', rotulo: 'ui.seleccao.desagrupar', documental: true },
] as const satisfies readonly AtalhoDoCatalogo[]

export type IdDeAtalho = (typeof CATALOGO_DE_ATALHOS)[number]['id']

/** A ordem em que os escopos aparecem na folha de atalhos. */
export const ESCOPOS: readonly EscopoDeAtalho[] = ['global', 'sala', 'estudio', 'mesa', 'quadro']

/** As teclas de um atalho escritas para o ecrã — com a faixa, quando a tem. */
export function teclasDoAtalho(a: AtalhoDoCatalogo, mac = eMac()): string {
  return a.ate ? escreverFaixa(a.combinacao, a.ate, mac) : escreverAtalho(a.combinacao, mac)
}

export function atalhoPorId(id: IdDeAtalho): AtalhoDoCatalogo {
  const a = CATALOGO_DE_ATALHOS.find((x) => x.id === id)
  // Impossível com o tipo `IdDeAtalho`; a guarda é para quem chamar de JS.
  if (!a) throw new Error(`atalho desconhecido: ${id}`)
  return a
}

/** As entradas de um escopo, com os `id` literais preservados para o hook. */
export function atalhosDoEscopo(escopo: EscopoDeAtalho): readonly (typeof CATALOGO_DE_ATALHOS)[number][] {
  return CATALOGO_DE_ATALHOS.filter((a) => a.escopo === escopo)
}
