/**
 * Rótulos do catálogo (área de locale `diagramCatalog`), carregados à parte e
 * juntados ao i18next da aplicação — a mesma técnica com que o `i18n.ts`
 * carrega as línguas, mas só quando o editor de diagramas abre.
 */
import i18n from 'i18next'
import { currentLang, Lang } from '../../../i18n'
import { notifyCatalog } from './index'

const LOADERS: Record<Lang, () => Promise<{ default: unknown }>> = {
  'pt-AO': () => import('../../../locales/pt/diagramCatalog'),
  en: () => import('../../../locales/en/diagramCatalog'),
  'fr-FR': () => import('../../../locales/fr/diagramCatalog'),
  'zh-CN': () => import('../../../locales/zh/diagramCatalog'),
}

const done = new Set<Lang>()

export async function loadCatalogLabels(lang: Lang = currentLang()): Promise<void> {
  if (done.has(lang)) return
  const mod = await LOADERS[lang]()
  // O português é a língua de recurso: carrega-se sempre, para nenhum rótulo aparecer como chave.
  if (lang !== 'pt-AO' && !done.has('pt-AO')) await loadCatalogLabels('pt-AO')
  i18n.addResourceBundle(lang, 'translation', { diagramCatalog: mod.default }, true, true)
  done.add(lang)
  notifyCatalog()
}

export const catalogLabelsReady = (lang: Lang = currentLang()) => done.has(lang)
