export interface User {
  id: string
  email: string
  username: string
  locale?: string
}

export interface Room {
  id: string
  code: string
  name: string
  owner_id: string
  topology: string
  waiting_room: boolean
  e2ee: boolean
  /** 'normal' (por defeito) ou 'training' — só treino tem salas de grupo. */
  format?: string
}

/** Resposta de auth: o refresh token NÃO vem aqui — vive num cookie HttpOnly. */
export interface AuthOk {
  access_token: string
  user: User
}

let accessToken: string | null = localStorage.getItem('dx_access')

export function currentUser(): User | null {
  const raw = localStorage.getItem('dx_user')
  return raw ? JSON.parse(raw) : null
}

function saveSession(t: AuthOk) {
  accessToken = t.access_token
  localStorage.setItem('dx_access', t.access_token)
  localStorage.setItem('dx_user', JSON.stringify(t.user))
}

export function logout() {
  accessToken = null
  localStorage.removeItem('dx_access')
  localStorage.removeItem('dx_user')
  // Revoga o refresh e limpa o cookie no servidor (best-effort).
  void fetch('/api/auth/logout', { method: 'POST', credentials: 'same-origin' }).catch(() => {})
}

async function request<T>(path: string, options: RequestInit = {}, retry = true): Promise<T> {
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    ...(options.headers as Record<string, string>),
  }
  if (accessToken) headers['Authorization'] = `Bearer ${accessToken}`
  const res = await fetch(path, { ...options, headers, credentials: 'same-origin' })
  // 401 + temos utilizador em sessão → tenta renovar via cookie de refresh.
  if (res.status === 401 && retry && localStorage.getItem('dx_user')) {
    await refreshSession()
    return request<T>(path, options, false)
  }
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: res.statusText }))
    throw new Error(body.error ?? 'request failed')
  }
  return res.json()
}

async function refreshSession() {
  // Sem corpo: o refresh token vai no cookie HttpOnly (enviado automaticamente).
  const res = await fetch('/api/auth/refresh', { method: 'POST', credentials: 'same-origin' })
  if (!res.ok) {
    logout()
    // Sessão expirada/inválida (ex.: sessão antiga sem cookie de refresh):
    // avisa a app para mostrar o login limpo, em vez de "unauthorized" numa
    // página meia-carregada.
    window.dispatchEvent(new Event('dx-auth-expired'))
    throw new Error('session expired')
  }
  saveSession(await res.json())
}

/** Registo = criar organização (org-first). O admin fica com o domínio do email. */
export async function registerOrg(orgName: string, email: string, password: string): Promise<User> {
  const t = await request<AuthOk>('/api/auth/register', {
    method: 'POST',
    body: JSON.stringify({ org_name: orgName, email, password }),
  })
  saveSession(t)
  return t.user
}

export async function login(email: string, password: string): Promise<User> {
  const t = await request<AuthOk>('/api/auth/login', {
    method: 'POST',
    body: JSON.stringify({ email, password }),
  })
  saveSession(t)
  return t.user
}

/** Verifica se o domínio de email tem SSO configurado. */
export interface SsoCheckResult {
  sso_enabled: boolean
  enforce_sso: boolean
}
export async function ssoCheck(domain: string): Promise<SsoCheckResult> {
  const res = await fetch(`/api/auth/sso/check?domain=${encodeURIComponent(domain)}`)
  if (!res.ok) return { sso_enabled: false, enforce_sso: false }
  return res.json()
}

/** Redireciona o browser para o IdP OIDC da organização. */
export function ssoRedirect(domain: string) {
  window.location.href = `/api/auth/sso/login?domain=${encodeURIComponent(domain)}`
}

/**
 * Chamado pela rota `#/sso-complete` após o callback do IdP.
 * O access token vem no hash fragment (seguro — não aparece nos logs do servidor).
 * O refresh cookie já foi definido pelo servidor no redirect.
 */
export async function completeSsoLogin(): Promise<User | null> {
  const hash = window.location.hash
  const match = hash.match(/token=([^&]+)/)
  if (!match) return null
  const token = match[1]
  accessToken = token
  localStorage.setItem('dx_access', token)
  // Buscar os dados do utilizador com o token fresco.
  try {
    const user = await request<User>('/api/users/me')
    localStorage.setItem('dx_user', JSON.stringify(user))
    // Limpar o hash para não expor o token na URL.
    window.location.hash = '#/'
    return user
  } catch {
    return null
  }
}

export const createRoom = (
  name: string,
  topology: 'sfu' | 'mesh' = 'sfu',
  waitingRoom = false,
  e2ee = false,
  format: 'normal' | 'training' = 'normal',
) =>
  request<Room>('/api/rooms', {
    method: 'POST',
    body: JSON.stringify({ name, topology, waiting_room: waitingRoom, e2ee, format }),
  })

export const getRoom = (code: string) => request<Room>(`/api/rooms/${code}`)

export const joinRoom = (code: string) =>
  request<{ room: Room; room_token: string; scheduled?: boolean }>(`/api/rooms/${code}/join`, { method: 'POST' })

export const iceServers = () => request<RTCConfiguration>('/api/ice')

export interface ChatHistoryMsg {
  id: string
  user_id: string
  username: string
  message: string
  created_at: string
}

export const roomChatHistory = (code: string) =>
  request<ChatHistoryMsg[]>(`/api/rooms/${code}/chat`)

export interface Recording {
  id: string
  room_id: string
  uploader_id: string
  filename: string
  size_bytes: number
  created_at: string
}

/** Item da biblioteca, com metadados extra. */
export interface RecordingItem extends Recording {
  room_code: string
  uploader_name: string
  owned: boolean
  share_count: number
  /** RBAC: só dono + admins da org podem descarregar (os restantes só reproduzem). */
  can_download: boolean
}

export type RecurrenceFreq = 'daily' | 'weekly' | 'monthly' | 'yearly'

export interface Meeting {
  id: string
  owner_id: string
  owner_name: string
  title: string
  description: string
  kind: 'video' | 'voice'
  starts_at: string
  duration_min: number
  room_code: string | null
  is_owner: boolean
  minutes?: string
  room_ref?: string | null
  room_name?: string | null
  my_status?: 'owner' | 'pending' | 'accepted' | 'declined'
  recurrence_freq?: RecurrenceFreq | null
  recurrence_interval?: number
  recurrence_parent_id?: string | null
}

export interface MeetingRoom {
  id: string
  org_id: string
  name: string
  location: string
  capacity: number
}

export interface ParticipantConflict {
  user_id: string
  username: string
  meeting_id: string
  meeting_title: string
  starts_at: string
}
export interface RoomConflict {
  meeting_id: string
  meeting_title: string
  starts_at: string
}
export interface Conflicts {
  participants: ParticipantConflict[]
  room: RoomConflict[]
}

export interface InviteeResponse {
  user_id: string
  username: string
  status: 'pending' | 'accepted' | 'declined'
  decline_reason: string
  responded_at: string | null
}

export interface QuarantineRow {
  user_id: string
  username: string
  count: number
}

export const listRecordings = (code: string) => request<Recording[]>(`/api/rooms/${code}/recordings`)

export const recordingsLibrary = () => request<RecordingItem[]>('/api/recordings')

export const searchUsers = (q: string) =>
  request<User[]>(`/api/users/search?q=${encodeURIComponent(q)}`)

/** Atualiza os próprios dados (username, password e/ou locale) e sincroniza o cache local. */
export async function updateMe(data: { username?: string; password?: string; locale?: string }): Promise<User> {
  const user = await request<User>('/api/users/me', { method: 'PATCH', body: JSON.stringify(data) })
  localStorage.setItem('dx_user', JSON.stringify(user))
  return user
}

export const updateEmployee = (orgId: string, userId: string, data: { role?: string; title?: string; branch_id?: string | null }) =>
  request<Employee>(`/api/orgs/${orgId}/employees/${userId}`, { method: 'PATCH', body: JSON.stringify(data) })

export const shareRecording = (id: string, userId: string) =>
  request(`/api/recordings/${id}/share`, { method: 'POST', body: JSON.stringify({ user_id: userId }) })

export const listRecordingShares = (id: string) => request<User[]>(`/api/recordings/${id}/share`)

export const unshareRecording = (id: string, userId: string) =>
  request(`/api/recordings/${id}/share/${userId}`, { method: 'DELETE' })

export interface ShareLink {
  id: string
  recording_id: string
  token: string
  expires_at: string | null
  created_at: string
}

export const getRecordingLink = (id: string) =>
  request<ShareLink | null>(`/api/recordings/${id}/link`)

export const createRecordingLink = (id: string, opts: { password?: string; expires_at?: string | null }) =>
  request<ShareLink>(`/api/recordings/${id}/link`, {
    method: 'POST',
    body: JSON.stringify(opts),
  })

export const revokeRecordingLink = (id: string) =>
  request(`/api/recordings/${id}/link`, { method: 'DELETE' })

export interface PublicShareInfo {
  recording_id: string
  filename: string
  size_bytes: number
  created_at: string
  download_url: string
  has_password: boolean
}

export async function getPublicShare(token: string, password?: string): Promise<PublicShareInfo> {
  const url = `/api/share/${token}${password ? `?password=${encodeURIComponent(password)}` : ''}`
  const res = await fetch(url, { credentials: 'same-origin' })
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: res.statusText }))
    throw Object.assign(new Error(body.error ?? 'request failed'), { status: res.status })
  }
  return res.json()
}

export const inviteToRoom = (code: string, targets: string[], kind: 'video' | 'voice' = 'video') =>
  request<{ ringing: string[]; offline: string[] }>(`/api/rooms/${code}/invite`, {
    method: 'POST',
    body: JSON.stringify({ targets, kind }),
  })

export const listMeetings = () => request<Meeting[]>('/api/meetings')

export const createMeeting = (m: {
  title: string
  description?: string
  kind: 'video' | 'voice'
  starts_at: string
  duration_min: number
  invitee_ids: string[]
  room_ref?: string | null
  recurrence_freq?: RecurrenceFreq | null
  recurrence_interval?: number
  recurrence_until?: string | null
  recurrence_count?: number | null
  recurrence_byday?: string | null
}) => request<Meeting & { conflicts: Conflicts }>('/api/meetings', { method: 'POST', body: JSON.stringify(m) })

export const checkConflicts = (body: {
  starts_at: string
  duration_min: number
  invitee_ids: string[]
  room_ref?: string | null
}) => request<Conflicts>('/api/meetings/conflicts', { method: 'POST', body: JSON.stringify(body) })

export const deleteMeeting = (id: string) => request(`/api/meetings/${id}`, { method: 'DELETE' })

export const startMeeting = (id: string) =>
  request<{ code: string; kind: 'video' | 'voice' }>(`/api/meetings/${id}/start`, { method: 'POST' })

export const respondMeeting = (id: string, status: 'accepted' | 'declined', reason = '') =>
  request(`/api/meetings/${id}/respond`, { method: 'POST', body: JSON.stringify({ status, reason }) })

export const meetingInvitees = (id: string) => request<InviteeResponse[]>(`/api/meetings/${id}/invitees`)

export const quarantineAnalytics = (period: 'week' | 'month' | 'quarter' | 'year', orgId?: string) =>
  request<QuarantineRow[]>(`/api/quarantine/analytics?period=${period}${orgId ? `&org_id=${orgId}` : ''}`)

export const listMeetingRooms = (orgId: string) => request<MeetingRoom[]>(`/api/orgs/${orgId}/meeting-rooms`)
export const createMeetingRoom = (orgId: string, name: string, location: string, capacity: number) =>
  request<MeetingRoom>(`/api/orgs/${orgId}/meeting-rooms`, {
    method: 'POST',
    body: JSON.stringify({ name, location, capacity }),
  })

export const saveMinutesByRoom = (code: string, minutes: string, transcript: string) =>
  request(`/api/rooms/${code}/minutes`, { method: 'POST', body: JSON.stringify({ minutes, transcript }) })

// ---------- Enterprise ----------

export interface OrgSummary {
  id: string
  name: string
  slug: string
  role: 'admin' | 'member'
  member_count: number
  domain?: string
  retention_days?: number
  max_groups?: number | null
  max_rooms?: number | null
  max_meetings?: number | null
}

export interface OrgQuotas {
  max_groups: number | null
  max_rooms: number | null
  max_meetings: number | null
}

export interface WhiteboardMeta {
  id: string
  title: string
  room_code: string
  is_public: boolean
  share_token: string
  created_at: string
}

export const listWhiteboards = () => request<WhiteboardMeta[]>('/api/whiteboards')
export const saveWhiteboard = (title: string, roomCode: string, pngBase64: string) =>
  request<WhiteboardMeta>('/api/whiteboards', {
    method: 'POST',
    body: JSON.stringify({ title, room_code: roomCode, png_base64: pngBase64 }),
  })
export const deleteWhiteboard = (id: string) => request(`/api/whiteboards/${id}`, { method: 'DELETE' })
export const shareWhiteboard = (id: string, isPublic: boolean) =>
  request<WhiteboardMeta>(`/api/whiteboards/${id}/share`, {
    method: 'POST',
    body: JSON.stringify({ public: isPublic }),
  })
export const whiteboardPngUrl = (id: string) => `/api/whiteboards/${id}/png`

export interface Webhook {
  id: string
  org_id: string
  kind: 'slack' | 'teams' | 'mattermost' | 'generic'
  url: string
  events: string
  active: boolean
}

export const updateOrgSettings = (
  orgId: string,
  domain: string,
  retentionDays: number,
  quotas?: Partial<OrgQuotas>,
) =>
  request(`/api/orgs/${orgId}/settings`, {
    method: 'POST',
    body: JSON.stringify({ domain, retention_days: retentionDays, ...quotas }),
  })

/** Busca autenticada de um recurso binário → object URL (para <img>). */
export async function authedBlobUrl(path: string): Promise<string> {
  const res = await fetch(path, {
    headers: accessToken ? { Authorization: `Bearer ${accessToken}` } : {},
  })
  if (!res.ok) throw new Error(`blob ${res.status}`)
  return URL.createObjectURL(await res.blob())
}

export interface ApiKeyInfo {
  id: string
  name: string
  prefix: string
  created_at: string
  last_used_at: string | null
}
export const listApiKeys = (orgId: string) => request<ApiKeyInfo[]>(`/api/orgs/${orgId}/api-keys`)
export const createApiKey = (orgId: string, name: string) =>
  request<{ id: string; name: string; prefix: string; key: string }>(`/api/orgs/${orgId}/api-keys`, {
    method: 'POST',
    body: JSON.stringify({ name }),
  })
export const revokeApiKey = (orgId: string, keyId: string) =>
  request(`/api/orgs/${orgId}/api-keys/${keyId}`, { method: 'DELETE' })

export const listWebhooks = (orgId: string) => request<Webhook[]>(`/api/orgs/${orgId}/webhooks`)
export const createWebhook = (
  orgId: string,
  body: { kind: string; url: string; secret?: string; events?: string },
) => request<Webhook>(`/api/orgs/${orgId}/webhooks`, { method: 'POST', body: JSON.stringify(body) })
export const deleteWebhook = (orgId: string, hookId: string) =>
  request(`/api/orgs/${orgId}/webhooks/${hookId}`, { method: 'DELETE' })
export interface Branch {
  id: string
  org_id: string
  name: string
  location: string
}
export interface Employee {
  user_id: string
  username: string
  email: string
  role: 'admin' | 'member'
  title: string
  branch_id: string | null
  branch_name: string | null
  last_active?: string | null
}
export interface Group {
  id: string
  org_id: string
  name: string
  member_count: number
}

export interface WeekBucket {
  week_start: string
  count: number
  minutes: number
}
export interface OrgStats {
  meetings_30d: number
  meeting_minutes_30d: number
  active_users_30d: number
  members_total: number
  recordings_total: number
  recordings_bytes: number
  video_30d: number
  voice_30d: number
  avg_duration_min: number
  top_organizers: { username: string; count: number }[]
  meetings_per_week: WeekBucket[]
  quality_samples_30d: number
  avg_rtt_ms: number | null
  avg_loss_pct: number
  pct_good: number
  pct_poor: number
  meetings_prev_30d: number
  meeting_minutes_prev_30d: number
  active_users_prev_30d: number
}

/** Amostra de qualidade de chamada (QoS) reportada durante a reunião. */
export const postQos = (code: string, s: { rtt_ms: number | null; loss_pct: number; up_kbps: number }) =>
  request(`/api/rooms/${code}/qos`, { method: 'POST', body: JSON.stringify(s) })

/** Tradução de uma linha de legenda via LLM local (Ollama in-cluster). */
export const translateCaption = (text: string, target: string) =>
  request<{ text: string }>('/api/translate', { method: 'POST', body: JSON.stringify({ text, target }) })

export const myOrgs = () => request<OrgSummary[]>('/api/orgs')
export const orgStats = (orgId: string) => request<OrgStats>(`/api/orgs/${orgId}/stats`)

export interface AuditEntry {
  id: number
  actor: string
  action: string
  target: string
  created_at: string
}
/** Registos de auditoria da organização (só admins). */
export const listAudit = (orgId: string, limit = 100) =>
  request<AuditEntry[]>(`/api/orgs/${orgId}/audit?limit=${limit}`)
export const createOrg = (name: string) =>
  request<OrgSummary>('/api/orgs', { method: 'POST', body: JSON.stringify({ name }) })
export const listBranches = (orgId: string) => request<Branch[]>(`/api/orgs/${orgId}/branches`)
export const createBranch = (orgId: string, name: string, location: string) =>
  request<Branch>(`/api/orgs/${orgId}/branches`, { method: 'POST', body: JSON.stringify({ name, location }) })
export const listEmployees = (orgId: string) => request<Employee[]>(`/api/orgs/${orgId}/employees`)
export const addEmployee = (
  orgId: string,
  body: { email: string; username?: string; password?: string; title?: string; role?: string; branch_id?: string },
) => request<Employee>(`/api/orgs/${orgId}/employees`, { method: 'POST', body: JSON.stringify(body) })
export const removeEmployee = (orgId: string, userId: string) =>
  request(`/api/orgs/${orgId}/employees/${userId}`, { method: 'DELETE' })
export const listGroups = (orgId: string) => request<Group[]>(`/api/orgs/${orgId}/groups`)
export const createGroup = (orgId: string, name: string, memberIds: string[]) =>
  request<Group>(`/api/orgs/${orgId}/groups`, { method: 'POST', body: JSON.stringify({ name, member_ids: memberIds }) })

// ---------- Integração Odoo (nk_delonix_meet) ----------

export interface OdooConfig {
  org_id: string
  odoo_enabled: boolean
  odoo_url: string | null
  odoo_db: string | null
  odoo_token_prefix: string | null
  odoo_admin_id: string | null
  odoo_synced_at: string | null
  hide_org_creation: boolean
  hide_sso_button: boolean
}

export interface OdooConfigSaveReq {
  odoo_enabled: boolean
  odoo_url: string | null
  odoo_db: string | null
  hide_org_creation: boolean
  hide_sso_button: boolean
}

export const getOdooConfig = (orgId: string) =>
  request<OdooConfig>(`/api/orgs/${orgId}/integration/odoo`)

export const saveOdooConfig = (orgId: string, cfg: OdooConfigSaveReq) =>
  request<{ ok: boolean }>(`/api/orgs/${orgId}/integration/odoo`, {
    method: 'PUT',
    body: JSON.stringify(cfg),
  })

export const rotateOdooToken = (orgId: string) =>
  request<{ token: string; prefix: string }>(`/api/orgs/${orgId}/integration/odoo/token`, {
    method: 'POST',
  })

/** Configurações públicas da plataforma (sem autenticação). */
export interface PlatformSettings {
  hide_org_creation: boolean
  hide_sso_button: boolean
}
export const getPlatformSettings = () =>
  fetch('/api/public/settings').then((r) => r.json() as Promise<PlatformSettings>)

export function accessTokenValue(): string | null {
  return accessToken
}

/**
 * Tenta renovar a sessão via refresh cookie. Devolve `true` em caso de
 * sucesso, `false` se a sessão expirou (utilizador deve fazer login de novo).
 * Usado pelo cliente de presença antes de cada reconexão WebSocket —
 * o token de acesso tem 15 min de TTL e o WS não passa por `request()`.
 */
export async function tryRefreshToken(): Promise<boolean> {
  try {
    await refreshSession()
    return true
  } catch {
    return false
  }
}

export const ackMissedCalls = () => request('/api/missed-calls/ack', { method: 'POST' })

export async function uploadRecording(code: string, blob: Blob, name: string): Promise<Recording> {
  const res = await fetch(`/api/rooms/${code}/recordings?name=${encodeURIComponent(name)}`, {
    method: 'POST',
    headers: authHeader(),
    body: blob,
  })
  if (!res.ok) throw new Error('upload failed')
  return res.json()
}

export interface RoomNotes {
  title: string
  minutes: string
  transcript: string
}
export const roomNotes = (code: string) => request<RoomNotes>(`/api/rooms/${code}/notes`)

/** URL de objeto para reproduzir a gravação inline (o <video> não envia Bearer). */
export async function recordingObjectUrl(rec: Recording): Promise<string> {
  const res = await fetch(`/api/recordings/${rec.id}`, { headers: authHeader() })
  if (!res.ok) throw new Error('failed to load recording')
  return URL.createObjectURL(await res.blob())
}

export async function downloadMeetingIcs(id: string, title: string): Promise<void> {
  const res = await fetch(`/api/meetings/${id}/ics`, { headers: authHeader() })
  if (!res.ok) throw new Error('ics failed')
  const url = URL.createObjectURL(await res.blob())
  const el = document.createElement('a')
  el.href = url
  el.download = `${title.replace(/[^\w\- ]+/g, '')}.ics`
  el.click()
  URL.revokeObjectURL(url)
}

export async function downloadRecording(rec: Recording): Promise<void> {
  // ?dl=1 → o servidor exige a permissão de download (RBAC: dono + admin da org).
  const res = await fetch(`/api/recordings/${rec.id}?dl=1`, { headers: authHeader() })
  if (res.status === 401 || res.status === 403) throw new Error('Sem permissão para descarregar')
  if (!res.ok) throw new Error('download failed')
  const url = URL.createObjectURL(await res.blob())
  const a = document.createElement('a')
  a.href = url
  a.download = rec.filename
  a.click()
  URL.revokeObjectURL(url)
}

// ---------- Agenda de reunião ----------

export interface AgendaItem {
  id: string
  meeting_id: string
  position: number
  topic: string
  description: string
  duration_min: number
  done: boolean
  done_at: string | null
  done_by_id: string | null
  created_at: string
}

export async function listAgenda(meetingId: string): Promise<AgendaItem[]> {
  return request<AgendaItem[]>(`/api/meetings/${meetingId}/agenda`)
}

export async function addAgendaItem(
  meetingId: string,
  item: { topic: string; description?: string; duration_min?: number },
): Promise<AgendaItem> {
  return request<AgendaItem>(`/api/meetings/${meetingId}/agenda`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(item),
  })
}

export async function patchAgendaItem(
  meetingId: string,
  itemId: string,
  patch: { topic?: string; description?: string; duration_min?: number; done?: boolean; position?: number },
): Promise<AgendaItem> {
  return request<AgendaItem>(`/api/meetings/${meetingId}/agenda/${itemId}`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(patch),
  })
}

export async function deleteAgendaItem(meetingId: string, itemId: string): Promise<void> {
  await request(`/api/meetings/${meetingId}/agenda/${itemId}`, { method: 'DELETE' })
}

// ---------- Plano de Ação 5W2H ----------

export interface ActionItem {
  id: string
  plan_id: string
  position: number
  what: string
  when_date: string | null
  where_text: string
  who_id: string | null
  who_name: string
  why: string
  how: string
  resources: string
  /** 'todo' = A SER FEITO | 'doing' = EM ANDAMENTO | 'done' = REALIZADO */
  status: 'todo' | 'doing' | 'done'
  created_at: string
  updated_at: string
}

export interface ActionPlan {
  id: string
  meeting_id: string
  goal: string
  items: ActionItem[]
  created_at: string
}

export async function getActionPlan(meetingId: string): Promise<ActionPlan | null> {
  return request<ActionPlan | null>(`/api/meetings/${meetingId}/action-plan`)
}

export async function upsertActionPlan(meetingId: string, goal: string): Promise<ActionPlan> {
  return request<ActionPlan>(`/api/meetings/${meetingId}/action-plan`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ goal }),
  })
}

export async function addActionItem(
  meetingId: string,
  item: Partial<Omit<ActionItem, 'id' | 'plan_id' | 'created_at' | 'updated_at'>>,
): Promise<ActionItem> {
  return request<ActionItem>(`/api/meetings/${meetingId}/action-plan/items`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(item),
  })
}

export async function patchActionItem(
  itemId: string,
  patch: Partial<Omit<ActionItem, 'id' | 'plan_id' | 'created_at' | 'updated_at'>>,
): Promise<ActionItem> {
  return request<ActionItem>(`/api/action-items/${itemId}`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(patch),
  })
}

export async function deleteActionItem(itemId: string): Promise<void> {
  await request(`/api/action-items/${itemId}`, { method: 'DELETE' })
}

// ---------- SSO Config (admin) ----------

export interface SsoConfig {
  org_id: string
  issuer_url: string
  client_id: string
  enforce_sso: boolean
}

export async function getSsoConfig(orgId: string): Promise<SsoConfig | null> {
  const res = await request<SsoConfig | null>(`/api/orgs/${orgId}/sso`)
  return res
}

export async function saveSsoConfig(
  orgId: string,
  cfg: { issuer_url: string; client_id: string; client_secret: string; enforce_sso: boolean },
): Promise<void> {
  await request(`/api/orgs/${orgId}/sso`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(cfg),
  })
}

export async function deleteSsoConfig(orgId: string): Promise<void> {
  await request(`/api/orgs/${orgId}/sso`, { method: 'DELETE' })
}

function authHeader(): Record<string, string> {
  return accessToken ? { Authorization: `Bearer ${accessToken}` } : {}
}
