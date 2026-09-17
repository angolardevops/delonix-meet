/**
 * Guarda dos atalhos globais de teclado.
 *
 * A regra é a do Odoo: Ctrl/Cmd+K abre a pesquisa em QUALQUER ecrã — consola,
 * sala, Estúdio — excepto quando a pessoa está a escrever. «A escrever» é o
 * foco (ou o alvo do evento) num `input`, `textarea`, `select`, num elemento
 * `contenteditable`, ou dentro de uma zona marcada com `data-typing` (o quadro
 * e o editor de diagramas marcam assim a caixa onde se escreve texto sobre o
 * canvas, quando essa caixa não é um campo nativo).
 *
 * Funções puras sobre o evento: testam-se sem DOM.
 */

/** O mínimo de um elemento que a guarda precisa de ler (testável sem DOM). */
export interface TypingProbe {
  tagName?: string
  isContentEditable?: boolean
  closest?: (selector: string) => unknown
}

const TYPING_TAGS = new Set(['INPUT', 'TEXTAREA', 'SELECT'])

/** `true` quando o elemento recebe texto — os atalhos não lhe roubam teclas. */
export function isTypingTarget(el: TypingProbe | EventTarget | null | undefined): boolean {
  const e = el as TypingProbe | null | undefined
  if (!e || typeof e !== 'object') return false
  if (e.tagName && TYPING_TAGS.has(e.tagName.toUpperCase())) return true
  if (e.isContentEditable) return true
  if (typeof e.closest === 'function' && e.closest('[data-typing]')) return true
  return false
}

export interface ShortcutEvent {
  key: string
  ctrlKey: boolean
  metaKey: boolean
  altKey: boolean
  shiftKey: boolean
  defaultPrevented?: boolean
  isComposing?: boolean
  target: EventTarget | TypingProbe | null
}

/**
 * Ctrl+K (ou Cmd+K) sem Alt nem Shift, fora de um campo de texto. Também se
 * lê o elemento com foco: um evento sintético ou reencaminhado pode vir com
 * `target` no `body` enquanto o foco está num campo.
 */
export function isPaletteShortcut(e: ShortcutEvent, active: TypingProbe | Element | null = activeElement()): boolean {
  if (e.defaultPrevented || e.isComposing) return false
  if (!(e.ctrlKey || e.metaKey) || e.altKey || e.shiftKey) return false
  if (e.key.toLowerCase() !== 'k') return false
  return !isTypingTarget(e.target) && !isTypingTarget(active)
}

function activeElement(): Element | null {
  return typeof document === 'undefined' ? null : document.activeElement
}
