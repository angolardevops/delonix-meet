/**
 * Contrato «Editar no Studio» entre o leitor (`frontend/l1-diagramas`) e o
 * editor do Estúdio (`frontend/l1-editor`).
 *
 *   #/studio?editar=<id da gravação>
 *
 * Quem ABRE (leitor, biblioteca) só navega para este endereço: não descarrega
 * nada nem passa blobs pela URL. Quem RECEBE (o Estúdio) lê o id com
 * `studioEditTarget(location.hash)`, procura a gravação em
 * `recordingsLibrary()` — é aí que o servidor decide se a pessoa a pode ver —,
 * descarrega-a com `recordingObjectUrl(rec)` e abre o projecto de edição.
 * Um id que não está na biblioteca é «gravação indisponível», nunca um erro
 * mudo. O parâmetro sai do endereço (`history.replaceState`) depois de lido,
 * para um recarregar não reabrir a mesma importação.
 *
 * Os dois ramos importam ESTE ficheiro: o nome do parâmetro não se escreve
 * em mais lado nenhum.
 */
export const STUDIO_EDIT_PARAM = 'editar'

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i

export function studioEditHash(recordingId: string): string {
  return `#/studio?${STUDIO_EDIT_PARAM}=${encodeURIComponent(recordingId)}`
}

/** Id da gravação a abrir no editor, ou `null` se o endereço não o pede. */
export function studioEditTarget(hash: string): string | null {
  const m = /^#\/studio\?(.*)$/.exec(hash)
  if (!m) return null
  const id = new URLSearchParams(m[1]).get(STUDIO_EDIT_PARAM)
  return id && UUID.test(id) ? id.toLowerCase() : null
}

/** Endereço do leitor em página inteira. */
export function playerHash(recordingId: string): string {
  return `#/recordings/${encodeURIComponent(recordingId)}`
}
