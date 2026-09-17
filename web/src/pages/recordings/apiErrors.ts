/**
 * Códigos estáveis das recusas das gravações (#93) → chave de tradução.
 * Um código que não está aqui usa a mensagem do servidor ou a de recurso.
 */
import type { TFunction } from 'i18next'
import { ApiError, apiErrorMessage } from '../../api'

const CODES: Record<string, string> = {
  'recording.chapter_timestamp_taken': 'player.erros.capituloRepetido',
  'recording.no_file': 'player.erros.semFicheiro',
  'recording.not_published': 'player.erros.naoPublicada',
  'recording.not_owner': 'player.erros.naoDono',
}

export function recordingErrorMessage(e: unknown, t: TFunction, fallbackKey: string): string {
  const code = e instanceof ApiError ? (e.body as { code?: string } | null)?.code : undefined
  if (code && CODES[code]) return t(CODES[code])
  return apiErrorMessage(e, t(fallbackKey))
}
