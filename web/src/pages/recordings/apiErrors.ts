/**
 * Códigos estáveis das recusas das gravações → chave de tradução.
 *
 * Só entram aqui os códigos que o servidor EMITE mesmo (`server/src/recordings.rs`,
 * `server/src/recording_meta.rs`): um código inventado nunca casa, e o ecrã
 * acabaria a mostrar a mensagem de recurso em vez da recusa verdadeira. Um
 * código que não está aqui usa a mensagem do servidor (`error`) ou a de recurso.
 *
 * Um `403` destes NÃO é sessão morta: a pessoa VÊ o recurso e não pode a acção.
 * Quem não o pode ver leva `404` — o servidor não distingue «não existe» de
 * «não é para ti», e o ecrã também não (R153, R181, R234–R237).
 */
import type { TFunction } from 'i18next'
import { ApiError, apiErrorMessage } from '../../api'

const CODES: Record<string, string> = {
  'recording.not_manager': 'player.erros.naoGere',
  'recording.not_comment_author': 'player.erros.naoAutor',
  'recording.transcript_forbidden': 'player.erros.semTranscricao',
  'recording.participants_forbidden': 'player.erros.semParticipantes',
  'recording.download_forbidden': 'player.erros.semDescarga',
  'recording.too_many_chapters': 'player.erros.demasiadosCapitulos',
}

export function recordingErrorMessage(e: unknown, t: TFunction, fallbackKey: string): string {
  const code = e instanceof ApiError ? (e.body as { code?: string } | null)?.code : undefined
  if (code && CODES[code]) return t(CODES[code])
  return apiErrorMessage(e, t(fallbackKey))
}
