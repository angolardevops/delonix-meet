/**
 * As rotas da aplicação, lidas do endereço.
 *
 * Saiu do `App.tsx` a 2026-10-06 por uma razão concreta: enquanto viveu lá
 * dentro **não teve um único teste**, e tinha três defeitos que só se vêem numa
 * tabela de casos (ver `rota.test.ts`):
 *
 * - um link de sala em MAIÚSCULAS abria o Início — e o `roomCode.ts` já
 *   normalizava o que se cola na caixa, só não o que vem no endereço;
 * - um parâmetro a mais (`?utm_source=email`, de um encurtador ou de um cliente
 *   de correio) abria o Início;
 * - `#/adminzinho` abria a Administração, porque os nomes de página casavam por
 *   prefixo e não por segmento.
 *
 * E um endereço desconhecido mostrava o Início **sem o dizer**, com a barra a
 * mostrar outra coisa. Passa a haver `desconhecida`.
 *
 * O que NÃO mudou de propósito: os tokens (convite, partilha, diagrama) são
 * case-sensitive e continuam a ser casados como estão — normalizar um token é
 * perdê-lo. Só os CÓDIGOS (sala, sala de espera, telemóvel) se normalizam.
 */
import type { NavKey } from './components/Shell'

export type Route =
  | { kind: NavKey }
  | { kind: 'room'; code: string; voice: boolean }
  | { kind: 'lobby'; code: string }
  | { kind: 'telemovel'; code: string }
  | { kind: 'share'; token: string }
  | { kind: 'invite'; token: string }
  | { kind: 'diagram'; id: string | null }
  | { kind: 'player'; id: string }
  /** O endereço não é nenhuma rota. Mostra-se e diz-se — não se finge o Início. */
  | { kind: 'desconhecida'; endereco: string }

export const PAGES: NavKey[] = [
  'calendar',
  'rooms',
  'studio',
  'recordings',
  'whiteboards',
  'directory',
  'integrations',
  'analytics',
  'admin',
  'telecom',
  'ai',
]

/** `?voice` e `?voice=1` valem os dois; `?voice=0` e `?voice=false` não. */
function querVoz(query: string | undefined): boolean {
  if (query === undefined) return false
  const p = new URLSearchParams(query)
  if (!p.has('voice')) return false
  const v = p.get('voice')
  return v === null || v === '' || (v !== '0' && v.toLowerCase() !== 'false')
}

/**
 * Um nome de página casa por SEGMENTO: `#/admin`, `#/admin?x=1`, `#/admin/algo`.
 * Nunca `#/adminzinho`, que até 2026-10-06 abria a Administração.
 */
function paginaDe(h: string): NavKey | null {
  for (const p of PAGES) {
    if (h === `#/${p}` || h.startsWith(`#/${p}?`) || h.startsWith(`#/${p}/`)) return p
  }
  return null
}

export function parseHash(hash: string = location.hash): Route {
  const h = hash
  // Um hash que não é um caminho (`#conteudo`, de uma âncora na página) não é
  // uma rota: é o Início, como sempre foi.
  if (h === '' || h === '#' || h === '#/' || !h.startsWith('#/')) return { kind: 'home' }

  const room = h.match(/^#\/r\/([A-Za-z-]+)(?:\?(.*))?$/)
  if (room) return { kind: 'room', code: room[1].toLowerCase(), voice: querVoz(room[2]) }
  const lobby = h.match(/^#\/lobby\/([A-Za-z-]+)(?:\?.*)?$/)
  if (lobby) return { kind: 'lobby', code: lobby[1].toLowerCase() }
  const telemovel = h.match(/^#\/telemovel\/([A-Za-z-]+)(?:\?.*)?$/)
  if (telemovel) return { kind: 'telemovel', code: telemovel[1].toLowerCase() }
  const share = h.match(/^#\/share\/([a-f0-9]+)(?:\?.*)?$/)
  if (share) return { kind: 'share', token: share[1] }
  const invite = h.match(/^#\/invite\/([A-Za-z0-9_-]+)(?:\?.*)?$/)
  if (invite) return { kind: 'invite', token: invite[1] }
  const diagram = h.match(/^#\/whiteboards\/diagram(?:\/([A-Za-z0-9_-]+))?(?:\?.*)?$/)
  if (diagram) return { kind: 'diagram', id: diagram[1] ?? null }
  const player = h.match(/^#\/recordings\/([0-9a-f-]{36})(?:\?.*)?$/)
  if (player) return { kind: 'player', id: player[1] }
  const pagina = paginaDe(h)
  if (pagina) return { kind: pagina }
  return { kind: 'desconhecida', endereco: h }
}
