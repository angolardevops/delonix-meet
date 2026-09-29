/**
 * Contrato «Editar no Studio» entre o leitor (`frontend/l1-diagramas`) e o
 * editor do Estúdio (`frontend/l1-editor`).
 *
 *   #/studio?vista=edicao&gravacao=<id da gravação>
 *
 * Quem ABRE (leitor, biblioteca) só navega para este endereço: não descarrega
 * nada nem passa blobs pela URL. Quem RECEBE (o Estúdio: `vista` escolhe o
 * editor, o `EditPanel` lê o id com `studioEditTarget(location.hash)`) pede a
 * gravação com `getRecording` — é aí que o servidor decide se a pessoa a pode
 * ver —, descarrega-a com `recordingObjectUrl(rec)` e abre o projecto.
 * O parâmetro chamava-se `editar` e ninguém do lado do Estúdio o lia: o botão
 * abria o Estúdio na emissão, sem gravação nenhuma.
 * Um id que não está na biblioteca é «gravação indisponível», nunca um erro
 * mudo. O parâmetro sai do endereço (`history.replaceState`) depois de lido,
 * para um recarregar não reabrir a mesma importação.
 *
 * Os dois ramos importam ESTE ficheiro: o nome do parâmetro não se escreve
 * em mais lado nenhum.
 */
export const STUDIO_EDIT_PARAM = 'gravacao'

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i

export function studioEditHash(recordingId: string): string {
  return `#/studio?vista=edicao&${STUDIO_EDIT_PARAM}=${encodeURIComponent(recordingId)}`
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
