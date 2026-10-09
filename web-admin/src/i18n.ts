/**
 * i18n do backoffice — só pt-AO e en, ao contrário das quatro línguas da
 * consola de tenant (`web/`). pt-AO é o fallback e por isso o único
 * dicionário no chunk de arranque; o inglês chega por `import()`.
 */
import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'
import pt from './locales/pt'

export type Lang = 'pt-AO' | 'en'

export const LANGS: readonly Lang[] = ['pt-AO', 'en']

export const LANG_NAMES: Record<Lang, string> = {
  'pt-AO': 'Português',
  en: 'English',
}

function resolveLang(tag: string | null | undefined): Lang | null {
  const base = (tag ?? '').trim().toLowerCase().split(/[-_]/)[0]
  if (base === 'pt') return 'pt-AO'
  if (base === 'en') return 'en'
  return null
}

export function currentLang(): Lang {
  return resolveLang(i18n.language) ?? 'pt-AO'
}

function storedLanguage(): Lang | null {
  try {
    return resolveLang(localStorage.getItem('dxa_lang'))
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

function detectLanguage(): Lang {
  return storedLanguage() ?? browserLanguage() ?? 'pt-AO'
}

i18n.use(initReactI18next).init({
  resources: { 'pt-AO': { translation: pt } },
  lng: 'pt-AO',
  fallbackLng: 'pt-AO',
  interpolation: { escapeValue: false },
})

async function ensureLoaded(lang: Lang): Promise<void> {
  if (lang === 'pt-AO') return
  if (i18n.hasResourceBundle(lang, 'translation')) return
  const mod = await import('./locales/en')
  i18n.addResourceBundle(lang, 'translation', mod.default, true, true)
}

export async function setLanguage(lang: Lang, remember = true): Promise<void> {
  if (remember) {
    try {
      localStorage.setItem('dxa_lang', lang)
    } catch {
      /* sem armazenamento: vale para esta sessão */
    }
  }
  try {
    await ensureLoaded(lang)
  } catch {
    return
  }
  await i18n.changeLanguage(lang)
  document.documentElement.lang = lang
}

/** Resolve o idioma ANTES do primeiro render — sem flash de português. */
export async function initLanguage(): Promise<void> {
  const lang = detectLanguage()
  document.documentElement.lang = lang
  if (lang === 'pt-AO') return
  await setLanguage(lang, false)
}

export default i18n
