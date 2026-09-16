/** Formatos da Análise: números no idioma activo e variação vs. período anterior. */
import { useTranslation } from 'react-i18next'
import { ApiError } from '../../api'

export function useNumFmt() {
  const { i18n } = useTranslation()
  const tag = i18n.language.startsWith('en') ? 'en-GB' : i18n.language.startsWith('fr') ? 'fr-FR' : 'pt-PT'
  return {
    tag,
    n: (v: number, digits = 0) => v.toLocaleString(tag, { maximumFractionDigits: digits }),
    week: (iso: string) => new Date(iso).toLocaleDateString(tag, { day: 'numeric', month: 'short' }),
  }
}

/**
 * Variação percentual face aos 30 dias anteriores. Sem base (0 antes) não há
 * variação: `null`, e o ecrã não mostra chip — um «+∞%» não informa ninguém.
 */
export function delta(cur: number, prev: number): number | null {
  return prev > 0 ? Math.round(((cur - prev) / prev) * 100) : null
}

/** Bytes em GB (≥ 1 GiB) ou MB — só o número e a unidade, a frase vem do t(). */
export function bytesParts(b: number): { v: number; unit: 'gb' | 'mb' } {
  return b >= 1024 ** 3 ? { v: b / 1024 ** 3, unit: 'gb' } : { v: b / 1024 ** 2, unit: 'mb' }
}

/** O 403 lê-se como frase no idioma da pessoa, não como o texto cru do servidor. */
export function forbiddenAsMessage(msg: string) {
  return (e: unknown): never => {
    if (e instanceof ApiError && e.status === 403) throw new Error(msg)
    throw e
  }
}
