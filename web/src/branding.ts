// Branding personalizável pelo utilizador (persistido localmente): nome da
// aplicação e imagem de fundo do ecrã de login (aplicada desfocada).
const NAME_KEY = 'dx_app_name'
const BG_KEY = 'dx_login_bg'
const DEFAULT_NAME = 'Delonix Meet'

export function getAppName(): string {
  return localStorage.getItem(NAME_KEY) || DEFAULT_NAME
}
export function setAppName(name: string) {
  const v = name.trim()
  if (v && v !== DEFAULT_NAME) localStorage.setItem(NAME_KEY, v)
  else localStorage.removeItem(NAME_KEY)
  document.title = getAppName()
  window.dispatchEvent(new Event('dx-branding'))
}

/** Devolve [primeira palavra, resto] para estilizar a 2ª parte (ex.: "Meet"). */
export function appNameParts(): [string, string] {
  const name = getAppName()
  const i = name.indexOf(' ')
  return i === -1 ? [name, ''] : [name.slice(0, i), name.slice(i + 1)]
}

export function getLoginBg(): string | null {
  return localStorage.getItem(BG_KEY)
}
export function setLoginBg(dataUrl: string | null) {
  if (dataUrl) localStorage.setItem(BG_KEY, dataUrl)
  else localStorage.removeItem(BG_KEY)
  window.dispatchEvent(new Event('dx-branding'))
}
