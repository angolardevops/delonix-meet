/**
 * Tema — extraído do Shell (achado 1.2 do docs/ux-perf-review.md).
 *
 * Vivia em components/Shell.tsx, o que obrigava o main.tsx a importar a consola
 * INTEIRA (CommandPalette, NotificationCenter, OnboardingTour, SettingsModal…)
 * só para aplicar o tema guardado antes do primeiro pixel. Aqui não depende de
 * nada — nem sequer do React.
 */
export type Theme = 'default' | 'delonix-light'

export function applyTheme(theme: Theme) {
  if (theme === 'default') delete document.documentElement.dataset.theme
  else document.documentElement.dataset.theme = theme
  localStorage.setItem('dx_theme', theme)
}

export function storedTheme(): Theme {
  return localStorage.getItem('dx_theme') === 'delonix-light' ? 'delonix-light' : 'default'
}

export function initTheme() {
  applyTheme(storedTheme())
}
