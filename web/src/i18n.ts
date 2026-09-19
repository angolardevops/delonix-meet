/**
 * i18n pt-AO / en / fr-FR / zh-CN. O português de base é o de Angola.
 *
 * Ordem de escolha (a primeira que der uma língua suportada ganha):
 *   1. a escolha explícita feita NESTE browser (localStorage `dx_lang`);
 *   2. a língua guardada na conta — excepto `pt`, que é o valor por omissão
 *      do servidor e não distingue «escolheu português» de «nunca escolheu»;
 *   3. `navigator.languages`, pela ordem do browser;
 *   4. pt-AO.
 *
 * pt-AO é o fallback e por isso o único dicionário no chunk de arranque — EN,
 * FR e ZH chegam por `import()` quando forem escolhidos. Os dicionários vivem
 * em `locales/<pasta>/<área>.ts` (a pasta `pt` é o pt-AO, `fr` o fr-FR e `zh`
 * o zh-CN) e `index.ts` compõe-nos.
 */
import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'
import pt from './locales/pt'

export type Lang = 'pt-AO' | 'en' | 'fr-FR' | 'zh-CN'

export const LANGS: readonly Lang[] = ['pt-AO', 'en', 'fr-FR', 'zh-CN']

/** Nome de cada língua NA própria língua — quem não lê português tem de o reconhecer. */
export const LANG_NAMES: Record<Lang, string> = {
  'pt-AO': 'Português',
  en: 'English',
  'fr-FR': 'Français',
  'zh-CN': '简体中文',
}

const LOADERS: Record<Exclude<Lang, 'pt-AO'>, () => Promise<{ default: unknown }>> = {
  en: () => import('./locales/en'),
  'fr-FR': () => import('./locales/fr'),
  'zh-CN': () => import('./locales/zh'),
}

const loaded = new Set<Lang>(['pt-AO'])

/**
 * Normaliza uma etiqueta BCP 47 (ou um valor antigo: `pt`, `fr`) para uma
 * língua suportada. `pt`, `pt-PT` e `pt-BR` resolvem para pt-AO; qualquer
 * `zh-*` para o chinês simplificado (é o único que existe).
 */
export function resolveLang(tag: string | null | undefined): Lang | null {
  const base = (tag ?? '').trim().toLowerCase().split(/[-_]/)[0]
  if (base === 'pt') return 'pt-AO'
  if (base === 'en') return 'en'
  if (base === 'fr') return 'fr-FR'
  if (base === 'zh') return 'zh-CN'
  return null
}

/** Língua activa, sempre uma das suportadas. */
export function currentLang(): Lang {
  return resolveLang(i18n.language) ?? 'pt-AO'
}

/** Locale para `Intl`/`toLocale*` a partir da língua (o inglês em 24 h, como antes). */
export function intlLocale(lang: string = i18n.language): string {
  const l = resolveLang(lang) ?? 'pt-AO'
  return l === 'en' ? 'en-GB' : l
}

/** O código que o servidor aceita em `locale` (hoje só `pt`, `en`, `fr`). */
export function serverLocale(lang: Lang): string | null {
  return lang === 'zh-CN' ? null : lang.slice(0, 2)
}

export function storedLanguage(): Lang | null {
  try {
    return resolveLang(localStorage.getItem('dx_lang'))
  } catch {
    return null
  }
}

function browserLanguage(): Lang | null {
  if (typeof navigator === 'undefined') return null
  const list = navigator.languages?.length ? navigator.languages : [navigator.language]
  for (const tag of list) {
    const l = resolveLang(tag)
    if (l) return l
  }
  return null
}

/** Língua a usar no arranque — ver a ordem no topo do ficheiro. */
export function detectLanguage(account?: string | null): Lang {
  const fromAccount = resolveLang(account)
  const accountChoice = fromAccount && fromAccount !== 'pt-AO' ? fromAccount : null
  return storedLanguage() ?? accountChoice ?? browserLanguage() ?? 'pt-AO'
}

i18n.use(initReactI18next).init({
  resources: { 'pt-AO': { translation: pt } },
  lng: 'pt-AO',
  fallbackLng: 'pt-AO',
  interpolation: { escapeValue: false },
})

async function ensureLoaded(lang: Lang): Promise<void> {
  if (loaded.has(lang)) return
  const mod = await LOADERS[lang as Exclude<Lang, 'pt-AO'>]()
  i18n.addResourceBundle(lang, 'translation', mod.default, true, true)
  loaded.add(lang)
}

/** Troca de língua. `remember` guarda-a como escolha explícita deste browser. */
export async function setLanguage(lang: Lang, remember = true): Promise<void> {
  if (remember) {
    try {
      localStorage.setItem('dx_lang', lang)
    } catch {
      /* sem armazenamento: vale para esta sessão */
    }
  }
  try {
    await ensureLoaded(lang)
  } catch {
    // Rede em baixo a meio da troca: fica-se no idioma actual em vez de
    // mostrar chaves cruas.
    return
  }
  await i18n.changeLanguage(lang)
  document.documentElement.lang = lang
}

/** Resolve o idioma ANTES do primeiro render — sem flash de português. */
export async function initLanguage(account?: string | null): Promise<void> {
  const lang = detectLanguage(account)
  document.documentElement.lang = lang
  if (lang === 'pt-AO') return
  // Detectada, não escolhida: não se grava — senão o browser deixava de contar.
  await setLanguage(lang, false)
}

export default i18n
