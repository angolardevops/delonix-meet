/**
 * As rotas do backoffice, lidas do endereço. Mesmo padrão de hash routing do
 * `web/src/rota.ts` (casar por SEGMENTO, nunca por prefixo — `#/adminzinho`
 * não pode abrir `#/admin`), mas muito mais pequeno: cinco secções fixas,
 * sem sala, sem convite, sem diagrama — nada que precise de token ou de
 * código normalizado.
 */

export type NavKey = 'overview' | 'tenants' | 'integrations' | 'security' | 'communications'

export const PAGES: NavKey[] = ['overview', 'tenants', 'integrations', 'security', 'communications']

export type Route = { kind: NavKey } | { kind: 'desconhecida'; endereco: string }

function paginaDe(h: string): NavKey | null {
  for (const p of PAGES) {
    if (h === `#/${p}` || h.startsWith(`#/${p}?`) || h.startsWith(`#/${p}/`)) return p
  }
  return null
}

export function parseHash(hash: string = location.hash): Route {
  const h = hash
  if (h === '' || h === '#' || h === '#/' || !h.startsWith('#/')) return { kind: 'overview' }
  const pagina = paginaDe(h)
  if (pagina) return { kind: pagina }
  return { kind: 'desconhecida', endereco: h }
}

/** Rótulo de cada secção, por chave de tradução — a mesma fonte que a barra lateral usa. */
export const NAV_I18N: Record<NavKey, string> = {
  overview: 'shell.nav.overview',
  tenants: 'shell.nav.tenants',
  integrations: 'shell.nav.integrations',
  security: 'shell.nav.security',
  communications: 'shell.nav.communications',
}
