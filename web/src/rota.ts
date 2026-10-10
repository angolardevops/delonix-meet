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
  /** O link do email de prova do endereço (`#/verificar-email?token=…`). */
  | { kind: 'verificar-email'; token: string }
  /** O link do email de reposição de password (`#/repor-password?token=…`). */
  | { kind: 'repor-password'; token: string }
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
  const verificar = h.match(/^#\/verificar-email\?token=([A-Za-z0-9_-]+)$/)
  if (verificar) return { kind: 'verificar-email', token: verificar[1] }
  const repor = h.match(/^#\/repor-password\?token=([A-Za-z0-9_-]+)$/)
  if (repor) return { kind: 'repor-password', token: repor[1] }
  const diagram = h.match(/^#\/whiteboards\/diagram(?:\/([A-Za-z0-9_-]+))?(?:\?.*)?$/)
  if (diagram) return { kind: 'diagram', id: diagram[1] ?? null }
  const player = h.match(/^#\/recordings\/([0-9a-f-]{36})(?:\?.*)?$/)
  if (player) return { kind: 'player', id: player[1] }
  const pagina = paginaDe(h)
  if (pagina) return { kind: pagina }
  return { kind: 'desconhecida', endereco: h }
}

// ---------------------------------------------------------------------------
//  O trilho (breadcrumbs), o título e a hierarquia
//
//  A hierarquia NÃO foi inventada aqui: estava escrita em dois sítios e
//  enterrada. O pai de cada rota vivia num ternário dentro de um atributo do
//  `App.tsx` (`player → recordings`, `diagram → whiteboards`) e os rótulos do
//  primeiro nível são os do rail (`Shell.tsx`). O que faltava era um nome para
//  isso e um sítio onde o pedir.
// ---------------------------------------------------------------------------

/**
 * O rótulo de cada destino, por chave de tradução. É a MESMA fonte que o rail
 * usa: o `Shell` importa este mapa e junta-lhe os ícones. Dois sítios a
 * escrever o nome de «Gravações» davam dois nomes.
 */
export const NAV_I18N: Record<NavKey, string> = {
  home: 'shell.nav.inicio',
  calendar: 'shell.nav.agenda',
  rooms: 'shell.nav.salas',
  studio: 'shell.nav.estudio',
  recordings: 'shell.nav.gravacoes',
  whiteboards: 'shell.nav.quadros',
  directory: 'shell.nav.contactos',
  integrations: 'shell.nav.integracoes',
  analytics: 'shell.nav.analise',
  admin: 'shell.nav.administracao',
  telecom: 'telecom.titulo',
  ai: 'consola.nav.ia',
}

/** Um degrau do trilho: para onde leva e como se chama. */
export type Degrau = { hash: string; chave: string }

/**
 * O destino do rail a que uma rota pertence — é o que fica aceso no menu.
 * `null` para o que não tem destino (a sala, a moderação de uma sala, as
 * páginas públicas).
 */
export function destinoNoRail(r: Route): NavKey | null {
  switch (r.kind) {
    case 'player':
      return 'recordings'
    case 'diagram':
      return 'whiteboards'
    case 'room':
    case 'telemovel':
    case 'share':
    case 'invite':
    case 'verificar-email':
    case 'repor-password':
    case 'lobby':
    case 'desconhecida':
      return null
    default:
      return r.kind
  }
}

/**
 * Os ANTECEDENTES da rota, do Início até ao pai — a página actual não entra,
 * porque o `<h1>` da barra já a diz.
 *
 * Vazio para o Início e para o que vive fora da consola: um trilho de um degrau
 * não é um trilho, é ruído.
 */
export function trilhoDe(r: Route): Degrau[] {
  const inicio: Degrau = { hash: '#/', chave: NAV_I18N.home }
  switch (r.kind) {
    case 'home':
    case 'room':
    case 'telemovel':
    case 'share':
    case 'invite':
    case 'verificar-email':
    case 'repor-password':
      return []
    case 'player':
      return [inicio, { hash: '#/recordings', chave: NAV_I18N.recordings }]
    case 'diagram':
      return [inicio, { hash: '#/whiteboards', chave: NAV_I18N.whiteboards }]
    case 'lobby':
      return [inicio, { hash: '#/rooms', chave: NAV_I18N.rooms }]
    case 'desconhecida':
      return [inicio]
    default:
      return [inicio]
  }
}

/**
 * A chave de tradução do nome da rota, para o `document.title` — que até
 * 2026-10-06 era o nome da aplicação em TODOS os ecrãs (duas escritas em todo o
 * `web/src`, as duas com o mesmo valor): quinze abas iguais, histórico do
 * browser indistinguível e um leitor de ecrã a dizer sempre o mesmo.
 *
 * `null` quando o nome não se sabe aqui — a sala e o leitor de gravação são
 * nomeados pelo que estão a mostrar, e isso é o ecrã que o sabe.
 */
export function chaveDoTitulo(r: Route): string | null {
  const destino = destinoNoRail(r)
  if (destino && destino !== 'home') return NAV_I18N[destino]
  switch (r.kind) {
    case 'lobby':
      return 'shell.nav.salas'
    case 'desconhecida':
      return 'ui.rotaDesconhecida.titulo'
    default:
      return null
  }
}
