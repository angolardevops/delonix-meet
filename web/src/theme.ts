/**
 * Tema da consola: claro (o do template para entrada e gestão) ou escuro.
 * Não depende do React — o main.tsx aplica-o antes do primeiro pixel.
 *
 * A sala, a pré-entrada e o estúdio de emissão NÃO seguem isto: estão
 * dentro de `.dx-stage`, que reafirma o escuro (ver ui/tokens.css).
 */
export type Theme = 'light' | 'dark'

const KEY = 'dx_theme'

export function storedTheme(): Theme {
  try {
    return localStorage.getItem(KEY) === 'dark' ? 'dark' : 'light'
  } catch {
    return 'light'
  }
}

export function applyTheme(theme: Theme) {
  document.documentElement.dataset.theme = theme
  try {
    localStorage.setItem(KEY, theme)
  } catch {
    /* armazenamento bloqueado — o tema vale só para esta sessão */
  }
}

export function initTheme() {
  applyTheme(storedTheme())
}
