/**
 * Da pesquisa global para as rotas da app. O servidor devolve IDS (`target`),
 * não rotas da UI (contrato §1): o mapeamento vive aqui, num só sítio, e
 * testa-se sem DOM.
 */
import type { SearchHit, SearchType } from '../api'

const enc = encodeURIComponent

/** Rota (sem `#`) para abrir um resultado; `null` se o tipo não tem ecrã. */
export function hitHash(hit: Pick<SearchHit, 'type' | 'id' | 'target'>): string | null {
  const tg = hit.target ?? {}
  const s = (k: string) => (tg[k] === null || tg[k] === undefined ? null : String(tg[k]))
  switch (hit.type) {
    case 'meetings':
      return `/calendar/m/${enc(s('meeting_id') ?? hit.id)}`
    case 'recordings': {
      const id = s('recording_id') ?? hit.id
      const at = typeof tg.at_secs === 'number' && tg.at_secs > 0 ? Math.floor(tg.at_secs) : null
      return `/recordings/${enc(id)}${at !== null ? `?t=${at}` : ''}`
    }
    case 'people':
      return `/directory?u=${enc(s('user_id') ?? hit.id)}`
    case 'whiteboards':
      return `/whiteboards?id=${enc(s('whiteboard_id') ?? hit.id)}`
    case 'rooms':
      return s('room_code') ? `/r/${enc(s('room_code')!)}` : null
    case 'messages':
      // A conversa vive na sala: abre-se a pré-entrada dela.
      return s('room_code') ? `/r/${enc(s('room_code')!)}` : null
    case 'stream_destinations':
      return '/studio'
    case 'webhooks':
      return '/integrations'
    case 'audit_events':
      return '/admin'
    default:
      return null
  }
}

/** «Ver todos em <ecrã>»: a lista desse recurso já com a pesquisa (`q`). */
export function moreHash(type: SearchType, q: string): string | null {
  const p = `q=${enc(q)}`
  switch (type) {
    case 'recordings':
      return `/recordings?${p}`
    case 'meetings':
      return `/calendar?vista=lista&${p}`
    case 'whiteboards':
      return `/whiteboards?${p}`
    case 'people':
      return `/directory?${p}`
    case 'audit_events':
      return `/admin?audit.${p}`
    default:
      return null
  }
}

const MAX_RECENT = 8
const key = (userId: string) => `dx_search_recent:${userId}`

export function readRecent(userId: string): string[] {
  try {
    const v = JSON.parse(localStorage.getItem(key(userId)) ?? '[]')
    return Array.isArray(v) ? v.filter((x) => typeof x === 'string').slice(0, MAX_RECENT) : []
  } catch {
    return []
  }
}

/** Põe no topo, sem repetidos (ignora maiúsculas), no máximo 8. */
export function withRecent(list: string[], q: string): string[] {
  const t = q.trim()
  if (!t) return list
  return [t, ...list.filter((x) => x.toLowerCase() !== t.toLowerCase())].slice(0, MAX_RECENT)
}

export function pushRecent(userId: string, q: string): string[] {
  const next = withRecent(readRecent(userId), q)
  try {
    localStorage.setItem(key(userId), JSON.stringify(next))
  } catch {
    /* sem armazenamento: fica só nesta sessão da paleta */
  }
  return next
}

export function clearRecent(userId: string) {
  try {
    localStorage.removeItem(key(userId))
  } catch {
    /* nada a limpar */
  }
}
