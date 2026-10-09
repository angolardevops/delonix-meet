/**
 * Cliente de API do backoffice de operador (`web-admin/`).
 *
 * Mesmas rotas de sessão que `web/src/api.ts` já usa (`/api/auth/*`,
 * `credentials: 'same-origin'`, token de acesso em memória + `localStorage`)
 * porque o CORS do servidor NÃO tem `allow_credentials` (`server/src/lib.rs`
 * `build_cors`) — esta app só funciona no MESMO origin da API, tal como
 * `web/`. Ver a nota do PR3.
 *
 * As chaves de `localStorage` são PRÓPRIAS (`dxa_*`, não `dx_*`): mesmo
 * origin significa `localStorage` partilhado entre `web/` e `web-admin/`. Um
 * operador que também é utilizador normal no mesmo browser não pode ter uma
 * app a apagar a sessão da outra.
 */

export interface User {
  id: string
  email: string
  username: string
  locale?: string
}

/** Resposta de auth: o refresh token NÃO vem aqui — vive num cookie HttpOnly. */
export interface AuthOk {
  access_token: string
  user: User
}

let accessToken: string | null = localStorage.getItem('dxa_access')

export function currentUser(): User | null {
  const raw = localStorage.getItem('dxa_user')
  return raw ? JSON.parse(raw) : null
}

function saveSession(t: AuthOk) {
  accessToken = t.access_token
  localStorage.setItem('dxa_access', t.access_token)
  localStorage.setItem('dxa_user', JSON.stringify(t.user))
}

export function logout() {
  accessToken = null
  localStorage.removeItem('dxa_access')
  localStorage.removeItem('dxa_user')
  // Revoga o refresh e limpa o cookie no servidor (best-effort).
  void fetch('/api/auth/logout', { method: 'POST', credentials: 'same-origin' }).catch(() => {})
}

/** Erro que carrega o estado HTTP — ver `web/src/api.ts` (mesmo desenho). */
export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly body: unknown,
    message: string,
  ) {
    super(message)
    this.name = 'ApiError'
  }
}

/** `AbortController.abort()` não é uma falha da API — nunca vira estado de erro. */
export function isAbort(e: unknown): boolean {
  return (e as { name?: string } | null)?.name === 'AbortError'
}

/** Mensagem legível de um erro de API, com recurso ao texto dado. */
export function apiErrorMessage(e: unknown, fallback: string): string {
  if (e instanceof ApiError) {
    const b = e.body as { error?: string; message?: string } | string | null
    if (typeof b === 'string' && b) return b
    if (b && typeof b === 'object') return b.error ?? b.message ?? fallback
  }
  if (e instanceof Error && e.message) return e.message
  return fallback
}

async function request<T>(path: string, options: RequestInit = {}, retry = true): Promise<T> {
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    ...(options.headers as Record<string, string>),
  }
  if (accessToken) headers['Authorization'] = `Bearer ${accessToken}`
  const res = await fetch(path, { ...options, headers, credentials: 'same-origin' })
  // 401 + temos utilizador em sessão → tenta renovar via cookie de refresh.
  if (res.status === 401 && retry && localStorage.getItem('dxa_user')) {
    await refreshSession()
    return request<T>(path, options, false)
  }
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: res.statusText }))
    throw new ApiError(res.status, body, body?.error ?? res.statusText ?? 'request failed')
  }
  if (res.status === 204) return undefined as T
  return res.json()
}

/** Renovação em curso, se houver — a mesma guarda de reentrada do `web/src/api.ts` (R98). */
let renovacaoEmCurso: Promise<void> | null = null

async function refreshSession(): Promise<void> {
  if (renovacaoEmCurso) return renovacaoEmCurso
  renovacaoEmCurso = renovarUmaVez().finally(() => {
    renovacaoEmCurso = null
  })
  return renovacaoEmCurso
}

async function renovarUmaVez() {
  const res = await fetch('/api/auth/refresh', { method: 'POST', credentials: 'same-origin' })
  if (!res.ok && res.status !== 401 && res.status !== 404) {
    throw new ApiError(res.status, null, 'refresh indisponível')
  }
  if (!res.ok) {
    logout()
    window.dispatchEvent(new Event('dxa-auth-expired'))
    throw new Error('session expired')
  }
  saveSession(await res.json())
}

/**
 * Entrar. Simplificado de propósito (sem SSO, sem segundo factor): este é o
 * backoffice de um punhado de operadores, não a porta de entrada de toda a
 * gente. Uma conta com MFA activo chega aqui com `mfa_required` e falha com
 * uma mensagem clara — suportar o desafio fica para quando houver um
 * operador que precise dele.
 */
export async function login(email: string, password: string): Promise<User> {
  const t = await request<AuthOk & { mfa_required?: boolean }>('/api/auth/login', {
    method: 'POST',
    body: JSON.stringify({ email, password }),
  })
  if (t.mfa_required) {
    throw new Error('Esta conta tem segundo factor activo — o backoffice ainda não o suporta.')
  }
  saveSession(t)
  return t.user
}

/** Busca autenticada de um recurso binário → object URL (para download). */
export async function authedBlobUrl(path: string): Promise<string> {
  const res = await fetch(path, {
    headers: accessToken ? { Authorization: `Bearer ${accessToken}` } : {},
  })
  if (!res.ok) throw new Error(`blob ${res.status}`)
  return URL.createObjectURL(await res.blob())
}

export function accessTokenValue(): string | null {
  return accessToken
}

// ---------------------------------------------------------------------------
//  Operador — `/api/operator/v1/*`. Formas confirmadas em
//  `docs/reference/openapi/operator.json` (gerado pelo servidor): não se
//  adivinha campo nem tipo daqui.
// ---------------------------------------------------------------------------

export interface MediaNode {
  node_id: string
  hostname: string
  version: string
  edition: string
  started_at: string
  last_seen_at: string
  /** `serving` | `draining` | `unreachable`. */
  status: string
  rooms: number
  peers: number
  ws_connections: number
  live_broadcasts: number
  /** Ocupação 0–1 face à capacidade declarada; ausente sem capacidade. */
  load: number | null
  peer_capacity: number | null
  accepting_new_rooms: boolean
}

export interface MediaNodeList {
  items: MediaNode[]
  serving: number
  draining: number
  unreachable: number
  /** Participantes em nós que respondem. */
  peers: number
}

export const listNodes = (signal?: AbortSignal) => request<MediaNodeList>('/api/operator/v1/nodes', { signal })

/** Uma organização vista pelo operador: identidade + os seis tectos de plano. */
export interface OperatorOrgSummary {
  id: string
  name: string
  slug: string
  domain: string
  created_at: string
  member_count: number
  max_groups: number | null
  max_rooms: number | null
  max_meetings: number | null
  max_storage_bytes: number | null
  max_seats: number | null
  max_concurrent_participants: number | null
}

export interface OperatorOrgPage {
  items: OperatorOrgSummary[]
  next_page_token: string | null
}

export const listTenants = (pageToken?: string | null, signal?: AbortSignal) => {
  const q = new URLSearchParams()
  if (pageToken) q.set('page_token', pageToken)
  const qs = q.toString()
  return request<OperatorOrgPage>(`/api/operator/v1/tenants${qs ? `?${qs}` : ''}`, { signal })
}

export interface UsageBucket {
  bytes: number
  count: number
}

export interface OrgStorageUsage {
  org_id: string
  recordings: UsageBucket
  whiteboards: UsageBucket
  /** `recordings.bytes + whiteboards.bytes`. */
  used_bytes: number
  /** Tecto em bytes; `null` = ilimitado. */
  max_storage_bytes: number | null
  /** Quanto ainda cabe (nunca negativo); `null` = ilimitado. */
  remaining_bytes: number | null
}

export interface SeatSummary {
  used: number
  active_this_month: number
  inactive: number
  inactive_days: number
  owner_missing: boolean
  /** Tecto do operador; `null` = sem tecto. */
  limit: number | null
  available: number | null
}

export interface OperatorOrgDetail {
  org: OperatorOrgSummary
  seats: SeatSummary
  storage: OrgStorageUsage
}

export const getTenant = (orgId: string, signal?: AbortSignal) =>
  request<OperatorOrgDetail>(`/api/operator/v1/tenants/${orgId}`, { signal })

export interface OperatorQuotasReq {
  /** `null`/negativo = ilimitado. */
  max_groups?: number | null
  max_rooms?: number | null
  max_meetings?: number | null
  /** `'freeswitch' | 'provider'`. Omisso ou fora do enum mantém o actual. */
  voice_media_backend?: string | null
  /** `'shared' | 'dedicated'`. Omisso ou fora do enum mantém o actual. */
  voice_did_model?: string | null
}

export const saveTenantQuotas = (orgId: string, body: OperatorQuotasReq) =>
  request<OperatorOrgSummary>(`/api/operator/v1/tenants/${orgId}/quotas`, {
    method: 'PUT',
    body: JSON.stringify(body),
  })

export const saveTenantSeats = (orgId: string, maxSeats: number | null) =>
  request<SeatSummary>(`/api/operator/v1/organizations/${orgId}/seats`, {
    method: 'PUT',
    body: JSON.stringify({ max_seats: maxSeats }),
  })

export interface ConcurrencyLimit {
  max_concurrent_participants: number | null
}

export const saveTenantConcurrency = (orgId: string, maxConcurrentParticipants: number | null) =>
  request<ConcurrencyLimit>(`/api/operator/v1/organizations/${orgId}/concurrency`, {
    method: 'PUT',
    body: JSON.stringify({ max_concurrent_participants: maxConcurrentParticipants }),
  })

export interface ObjectStoreView {
  endpoint: string
  bucket: string
  region: string
  used_for_recordings: boolean
}

export interface StorageConfig {
  storage_type: 'local' | 'nfs' | 'webdav'
  nfs_server: string | null
  nfs_path: string | null
  webdav_url: string | null
  webdav_user: string | null
  webdav_password_set: boolean
  webdav_path: string
  object_store?: ObjectStoreView | null
}

export interface StorageConfigSaveReq {
  storage_type: string
  nfs_server?: string
  nfs_path?: string
  webdav_url?: string
  webdav_user?: string
  webdav_password?: string
  webdav_path?: string
}

export const getPlatformStorage = () => request<StorageConfig>('/api/operator/v1/storage')

export const savePlatformStorage = (cfg: StorageConfigSaveReq) =>
  request<StorageConfig>('/api/operator/v1/storage', { method: 'PUT', body: JSON.stringify(cfg) })

export const testPlatformStorage = () =>
  request<{ ok: boolean; type: string; message: string }>('/api/operator/v1/storage/test', { method: 'POST' })

export interface OperatorLoginSettings {
  hide_org_creation: boolean
  hide_sso_button: boolean
}

export const getLoginSettings = () => request<OperatorLoginSettings>('/api/operator/v1/login-settings')

export const saveLoginSettings = (body: OperatorLoginSettings) =>
  request<OperatorLoginSettings>('/api/operator/v1/login-settings', { method: 'PUT', body: JSON.stringify(body) })
