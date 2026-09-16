/**
 * i18n PT/EN/FR. O idioma persiste em localStorage ('dx_lang'); PT é o
 * predefinido e o fallback, por isso é o único dicionário no chunk de
 * arranque — EN e FR chegam por `import()` quando forem escolhidos.
 *
 * Os dicionários vivem em `locales/<língua>/<área>.ts`: uma área por ecrã
 * ou família de ecrãs, e `index.ts` compõe-nas. As chaves de uma área são
 * `área.chave` (ex.: `home.proximas`).
 */
import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'
import pt from './locales/pt'

export type Lang = 'pt' | 'en' | 'fr'

export const LANGS: readonly Lang[] = ['pt', 'en', 'fr']

const LOADERS: Record<Exclude<Lang, 'pt'>, () => Promise<{ default: unknown }>> = {
  en: () => import('./locales/en'),
  fr: () => import('./locales/fr'),
}

const loaded = new Set<Lang>(['pt'])

function isLang(v: string | null | undefined): v is Lang {
  return !!v && (LANGS as readonly string[]).includes(v)
}

export function storedLanguage(): Lang {
  try {
    const v = localStorage.getItem('dx_lang')
    return isLang(v) ? v : 'pt'
  } catch {
    return 'pt'
  }
}

i18n.use(initReactI18next).init({
  resources: { pt: { translation: pt } },
  lng: 'pt',
  fallbackLng: 'pt',
  interpolation: { escapeValue: false },
})

async function ensureLoaded(lang: Lang): Promise<void> {
  if (loaded.has(lang)) return
  const mod = await LOADERS[lang as Exclude<Lang, 'pt'>]()
  i18n.addResourceBundle(lang, 'translation', mod.default, true, true)
  loaded.add(lang)
}

export async function setLanguage(lang: Lang): Promise<void> {
  try {
    localStorage.setItem('dx_lang', lang)
  } catch {
    /* sem armazenamento: vale para esta sessão */
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
export async function initLanguage(preferred?: string | null): Promise<void> {
  const lang = isLang(preferred) ? preferred : storedLanguage()
  document.documentElement.lang = lang
  if (lang === 'pt') return
  await setLanguage(lang)
}

export default i18n
