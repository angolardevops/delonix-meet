/**
 * Os atalhos da mesa de corte, lidos de um evento de teclado sem DOM.
 *
 *   1–6       fonte em pré-visualização
 *   ⇧1–⇧6     fonte directa ao ar
 *   espaço    cortar
 *   enter     misturar com a duração actual
 *   W / S     limpar / stinger
 *   ⌥1–⌥4     liga e desliga sobreposições (Alt fora do Mac); ⌘1–⌘4 e
 *             Ctrl+1–4 também, onde o browser os deixa chegar à página
 *   F1–F6     macros
 *
 * Os números lêem-se do `code` (`Digit1`), não da `key`: com ⇧ carregado a
 * `key` de «1» é «!» num teclado inglês e «+» num português.
 *
 * Porque o ⌥/Alt além do ⌘ do template: num separador do Chrome ou do Firefox,
 * ⌘1–⌘4 (Ctrl+1–4 fora do Mac) mudam de separador ANTES de a página ver a
 * tecla — o atalho escrito no template nunca chegava à mesa. Na janela da app
 * instalada (PWA) não há separadores e o ⌘ chega; o ⌥ funciona nos dois.
 */

export type AccaoDaMesa =
  | { tipo: 'previa'; n: number }
  | { tipo: 'ar'; n: number }
  | { tipo: 'cortar' }
  | { tipo: 'misturar' }
  | { tipo: 'limpar' }
  | { tipo: 'stinger' }
  | { tipo: 'sobreposicao'; n: number }
  | { tipo: 'macro'; n: number }

export interface TeclaDaMesa {
  key: string
  code: string
  shiftKey: boolean
  ctrlKey: boolean
  metaKey: boolean
  altKey: boolean
  repeat?: boolean
}

function digito(code: string): number {
  const m = code.match(/^(?:Digit|Numpad)([0-9])$/)
  return m ? Number(m[1]) : 0
}

export function accaoDaTecla(e: TeclaDaMesa): AccaoDaMesa | null {
  if (e.repeat) return null
  const f = e.key.match(/^F([1-6])$/)
  if (f && !e.shiftKey && !e.ctrlKey && !e.metaKey && !e.altKey) return { tipo: 'macro', n: Number(f[1]) }
  const n = digito(e.code)
  if (e.metaKey || e.ctrlKey || e.altKey) {
    if (n >= 1 && n <= 4 && !e.shiftKey) return { tipo: 'sobreposicao', n }
    return null
  }
  if (n >= 1 && n <= 6) return e.shiftKey ? { tipo: 'ar', n } : { tipo: 'previa', n }
  if (e.shiftKey) return null
  if (e.code === 'Space' || e.key === ' ') return { tipo: 'cortar' }
  if (e.key === 'Enter' || e.code === 'NumpadEnter') return { tipo: 'misturar' }
  if (e.code === 'KeyW') return { tipo: 'limpar' }
  if (e.code === 'KeyS') return { tipo: 'stinger' }
  return null
}

/** «⌥» no Mac, «Alt+» nos outros — o modificador das sobreposições que chega sempre à página. */
export function teclaDeSobreposicao(plataforma = typeof navigator !== 'undefined' ? navigator.platform : ''): string {
  return /mac|iphone|ipad/i.test(plataforma) ? '⌥' : 'Alt+'
}
