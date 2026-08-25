/**
 * i18n PT/EN/FR — estrutura de chaves espelhada do protótipo
 * ("Delonix Meet — completo.html": nav/hero/login/dash/call/…).
 * O idioma persiste em localStorage ('dx_lang'); PT é o predefinido.
 *
 * CARREGAMENTO (achado 1.3 do docs/ux-perf-review.md): os três dicionários
 * viviam aqui inline — 98,9 KB de fonte no chunk de arranque, dos quais dois
 * terços nunca eram lidos numa sessão. Agora só o PT entra no bundle inicial
 * (é o idioma por omissão E o fallback, por isso tem de estar sempre presente);
 * EN e FR chegam por `import()` quando forem escolhidos.
 */
import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'
import pt from './locales/pt'

export type Lang = 'pt' | 'en' | 'fr'

const LANGS: readonly Lang[] = ['pt', 'en', 'fr']

/** Dicionários que o browser vai buscar só quando o idioma for pedido. */
const LOADERS: Record<Exclude<Lang, 'pt'>, () => Promise<{ default: unknown }>> = {
  en: () => import('./locales/en'),
  fr: () => import('./locales/fr'),
}

/** PT já está no bundle; os outros entram aqui à medida que carregam. */
const loaded = new Set<Lang>(['pt'])

function isLang(v: string | null | undefined): v is Lang {
  return !!v && (LANGS as readonly string[]).includes(v)
}

/** O idioma guardado (ou PT). */
export function storedLanguage(): Lang {
  const v = localStorage.getItem('dx_lang')
  return isLang(v) ? v : 'pt'
}

// Arranca SEMPRE em PT: é o único dicionário disponível de forma síncrona.
// Se o idioma guardado for outro, o initLanguage() abaixo troca antes do render.
i18n.use(initReactI18next).init({
  resources: { pt: { translation: pt } },
  lng: 'pt',
  fallbackLng: 'pt',
  interpolation: { escapeValue: false },
})

/** Garante que o dicionário de `lang` está registado no i18next. */
async function ensureLoaded(lang: Lang): Promise<void> {
  if (loaded.has(lang)) return
  const mod = await LOADERS[lang as Exclude<Lang, 'pt'>]()
  i18n.addResourceBundle(lang, 'translation', mod.default, true, true)
  loaded.add(lang)
}

/**
 * Troca de idioma. Assíncrona porque EN/FR podem ter de ser transferidos —
 * quem chama não precisa de esperar: o react-i18next re-renderiza no
 * evento `languageChanged`.
 */
export async function setLanguage(lang: Lang): Promise<void> {
  localStorage.setItem('dx_lang', lang)
  try {
    await ensureLoaded(lang)
  } catch {
    // Rede em baixo a meio da troca: fica-se no idioma atual em vez de mostrar
    // chaves cruas. A preferência guardada aplica-se no próximo arranque.
    return
  }
  await i18n.changeLanguage(lang)
  // O `lang` do documento acompanha o idioma (leitores de ecrã e hifenização
  // usam-no) — estava fixo em `pt` no index.html. Achado 1.6, de graça aqui.
  document.documentElement.lang = lang
}

/**
 * Resolve o idioma de arranque ANTES do primeiro render. Para PT resolve no
 * mesmo tick (zero custo); para EN/FR espera pelo dicionário, o que evita o
 * flash de português que um `changeLanguage` depois do render daria.
 */
export async function initLanguage(preferred?: string | null): Promise<void> {
  const lang = isLang(preferred) ? preferred : storedLanguage()
  document.documentElement.lang = lang
  if (lang === 'pt') return
  await setLanguage(lang)
}

export default i18n
