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

/**
 * Erro que CARREGA o estado HTTP. O `request` atirava um `Error` nu, o que
 * obrigava quem apanha a adivinhar pela mensagem — e é dessa adivinha que
 * nascem os bugs do `isAuthFailure` abaixo.
 *
 * Mesmo desenho do `delonix-portal` (src/api/client.ts), de propósito: as duas
 * consolas partilham as armadilhas, e vale a pena partilharem as guardas.
 */
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

/**
 * `AbortController.abort()` — mudar de página, desmontar, ou o duplo-efeito do
 * StrictMode em dev — rejeita a promessa do fetch. Isso NÃO é uma falha da API:
 * não pode virar estado de erro na UI, e muito menos logout. Guardar sempre nos
 * `.catch()` de pedidos que levam um `AbortSignal`.
 *
 * No portal isto faltava em ONZE sítios e o sintoma era a consola a saltar para
 * o login sozinha em desenvolvimento.
 */
export function isAbort(e: unknown): boolean {
  return (e as { name?: string } | null)?.name === 'AbortError'
}

/**
 * `true` só quando o servidor RESPONDEU a dizer que a sessão não serve.
 *
 * Separa duas coisas que o `refreshSession` tratava como uma: «não estás
 * autenticado» e «não consegui falar com o servidor». Um gateway a devolver 502,
 * ou um `fetch` que rejeita por rede, não são sessão inválida — e mandar essa
 * pessoa para o login é responder à pergunta errada: ela ESTÁ autenticada, perde
 * o sítio onde estava, e voltar a autenticar-se não resolve nada porque o
 * problema é o transporte.
 */
export function isAuthFailure(e: unknown): boolean {
  return e instanceof ApiError && (e.status === 401 || e.status === 403)
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
  if (res.status === 401 && retry && localStorage.getItem('dx_user')) {
    await refreshSession()
    return request<T>(path, options, false)
  }
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: res.statusText }))
    throw new ApiError(res.status, body, body?.error ?? res.statusText ?? 'request failed')
  }
  // 204 não tem corpo: `res.json()` rejeitava com SyntaxError e um DELETE bem
  // sucedido chegava a quem chama como falha.
  if (res.status === 204) return undefined as T
  return res.json()
}

/**
 * Renovação em curso, se houver (R98).
 *
 * O servidor **rota** o refresh token: usá-lo revoga-o e emite um par novo.
 * Duas renovações concorrentes mandam o MESMO cookie — a primeira roda-o, a
 * segunda encontra-o revogado, leva 401, e o `refreshSession` faz `logout()`.
 * O utilizador é posto na página de entrada por ter feito duas coisas ao mesmo
 * tempo.
 *
 * E acontece a sério: depois de um F5, várias chamadas partem em paralelo com
 * o token de acesso já expirado e levam 401 quase ao mesmo instante. Numa
 * máquina rápida a primeira renovação acaba antes de a segunda chamada falhar
 * e o defeito não aparece; numa lenta — ou numa rede lenta, que é o caso
 * normal do nosso mercado — sobrepõem-se.
 *
 * Foi assim que apareceu: um teste de reentrada passava aqui e falhava sempre
 * no runner do CI, e o sintoma era o convidado a cair na página de entrada
 * depois de recarregar. Durante seis rondas tratei-o como um problema do
 * ambiente do teste. Era o produto a dizer a verdade.
 *
 * A guarda é uma promessa partilhada: quem chegar enquanto uma renovação
 * decorre espera pela mesma, em vez de começar outra.
 */
let renovacaoEmCurso: Promise<void> | null = null

async function refreshSession(): Promise<void> {
  if (renovacaoEmCurso) return renovacaoEmCurso
  renovacaoEmCurso = renovarUmaVez().finally(() => {
    renovacaoEmCurso = null
  })
  return renovacaoEmCurso
}

async function renovarUmaVez() {
  // Sem corpo: o refresh token vai no cookie HttpOnly (enviado automaticamente).
  const res = await fetch('/api/auth/refresh', { method: 'POST', credentials: 'same-origin' })
  // Só 401/403 são «a sessão não serve». Um 500/502/503 é o servidor com um
  // problema SEU: terminar a sessão aí faz o utilizador perder o sítio onde
  // estava para resolver um problema que não é dele (ver isAuthFailure).
  if (!res.ok && res.status !== 401 && res.status !== 403) {
    throw new ApiError(res.status, null, 'refresh indisponível')
  }
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

/** Resultado do login: sessão, ou desafio de segundo factor. */
export type LoginResult =
  | { kind: 'sessao'; user: User }
  | { kind: 'mfa'; mfa_token: string }

/**
 * Com MFA activo, a password **não** produz sessão: o servidor devolve um
 * desafio de 5 minutos e os tokens só saem no `loginMfa`. Quem chama tem de
 * tratar os dois casos — é por isso que o tipo de retorno os distingue em vez
 * de devolver `User | null`, que se ignora sem dar por isso.
 */
export async function login(email: string, password: string): Promise<LoginResult> {
  const t = await request<AuthOk & { mfa_required?: boolean; mfa_token?: string }>('/api/auth/login', {
    method: 'POST',
    body: JSON.stringify({ email, password }),
  })
  if (t.mfa_required && t.mfa_token) return { kind: 'mfa', mfa_token: t.mfa_token }
  saveSession(t)
  return { kind: 'sessao', user: t.user }
}

/** Verifica se o domínio de email tem SSO configurado. */
export interface SsoCheckResult {
  sso_enabled: boolean
  enforce_sso: boolean
}
export async function ssoCheck(domain: string): Promise<SsoCheckResult> {
  const res = await fetch(`/api/auth/sso/discovery?domain=${encodeURIComponent(domain)}`)
  if (!res.ok) return { sso_enabled: false, enforce_sso: false }
  return res.json()
}

/** Redireciona o browser para o IdP OIDC da organização. */
export function ssoRedirect(domain: string) {
  window.location.href = `/api/auth/sso/authorize?domain=${encodeURIComponent(domain)}`
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

export const iceServers = () => request<RTCConfiguration>('/api/ice-servers')

export interface ChatHistoryMsg {
  id: string
  user_id: string
  username: string
  message: string
  created_at: string
}

export const roomChatHistory = (code: string) =>
  request<ChatHistoryMsg[]>(`/api/rooms/${code}/messages`)

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
  /** `ready` = há ficheiro. `failed` = houve tentativa e não há nada. */
  status: 'ready' | 'failed' | string
  /** Causa em linguagem de utilizador, quando falhou. */
  failure_reason: string | null
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

export const recordingsLibrary = (signal?: AbortSignal) => request<RecordingItem[]>('/api/recordings', { signal })

export const searchUsers = (q: string) =>
  request<User[]>(`/api/users?q=${encodeURIComponent(q)}`)

/** Atualiza os próprios dados (username, password e/ou locale) e sincroniza o cache local. */
// ---------- MFA (segundo factor por TOTP) ----------

export interface MfaEstado {
  enabled: boolean
  /** Inscrito mas por confirmar: o autenticador já tem o segredo, falta a prova. */
  pending: boolean
  backup_codes_left: number
}

export const mfaEstado = () => request<MfaEstado>('/api/users/me/mfa')

/** Começa a inscrição. Devolve o segredo UMA vez — não há como o reler depois. */
export const mfaInscrever = () =>
  request<{ secret: string; otpauth_uri: string }>('/api/users/me/mfa/enroll', { method: 'POST' })

/** Confirma com um código do autenticador. Devolve os códigos de recuperação,
 *  também UMA vez: a partir daqui só existe o hash deles. */
export const mfaActivar = (code: string) =>
  request<{ backup_codes: string[] }>('/api/users/me/mfa/activate', {
    method: 'POST',
    body: JSON.stringify({ code }),
  })

/** Desactiva. Exige um código válido — de outra forma, uma sessão roubada
 *  bastava para desligar o segundo factor. */
export const mfaDesactivar = (code: string) =>
  request<void>('/api/users/me/mfa/disable', {
    method: 'POST',
    body: JSON.stringify({ code }),
  })

/** Segunda metade do login: troca o desafio + código pelos tokens de sessão. */
export async function loginMfa(mfa_token: string, code: string): Promise<User> {
  const t = await request<AuthOk>('/api/auth/login/mfa', {
    method: 'POST',
    body: JSON.stringify({ mfa_token, code }),
  })
  saveSession(t)
  return t.user
}

export async function updateMe(data: { username?: string; password?: string; locale?: string }): Promise<User> {
  const user = await request<User>('/api/users/me', { method: 'PATCH', body: JSON.stringify(data) })
  localStorage.setItem('dx_user', JSON.stringify(user))
  return user
}

// ---------- A minha conta: sessões e exportação de dados ----------

export interface AccountSession {
  session_id: string
  user_agent: string | null
  ip_address: string | null
  started_at: string
  last_used_at: string
  current: boolean
}

export const listSessions = () => request<AccountSession[]>('/api/users/me/sessions')

export const revokeSession = (sessionId: string) =>
  request<void>(`/api/users/me/sessions/${encodeURIComponent(sessionId)}`, { method: 'DELETE' })

export interface AccountDataExport {
  generated_at: string
  profile: User
  organizations: { org_id: string; org_name: string; role: string; title: string }[]
  rooms_owned: { id: string; code: string; name: string; created_at: string }[]
  recordings: { id: string; room_id: string; filename: string; size_bytes: number; created_at: string }[]
}

/** Pede os próprios dados e desencadeia o download como ficheiro JSON — o
 *  pedido precisa do Authorization header, por isso não pode ser um simples
 *  link: busca-se o corpo e constrói-se o download no cliente. */
export async function downloadMyData(): Promise<void> {
  const data = await request<AccountDataExport>('/api/users/me/export')
  const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = 'delonix-meet-dados.json'
  document.body.appendChild(a)
  a.click()
  a.remove()
  URL.revokeObjectURL(url)
}

export const updateEmployee = (
  orgId: string,
  userId: string,
  data: { role?: string; title?: string; branch_id?: string | null; suspended?: boolean },
) => request<Employee>(`/api/orgs/${orgId}/members/${userId}`, { method: 'PATCH', body: JSON.stringify(data) })

export const shareRecording = (id: string, userId: string) =>
  request(`/api/recordings/${id}/shares`, { method: 'POST', body: JSON.stringify({ user_id: userId }) })

export const listRecordingShares = (id: string) => request<User[]>(`/api/recordings/${id}/shares`)

export const unshareRecording = (id: string, userId: string) =>
  request(`/api/recordings/${id}/shares/${userId}`, { method: 'DELETE' })

export interface ShareLink {
  id: string
  recording_id: string
  token: string
  expires_at: string | null
  created_at: string
}

export const getRecordingLink = (id: string) =>
  request<ShareLink | null>(`/api/recordings/${id}/public-link`)

export const createRecordingLink = (id: string, opts: { password?: string; expires_at?: string | null }) =>
  request<ShareLink>(`/api/recordings/${id}/public-link`, {
    method: 'PUT',
    body: JSON.stringify(opts),
  })

export const revokeRecordingLink = (id: string) =>
  request(`/api/recordings/${id}/public-link`, { method: 'DELETE' })

export interface PublicShareInfo {
  recording_id: string
  filename: string
  size_bytes: number
  created_at: string
  download_url: string
  has_password: boolean
}

export async function getPublicShare(token: string, password?: string): Promise<PublicShareInfo> {
  const url = `/api/public/recordings/${token}${password ? `?password=${encodeURIComponent(password)}` : ''}`
  const res = await fetch(url, { credentials: 'same-origin' })
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: res.statusText }))
    throw Object.assign(new Error(body.error ?? 'request failed'), { status: res.status })
  }
  return res.json()
}

export const inviteToRoom = (code: string, targets: string[], kind: 'video' | 'voice' = 'video') =>
  request<{ ringing: string[]; offline: string[] }>(`/api/rooms/${code}/invitations`, {
    method: 'POST',
    body: JSON.stringify({ targets, kind }),
  })

export const listMeetings = (signal?: AbortSignal) => request<Meeting[]>('/api/meetings', { signal })

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
}) => request<Conflicts>('/api/meetings/check-conflicts', { method: 'POST', body: JSON.stringify(body) })

export const deleteMeeting = (id: string) => request(`/api/meetings/${id}`, { method: 'DELETE' })

export const startMeeting = (id: string) =>
  request<{ code: string; kind: 'video' | 'voice' }>(`/api/meetings/${id}/start`, { method: 'POST' })

export const respondMeeting = (id: string, status: 'accepted' | 'declined', reason = '') =>
  request(`/api/meetings/${id}/invitees/me`, { method: 'PUT', body: JSON.stringify({ status, reason }) })

export const meetingInvitees = (id: string) => request<InviteeResponse[]>(`/api/meetings/${id}/invitees`)

export const quarantineAnalytics = (orgId: string, period: 'week' | 'month' | 'quarter' | 'year') =>
  request<QuarantineRow[]>(`/api/orgs/${orgId}/analytics/quarantine?period=${period}`)

export const listMeetingRooms = (orgId: string) => request<MeetingRoom[]>(`/api/orgs/${orgId}/meeting-rooms`)
export const createMeetingRoom = (orgId: string, name: string, location: string, capacity: number) =>
  request<MeetingRoom>(`/api/orgs/${orgId}/meeting-rooms`, {
    method: 'POST',
    body: JSON.stringify({ name, location, capacity }),
  })

export const saveMinutesByRoom = (code: string, minutes: string, transcript: string) =>
  request(`/api/rooms/${code}/minutes`, { method: 'PUT', body: JSON.stringify({ minutes, transcript }) })

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

export const listWhiteboards = (signal?: AbortSignal) => request<WhiteboardMeta[]>('/api/whiteboards', { signal })
export const saveWhiteboard = (title: string, roomCode: string, pngBase64: string) =>
  request<WhiteboardMeta>('/api/whiteboards', {
    method: 'POST',
    body: JSON.stringify({ title, room_code: roomCode, png_base64: pngBase64 }),
  })
export const deleteWhiteboard = (id: string) => request(`/api/whiteboards/${id}`, { method: 'DELETE' })
export const shareWhiteboard = (id: string, isPublic: boolean) =>
  request<WhiteboardMeta>(`/api/whiteboards/${id}/public-link`, {
    method: 'PUT',
    body: JSON.stringify({ public: isPublic }),
  })
export const whiteboardPngUrl = (id: string) => `/api/whiteboards/${id}/image`

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
  request(`/api/orgs/${orgId}`, {
    method: 'PATCH',
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
/** `phone`/`phone_source`/`can_sms` vêm da extensão de SMS (ADR-0005 §Contactos). */
export interface Employee extends Partial<EmployeeSmsFields> {
  user_id: string
  username: string
  email: string
  role: 'admin' | 'member'
  title: string
  branch_id: string | null
  branch_name: string | null
  last_active?: string | null
  /** Bloqueado por um admin (suspenso); continua membro. Ver migração 0054. */
  suspended_at: string | null
}
export interface Group {
  id: string
  org_id: string
  name: string
  member_count: number
}

// ---------- Papéis e permissões (RBAC) ----------

export interface PermissionCatalogEntry {
  key: string
  label: string
}
export interface PermissionGrant {
  permission: string
  requires_approval: boolean
}
export interface RbacRole {
  id: string
  name: string
  is_system: boolean
  parent_role_id: string | null
  parent_role_name: string | null
  member_count: number
  /** Concessões directas deste papel. */
  permissions: PermissionGrant[]
  /** Concessões herdadas do pai (já não repetidas em `permissions`). */
  inherited: PermissionGrant[]
}
export interface RbacRolesResp {
  catalog: PermissionCatalogEntry[]
  roles: RbacRole[]
}
export interface RbacPermissionInput {
  permission: string
  requires_approval?: boolean
}
export interface RbacPermissionRequest {
  id: string
  requester_id: string
  requester_username: string
  requester_email: string
  permission: string
  status: 'pending' | 'approved' | 'denied'
  created_at: string
  decided_at: string | null
  expires_at: string | null
}
export interface MyPermissions {
  admin: boolean
  permissions: string[]
  pending: string[]
}

export const listRoles = (orgId: string, signal?: AbortSignal) =>
  request<RbacRolesResp>(`/api/orgs/${orgId}/roles`, { signal })

export const createRole = (
  orgId: string,
  data: { name: string; parent_role_id?: string | null; permissions?: RbacPermissionInput[] },
) => request<RbacRole>(`/api/orgs/${orgId}/roles`, { method: 'POST', body: JSON.stringify(data) })

export const updateRole = (
  orgId: string,
  roleId: string,
  data: { name?: string; parent_role_id?: string | null; permissions?: RbacPermissionInput[] },
) => request<RbacRole>(`/api/orgs/${orgId}/roles/${roleId}`, { method: 'PATCH', body: JSON.stringify(data) })

export const deleteRole = (orgId: string, roleId: string) =>
  request<void>(`/api/orgs/${orgId}/roles/${roleId}`, { method: 'DELETE' })

export const duplicateRole = (orgId: string, roleId: string) =>
  request<RbacRole>(`/api/orgs/${orgId}/roles/${roleId}/duplicate`, { method: 'POST' })

export const assignRole = (orgId: string, userId: string, roleId: string) =>
  request<void>(`/api/orgs/${orgId}/members/${userId}/role`, {
    method: 'PUT',
    body: JSON.stringify({ role_id: roleId }),
  })

export const listPermissionRequests = (orgId: string, signal?: AbortSignal) =>
  request<RbacPermissionRequest[]>(`/api/orgs/${orgId}/permission-requests`, { signal })

export const decidePermissionRequest = (orgId: string, requestId: string, approve: boolean) =>
  request<void>(`/api/orgs/${orgId}/permission-requests/${requestId}/decide`, {
    method: 'POST',
    body: JSON.stringify({ approve }),
  })

export const myPermissions = (orgId: string) => request<MyPermissions>(`/api/orgs/${orgId}/permissions/me`)

/** Descarrega a matriz de papéis e permissões como CSV. */
export async function downloadRolesCsv(orgId: string): Promise<void> {
  const res = await fetch(`/api/orgs/${orgId}/roles/export.csv`, {
    headers: accessToken ? { Authorization: `Bearer ${accessToken}` } : {},
    credentials: 'same-origin',
  })
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: res.statusText }))
    throw new ApiError(res.status, body, body?.error ?? res.statusText ?? 'request failed')
  }
  const blob = await res.blob()
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = 'papeis-e-permissoes.csv'
  document.body.appendChild(a)
  a.click()
  a.remove()
  URL.revokeObjectURL(url)
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
  /** Delonix Call Quality Score médio (0–100). `null` enquanto nenhum cliente
   *  com a versão que o reporta tiver enviado amostras — `null` diz «ainda não
   *  sei», que é diferente de `0`. */
  avg_score: number | null
  pct_low_score: number | null
  /** % de amostras cuja media passou por TURN relay (custo e latência). */
  pct_turn_relay: number | null
  /** % de amostras com o encoder travado por CPU do CLIENTE (não é a rede). */
  pct_cpu_limited: number | null
  meetings_prev_30d: number
  meeting_minutes_prev_30d: number
  active_users_prev_30d: number
}

/** Amostra de qualidade de chamada (QoS) reportada durante a reunião. */
/** Uma amostra de qualidade de chamada (ver `callQuality.ts` e a migração 0034).
 *  Todos os campos além dos três originais são OPCIONAIS do lado do servidor:
 *  um cliente antigo continua a reportar sem eles. */
export interface QosSample {
  rtt_ms: number | null
  loss_pct: number
  up_kbps: number
  down_kbps?: number
  jitter_ms?: number
  /** Delonix Call Quality Score, 0–100. */
  score?: number
  freeze_ms?: number
  concealment_pct?: number
  frames_dropped?: number
  nack?: number
  pli?: number
  fir?: number
  turn_relay?: boolean
  candidate_pair?: string | null
  limited_by?: string | null
}

/** Tempos de estabelecimento de UMA sessão (ver `callTimings.ts`). */
export const postTimings = (code: string, t: import('./callTimings').Tempos) =>
  request(`/api/rooms/${code}/join-timings`, { method: 'POST', body: JSON.stringify(t) })

export const postQos = (code: string, s: QosSample) =>
  request(`/api/rooms/${code}/quality-samples`, { method: 'POST', body: JSON.stringify(s) })

/** Tradução de uma linha de legenda via LLM local (Ollama in-cluster). */
export const translateCaption = (text: string, target: string) =>
  request<{ text: string }>('/api/ai/translations', { method: 'POST', body: JSON.stringify({ text, target }) })

export const myOrgs = (signal?: AbortSignal) => request<OrgSummary[]>('/api/orgs', { signal })
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
  request<AuditEntry[]>(`/api/orgs/${orgId}/audit-events?limit=${limit}`)
export const createOrg = (name: string) =>
  request<OrgSummary>('/api/orgs', { method: 'POST', body: JSON.stringify({ name }) })
export const listBranches = (orgId: string) => request<Branch[]>(`/api/orgs/${orgId}/branches`)
export const createBranch = (orgId: string, name: string, location: string) =>
  request<Branch>(`/api/orgs/${orgId}/branches`, { method: 'POST', body: JSON.stringify({ name, location }) })
export const listEmployees = (orgId: string) => request<Employee[]>(`/api/orgs/${orgId}/members`)
export const addEmployee = (
  orgId: string,
  body: { email: string; username?: string; password?: string; title?: string; role?: string; branch_id?: string },
) => request<Employee>(`/api/orgs/${orgId}/members`, { method: 'POST', body: JSON.stringify(body) })
export const removeEmployee = (orgId: string, userId: string) =>
  request(`/api/orgs/${orgId}/members/${userId}`, { method: 'DELETE' })

// ---------- Convites por link ("Utilizadores e convites") ----------
//
// Sem SMTP no servidor: o link não é enviado por email, é gerado e o admin
// copia-o e partilha-o pelo canal que preferir — ver InviteDialog.tsx.

export interface Invite {
  id: string
  org_id: string
  email: string
  role: 'admin' | 'member'
  branch_id: string | null
  branch_name: string | null
  title: string
  token: string
  invited_by: string
  invited_by_name: string
  created_at: string
  expires_at: string
  accepted_at: string | null
  revoked_at: string | null
}

export const listInvites = (orgId: string) => request<Invite[]>(`/api/orgs/${orgId}/invites`)

export const createInvite = (
  orgId: string,
  body: { email: string; role?: string; branch_id?: string; title?: string },
) => request<Invite>(`/api/orgs/${orgId}/invites`, { method: 'POST', body: JSON.stringify(body) })

export interface BulkInviteRow {
  email: string
  title?: string
  role?: string
  /** Nome de uma filial existente da org (comparado sem maiúsculas/minúsculas). */
  branch?: string
}
export interface BulkInviteResult {
  email: string
  ok: boolean
  error: string | null
  invite: Invite | null
}
export const bulkCreateInvites = (orgId: string, rows: BulkInviteRow[]) =>
  request<BulkInviteResult[]>(`/api/orgs/${orgId}/invites/bulk`, { method: 'POST', body: JSON.stringify({ rows }) })

export const revokeInvite = (orgId: string, inviteId: string) =>
  request(`/api/orgs/${orgId}/invites/${inviteId}`, { method: 'DELETE' })

/** Forma pública de um convite (sem sessão) — nunca traz `org_id`/`id`/`token`. */
export interface InvitePublic {
  org_name: string
  email: string
  role: string
  title: string
  expired: boolean
  revoked: boolean
  accepted: boolean
}

/** Sem sessão — como `getPublicShare`, `fetch` cru em vez de `request()` (não
 *  se quer a dança de renovação de sessão numa página que ninguém autenticou). */
export async function getInvitePublic(token: string): Promise<InvitePublic> {
  const res = await fetch(`/api/invites/${token}`, { credentials: 'same-origin' })
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: res.statusText }))
    throw Object.assign(new Error(body.error ?? 'request failed'), { status: res.status })
  }
  return res.json()
}

/**
 * Aceita o convite: cria a conta, entra na organização, e devolve a pessoa já
 * LOGADA — `saveSession` é a MESMA função que `registerOrg`/`login` usam, por
 * isso o resto da app (rota, `currentUser()`) não distingue esta entrada de
 * um registo normal.
 */
export async function acceptInvite(token: string, body: { username: string; password: string }): Promise<User> {
  const res = await fetch(`/api/invites/${token}/accept`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    credentials: 'same-origin',
    body: JSON.stringify(body),
  })
  if (!res.ok) {
    const b = await res.json().catch(() => ({ error: res.statusText }))
    throw new ApiError(res.status, b, b?.error ?? res.statusText ?? 'request failed')
  }
  const t: AuthOk = await res.json()
  saveSession(t)
  return t.user
}

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
  request<OdooConfig>(`/api/orgs/${orgId}/integrations/odoo`)

export const saveOdooConfig = (orgId: string, cfg: OdooConfigSaveReq) =>
  request<OdooConfig>(`/api/orgs/${orgId}/integrations/odoo`, {
    method: 'PUT',
    body: JSON.stringify(cfg),
  })

export const rotateOdooToken = (orgId: string) =>
  request<{ token: string; prefix: string }>(`/api/orgs/${orgId}/integrations/odoo/rotate-token`, {
    method: 'POST',
  })

/** Configurações públicas da plataforma (sem autenticação). */
export interface PlatformSettings {
  hide_org_creation: boolean
  hide_sso_button: boolean
}
export const getPlatformSettings = () =>
  fetch('/api/public/settings').then((r) => r.json() as Promise<PlatformSettings>)

// ---------- Platform storage ----------

export interface StorageConfig {
  storage_type: 'local' | 'nfs' | 'webdav'
  nfs_server: string | null
  nfs_path: string | null
  webdav_url: string | null
  webdav_user: string | null
  webdav_password_set: boolean
  webdav_path: string
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

export const getPlatformStorage = () =>
  request<StorageConfig>('/api/operator/v1/storage')

export const savePlatformStorage = (cfg: StorageConfigSaveReq) =>
  request<StorageConfig>('/api/operator/v1/storage', {
    method: 'PUT',
    body: JSON.stringify(cfg),
  })

export const testPlatformStorage = () =>
  request<{ ok: boolean; type: string; message: string }>('/api/operator/v1/storage/test', {
    method: 'POST',
  })

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

export const ackMissedCalls = () => request('/api/users/me/missed-calls/acknowledge', { method: 'POST' })

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
export const roomNotes = (code: string) => request<RoomNotes>(`/api/rooms/${code}/minutes`)

/** URL de objeto para reproduzir a gravação inline (o <video> não envia Bearer). */
export async function recordingObjectUrl(rec: Recording): Promise<string> {
  const res = await fetch(`/api/recordings/${rec.id}/content`, { headers: authHeader() })
  if (!res.ok) throw new Error('failed to load recording')
  return URL.createObjectURL(await res.blob())
}

export async function downloadMeetingIcs(id: string, title: string): Promise<void> {
  const res = await fetch(`/api/meetings/${id}/calendar.ics`, { headers: authHeader() })
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
  const res = await fetch(`/api/recordings/${rec.id}/content?dl=1`, { headers: authHeader() })
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
  return request<AgendaItem[]>(`/api/meetings/${meetingId}/agenda-items`)
}

export async function addAgendaItem(
  meetingId: string,
  item: { topic: string; description?: string; duration_min?: number },
): Promise<AgendaItem> {
  return request<AgendaItem>(`/api/meetings/${meetingId}/agenda-items`, {
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
  return request<AgendaItem>(`/api/meetings/${meetingId}/agenda-items/${itemId}`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(patch),
  })
}

export async function deleteAgendaItem(meetingId: string, itemId: string): Promise<void> {
  await request(`/api/meetings/${meetingId}/agenda-items/${itemId}`, { method: 'DELETE' })
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
  meetingId: string,
  itemId: string,
  patch: Partial<Omit<ActionItem, 'id' | 'plan_id' | 'created_at' | 'updated_at'>>,
): Promise<ActionItem> {
  return request<ActionItem>(`/api/meetings/${meetingId}/action-plan/items/${itemId}`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(patch),
  })
}

export async function deleteActionItem(meetingId: string, itemId: string): Promise<void> {
  await request(`/api/meetings/${meetingId}/action-plan/items/${itemId}`, { method: 'DELETE' })
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

// ---------- frontend/b1-sala ----------

/** Campos que o histórico de chat passou a trazer (fios e reacções). */
export interface ChatHistoryMsg {
  /** Mensagem a que esta responde (fio), ou `null`. */
  parent_id?: string | null
  /** Contagem de reacções por emoji (`{}` sem reacções). */
  reactions?: Record<string, number>
}

/** Quem espera na sala de espera (só dono/co-anfitrião — 403/404 aos outros). */
export interface WaitingPeer {
  peer_id: string
  username: string
  origin?: 'sso' | 'password' | 'guest' | 'pstn' | 'bot'
  title?: string
  /** Epoch ms de quando começou a esperar. */
  since: number
}

/**
 * Espreitar a sala de espera ANTES de entrar. O `?room=` é a chave de
 * afinidade do balanceador: a fila vive na memória do pod da sala.
 */
export const roomWaiting = (code: string) =>
  request<WaitingPeer[]>(`/api/rooms/${code}/waiting?room=${encodeURIComponent(code)}`)

/** Resultado de uma sondagem de rede contra este servidor. */
export interface NetProbeResult {
  download_bytes: number
  download_ms: number
  /** kbit/s medidos no cliente. */
  download_kbps: number
  upload_bytes: number
  /** Tempo a ler o corpo, medido NO SERVIDOR. */
  upload_server_ms: number
  upload_kbps: number
}

/**
 * Sondagem de descarga e subida (tecto 4 MiB por pedido; 30 sondagens por
 * conta por minuto). `bytes` é por sentido; por omissão 256 KiB.
 */
export async function netProbe(bytes = 256 * 1024, signal?: AbortSignal): Promise<NetProbeResult> {
  const t0 = performance.now()
  const dl = await fetch(`/api/net-probe?bytes=${bytes}`, { headers: authHeader(), cache: 'no-store', signal })
  if (!dl.ok) throw new ApiError(dl.status, null, `net-probe ${dl.status}`)
  const down = await dl.arrayBuffer()
  const downloadMs = Math.max(1, performance.now() - t0)

  const up = await fetch('/api/net-probe', {
    method: 'POST',
    headers: { ...authHeader(), 'Content-Type': 'application/octet-stream' },
    body: new Uint8Array(bytes),
    signal,
  })
  if (!up.ok) throw new ApiError(up.status, null, `net-probe ${up.status}`)
  const r = (await up.json()) as { bytes: number; server_ms: number }
  const serverMs = Math.max(1, r.server_ms)
  return {
    download_bytes: down.byteLength,
    download_ms: downloadMs,
    download_kbps: (down.byteLength * 8) / downloadMs,
    upload_bytes: r.bytes,
    upload_server_ms: r.server_ms,
    upload_kbps: (r.bytes * 8) / serverMs,
  }
}
// ---------- frontend/b1-gravacoes ----------
//
// Contrato das rotas abertas pelo branch `frontend/b1-gravacoes` (migrações
// 0040–0046). Os tipos estendem os que já existem em vez de os alterar, para a
// integração com os outros branches do lote ser trivial.

/** Pedido cuja resposta de sucesso não tem corpo (`204 No Content`). */
async function requestEmpty(path: string, options: RequestInit = {}, retry = true): Promise<void> {
  const headers: Record<string, string> = {
    ...(options.body ? { 'Content-Type': 'application/json' } : {}),
    ...(options.headers as Record<string, string>),
  }
  if (accessToken) headers['Authorization'] = `Bearer ${accessToken}`
  const res = await fetch(path, { ...options, headers, credentials: 'same-origin' })
  if (res.status === 401 && retry && localStorage.getItem('dx_user')) {
    await refreshSession()
    return requestEmpty(path, options, false)
  }
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: res.statusText }))
    throw new ApiError(res.status, body, body?.error ?? res.statusText ?? 'request failed')
  }
}

export type SessionKind = 'meeting' | 'training' | 'broadcast' | 'hybrid'
export type RecordQuality = '2160p' | '1080p' | '720p' | 'audio'
export type RecordingFileStatus = 'processing' | 'transcribing' | 'ready' | 'failed'
export type TranscriptStatus = 'none' | 'transcribing' | 'ready' | 'failed'

/** Página de uma listagem por cursor (`page_size` ≤ 100, `page_token` opaco). */
export interface Page<T> {
  items: T[]
  next_page_token: string | null
}

export interface PageParams {
  page_size?: number
  page_token?: string | null
}

function pageQuery(p?: PageParams): string {
  const q = new URLSearchParams()
  if (p?.page_size) q.set('page_size', String(p.page_size))
  if (p?.page_token) q.set('page_token', p.page_token)
  const s = q.toString()
  return s ? `?${s}` : ''
}

/** Item da biblioteca com metadados de media, estados, contagens e publicação. */
export interface RecordingLibraryItem extends RecordingItem {
  status: RecordingFileStatus
  /** `status`, com `published` quando está pronta e publicada. */
  state: RecordingFileStatus | 'published'
  progress_pct: number | null
  kind: SessionKind
  /** Medidos com ffprobe; `null` = não foi possível medir (nunca inventado). */
  duration_ms: number | null
  width: number | null
  height: number | null
  fps: number | null
  video_codec: string | null
  audio_codec: string | null
  has_thumbnail: boolean
  transcript_status: TranscriptStatus
  transcript_language: string | null
  transcribed_at: string | null
  chapter_count: number
  comment_count: number
  view_count: number
  participant_count: number
  /** Línguas com legenda publicada. */
  caption_languages: string[]
  description: string
  tags: string[]
  visibility: 'private' | 'org'
  published_at: string | null
  can_manage: boolean
  uploader_org_id: string | null
  uploader_org_name: string | null
}

/**
 * Biblioteca com metadados. `q` pesquisa no nome, autor, sala, descrição,
 * etiquetas e na TRANSCRIÇÃO. `scope: 'published'` lista as publicadas que o
 * utilizador vê (incluindo as da organização em que não participou).
 */
export const recordingsLibraryMeta = (
  params: { q?: string; scope?: 'mine' | 'published' } = {},
  signal?: AbortSignal,
) => {
  const q = new URLSearchParams()
  if (params.q) q.set('q', params.q)
  if (params.scope) q.set('scope', params.scope)
  const s = q.toString()
  return request<RecordingLibraryItem[]>(`/api/recordings${s ? `?${s}` : ''}`, { signal })
}

export const recordingDetails = (id: string, signal?: AbortSignal) =>
  request<RecordingLibraryItem>(`/api/recordings/${id}/details`, { signal })

export const updateRecording = (id: string, patch: { filename?: string; description?: string; tags?: string[] }) =>
  request<RecordingLibraryItem>(`/api/recordings/${id}`, { method: 'PATCH', body: JSON.stringify(patch) })

export const publishRecording = (id: string) =>
  request<RecordingLibraryItem>(`/api/recordings/${id}/publish`, {
    method: 'POST',
    body: JSON.stringify({ visibility: 'org' }),
  })

export const unpublishRecording = (id: string) =>
  request<RecordingLibraryItem>(`/api/recordings/${id}/unpublish`, { method: 'POST' })

/** URL de objecto da miniatura (o `<img>` não envia Bearer). Rejeita com 404 se não houver. */
export async function recordingThumbnailUrl(id: string): Promise<string> {
  const res = await fetch(`/api/recordings/${id}/thumbnail`, { headers: authHeader() })
  if (!res.ok) throw new ApiError(res.status, null, 'sem miniatura')
  return URL.createObjectURL(await res.blob())
}

/** Regista uma visualização (uma por pessoa por dia). */
export const recordRecordingView = (id: string) => requestEmpty(`/api/recordings/${id}/views`, { method: 'POST' })

export interface RecordingParticipant {
  user_id: string
  username: string
  joined_at: string
}

export const recordingParticipants = (id: string, page?: PageParams) =>
  request<Page<RecordingParticipant>>(`/api/recordings/${id}/participants${pageQuery(page)}`)

export const roomParticipants = (code: string, page?: PageParams) =>
  request<Page<RecordingParticipant>>(`/api/rooms/${code}/participants${pageQuery(page)}`)

export interface TranscriptSegment {
  start_ms: number
  end_ms: number
  text: string
  confidence: number | null
}

export interface RecordingTranscript {
  recording_id: string
  status: TranscriptStatus
  progress_pct: number | null
  language: string | null
  /** Média de exp(avg_logprob) dos segmentos, 0–1. */
  confidence: number | null
  transcribed_at: string | null
  error: string | null
  text: string
  segments: TranscriptSegment[]
}

export const recordingTranscript = (id: string, signal?: AbortSignal) =>
  request<RecordingTranscript>(`/api/recordings/${id}/transcript`, { signal })

export interface RecordingComment {
  id: string
  recording_id: string
  user_id: string
  username: string
  /** Instante do vídeo; `null` = comentário à gravação inteira. */
  t_ms: number | null
  body: string
  created_at: string
  can_delete: boolean
}

export const recordingComments = (id: string, page?: PageParams) =>
  request<Page<RecordingComment>>(`/api/recordings/${id}/comments${pageQuery(page)}`)

export const addRecordingComment = (id: string, body: string, tMs?: number | null) =>
  request<RecordingComment>(`/api/recordings/${id}/comments`, {
    method: 'POST',
    body: JSON.stringify({ body, t_ms: tMs ?? null }),
  })

export const deleteRecordingComment = (id: string, commentId: string) =>
  requestEmpty(`/api/recordings/${id}/comments/${commentId}`, { method: 'DELETE' })

export interface RecordingChapter {
  id: string
  recording_id: string
  t_ms: number
  title: string
  source: 'auto' | 'manual'
  created_at: string
}

export const recordingChapters = (id: string) => request<RecordingChapter[]>(`/api/recordings/${id}/chapters`)

export const addRecordingChapter = (id: string, tMs: number, title: string) =>
  request<RecordingChapter>(`/api/recordings/${id}/chapters`, {
    method: 'POST',
    body: JSON.stringify({ t_ms: tMs, title }),
  })

export const updateRecordingChapter = (id: string, chapterId: string, patch: { t_ms?: number; title?: string }) =>
  request<RecordingChapter>(`/api/recordings/${id}/chapters/${chapterId}`, {
    method: 'PATCH',
    body: JSON.stringify(patch),
  })

export const deleteRecordingChapter = (id: string, chapterId: string) =>
  requestEmpty(`/api/recordings/${id}/chapters/${chapterId}`, { method: 'DELETE' })

/**
 * Gera (ou volta a gerar) os capítulos automáticos pelo LLM local; os manuais
 * ficam. `409` sem transcrição, `503` sem LLM.
 */
export const generateRecordingChapters = (id: string) =>
  request<RecordingChapter[]>(`/api/recordings/${id}/chapters/generate`, { method: 'POST' })

// ---------- IA local no servidor (Ollama) para o Estúdio ----------

/** Estado do LLM local visto pela organização. Sempre 200: o erro vem em `error`. */
export interface StudioAiStatus {
  configured: boolean
  reachable: boolean
  model: string
  model_installed: boolean | null
  error: string | null
}

export const studioAiStatus = (orgId: string, signal?: AbortSignal) =>
  request<StudioAiStatus>(`/api/orgs/${orgId}/ai/status`, { signal })

export type StudioAiTask = 'summary' | 'publication' | 'fillers'

export interface StudioAiRequest {
  task: StudioAiTask
  language?: string
  title?: string
  segments: { start_ms: number; end_ms: number; text: string }[]
}

export interface StudioAiSummary {
  summary: string
  chapters: { t_ms: number; title: string }[]
}
export interface StudioAiPublication {
  title: string
  description: string
  tags: string[]
}
export interface StudioAiFillers {
  terms: string[]
}

/**
 * Pede ao LLM local da organização (Ollama, pelo servidor — o browser nunca
 * fala com o Ollama). Nada fica guardado no servidor. `503` com a razão quando
 * o modelo não está disponível; `429` quando a organização já tem um pedido a
 * correr.
 */
export const studioAi = <T,>(orgId: string, body: StudioAiRequest, signal?: AbortSignal) =>
  request<T>(`/api/orgs/${orgId}/ai/suggestions`, { method: 'POST', body: JSON.stringify(body), signal })

export interface RecordingCaption {
  recording_id: string
  lang: string
  source: 'upload' | 'transcript' | 'translation'
  status: 'generating' | 'draft' | 'published' | 'failed'
  progress_pct: number | null
  error: string | null
  created_at: string
  updated_at: string
  published_at: string | null
}

export const recordingCaptions = (id: string) => request<RecordingCaption[]>(`/api/recordings/${id}/captions`)

export const recordingCaption = (id: string, lang: string) =>
  request<RecordingCaption & { vtt: string }>(`/api/recordings/${id}/captions/${lang}`)

/** URL de objecto do VTT para `<track src>` (o elemento não envia Bearer). */
export async function recordingCaptionVttUrl(id: string, lang: string): Promise<string> {
  const res = await fetch(`/api/recordings/${id}/captions/${lang}/vtt`, { headers: authHeader() })
  if (!res.ok) throw new ApiError(res.status, null, 'legenda indisponível')
  return URL.createObjectURL(await res.blob())
}

export const putRecordingCaption = (id: string, lang: string, vtt: string, publish = false) =>
  request<RecordingCaption>(`/api/recordings/${id}/captions/${lang}`, {
    method: 'PUT',
    body: JSON.stringify({ vtt, publish }),
  })

export const setRecordingCaptionStatus = (id: string, lang: string, status: 'draft' | 'published') =>
  request<RecordingCaption>(`/api/recordings/${id}/captions/${lang}`, {
    method: 'PATCH',
    body: JSON.stringify({ status }),
  })

export const deleteRecordingCaption = (id: string, lang: string) =>
  requestEmpty(`/api/recordings/${id}/captions/${lang}`, { method: 'DELETE' })

/**
 * Gera legenda: na língua da transcrição sai já (rascunho); noutra língua é
 * traduzida em segundo plano (`status: 'generating'`, ver `progress_pct`).
 */
export const generateRecordingCaption = (id: string, lang?: string) =>
  request<RecordingCaption>(`/api/recordings/${id}/captions/generate`, {
    method: 'POST',
    body: JSON.stringify(lang ? { lang } : {}),
  })

export interface RecordingUploadResult extends Recording {
  kind: SessionKind
  status: RecordingFileStatus
  duration_ms: number | null
  width: number | null
  height: number | null
  fps: number | null
  video_codec: string | null
  audio_codec: string | null
  has_thumbnail: boolean
}

/** Upload que declara o tipo de sessão (o estúdio envia `broadcast`) e devolve os metadados medidos. */
export async function uploadRecordingWithKind(
  code: string,
  blob: Blob,
  name: string,
  kind?: SessionKind,
): Promise<RecordingUploadResult> {
  const q = new URLSearchParams({ name })
  if (kind) q.set('kind', kind)
  const res = await fetch(`/api/rooms/${code}/recordings?${q}`, {
    method: 'POST',
    headers: authHeader(),
    body: blob,
  })
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: res.statusText }))
    throw new ApiError(res.status, body, body?.error ?? 'upload failed')
  }
  return res.json()
}

/** Opções de sessão de uma reunião agendada (passadas à sala no arranque). */
export interface MeetingSessionOptions {
  format: SessionKind
  waiting_room: boolean
  auto_record: boolean
  record_quality: RecordQuality
}

export interface MeetingWithOptions extends Omit<Meeting, 'my_status'>, MeetingSessionOptions {
  my_status?: 'owner' | 'pending' | 'accepted' | 'declined' | 'tentative'
  /** Convidados (sem o anfitrião), qualquer que seja a resposta. */
  invitee_count: number
  /** Sistema de origem (`odoo`), ou `null` se criada no Meet. */
  external_source: string | null
}

export const listMeetingsWithOptions = (signal?: AbortSignal) =>
  request<MeetingWithOptions[]>('/api/meetings', { signal })

export const createMeetingWithOptions = (
  m: Parameters<typeof createMeeting>[0] & Partial<MeetingSessionOptions>,
) =>
  request<Meeting & MeetingSessionOptions & { conflicts: Conflicts }>('/api/meetings', {
    method: 'POST',
    body: JSON.stringify(m),
  })

export const startMeetingWithOptions = (id: string) =>
  request<{ code: string; kind: 'video' | 'voice' } & MeetingSessionOptions>(`/api/meetings/${id}/start`, {
    method: 'POST',
  })

export const respondMeetingStatus = (id: string, status: 'accepted' | 'declined' | 'tentative', reason = '') =>
  request(`/api/meetings/${id}/invitees/me`, { method: 'PUT', body: JSON.stringify({ status, reason }) })

// ---------- frontend/b1-emissao ----------

/**
 * Destino de directo GUARDADO pela organização (`server/src/stream_destinations.rs`,
 * G1 — regras em `delonix_meet_domain::content::stream_destination`).
 *
 * A chave de emissão NÃO vem aqui, nem cifrada: as respostas trazem só
 * `key_prefix` (para a reconhecer) e `has_key`. Para emitir com ele, passa
 * `{ id }` no `Destino` do `studio/directo.ts` — o servidor decifra a chave
 * do lado dele. A chave em claro só volta UMA vez, na criação e na rotação
 * (`StreamDestinationWithKey.stream_key`).
 */
export type StreamKind = 'youtube' | 'facebook' | 'linkedin' | 'rtmp' | 'internal'
export type StreamDestinationState = 'ready' | 'expired' | 'error'

export interface StreamDestination {
  id: string
  org_id: string
  kind: StreamKind
  label: string
  /** URL base, sem a chave. */
  url: string
  /** Primeiros caracteres da chave, para a reconhecer. */
  key_prefix: string
  has_key: boolean
  state: StreamDestinationState
  created_by: string
  created_at: string
  updated_at: string
}

/** Resposta da criação e da rotação: o destino e a chave em claro, uma única vez. */
export interface StreamDestinationWithKey extends StreamDestination {
  stream_key?: string
}

export interface StreamDestinationPage {
  items: StreamDestination[]
  next_page_token?: string
}

export interface StreamDestinationCreate {
  kind: string
  label: string
  url: string
  stream_key?: string
}

/** Omitir um campo mantém-no; a chave só muda por `rotateStreamDestinationKey`. */
export type StreamDestinationPatch = Partial<Pick<StreamDestination, 'label' | 'url' | 'state'>>

/** Lista, paginada, por data de criação (membro da organização). */
export const listStreamDestinations = (
  orgId: string,
  params: { pageSize?: number; pageToken?: string } = {},
  signal?: AbortSignal,
) => {
  const q = new URLSearchParams()
  if (params.pageSize) q.set('page_size', String(params.pageSize))
  if (params.pageToken) q.set('page_token', params.pageToken)
  const qs = q.toString()
  return request<StreamDestinationPage>(
    `/api/orgs/${orgId}/stream-destinations${qs ? `?${qs}` : ''}`,
    { signal },
  )
}

export const getStreamDestination = (orgId: string, id: string, signal?: AbortSignal) =>
  request<StreamDestination>(`/api/orgs/${orgId}/stream-destinations/${id}`, { signal })

/** Cria (administrador). `409` = nome repetido ou tecto; `503` = servidor sem `DATA_ENCRYPTION_KEYS`. */
export const createStreamDestination = (orgId: string, body: StreamDestinationCreate) =>
  request<StreamDestinationWithKey>(`/api/orgs/${orgId}/stream-destinations`, {
    method: 'POST',
    body: JSON.stringify(body),
  })

/** Altera rótulo, URL ou estado (administrador). Nunca a chave — ver `rotateStreamDestinationKey`. */
export const updateStreamDestination = (orgId: string, id: string, patch: StreamDestinationPatch) =>
  request<StreamDestination>(`/api/orgs/${orgId}/stream-destinations/${id}`, {
    method: 'PATCH',
    body: JSON.stringify(patch),
  })

/** Roda a chave (administrador): devolve-a em claro esta única vez. */
export const rotateStreamDestinationKey = (orgId: string, id: string, streamKey: string) =>
  request<StreamDestinationWithKey>(`/api/orgs/${orgId}/stream-destinations/${id}/rotate-key`, {
    method: 'POST',
    body: JSON.stringify({ stream_key: streamKey }),
  })

/**
 * Apaga (administrador). O servidor responde `204` sem corpo, que o `request`
 * (que lê sempre JSON) não sabe tratar — por isso este pedido tem o seu
 * próprio caminho, com a mesma renovação de sessão no `401`.
 */
export async function deleteStreamDestination(orgId: string, id: string): Promise<void> {
  const path = `/api/orgs/${orgId}/stream-destinations/${id}`
  const tentar = () =>
    fetch(path, { method: 'DELETE', headers: authHeader(), credentials: 'same-origin' })
  let res = await tentar()
  if (res.status === 401 && localStorage.getItem('dx_user')) {
    await refreshSession()
    res = await tentar()
  }
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: res.statusText }))
    throw new ApiError(res.status, body, body?.error ?? res.statusText ?? 'request failed')
  }
}

// ---------- frontend/l1-consola ----------

/** `GET /api/status` — público, sem sessão (`server/src/main.rs` `status`). */
export interface ServerStatus {
  status: 'ok' | 'degraded' | string
  api: boolean
  db: boolean
  uptime_secs: number
  version: string
}
export async function serverStatus(signal?: AbortSignal): Promise<ServerStatus> {
  const r = await fetch('/api/status', { signal, cache: 'no-store' })
  if (!r.ok) throw new ApiError(r.status, null, r.statusText)
  return (await r.json()) as ServerStatus
}

/**
 * `GET /api/orgs/{org_id}/audit-events/verification` — recalcula a cadeia de hashes do registo
 * de auditoria (migração 0037). Só admins. `intact: false` diz em que registo
 * a cadeia partiu.
 */
export interface AuditChainCheck {
  intact: boolean
  entries: number
  broken_at_seq: number | null
  detail: string
}
export const verifyAudit = (orgId: string, signal?: AbortSignal) =>
  request<AuditChainCheck>(`/api/orgs/${orgId}/audit-events/verification`, { signal })

// Dial-in PSTN — plano de controlo (`server/src/voice.rs`). Salas de voz com
// número e PIN, inventário de DIDs, CDR e resumo de facturação. A camada de
// media (Kamailio + FreeSWITCH, `voice/`) e a ponte FreeSWITCH↔SFU são outra
// coisa: sem a ponte, quem liga fala numa conferência só de voz, não na sala.

export interface VoiceRoomCreated {
  id: string
  room_code: string
  pin: string
  dial_in_number: string | null
  media_backend: string
}
export interface VoiceRoom {
  id: string
  org_id: string
  room_code: string
  pin: string
  did_id: string | null
  media_backend: string
  status: 'active' | 'closed' | string
  created_at: string
}
export interface VoiceParticipant {
  id: string
  channel: string
  caller_number: string
  joined_at: string
  left_at: string | null
}
export interface VoiceDid {
  id: string
  org_id: string | null
  e164: string
  market: string
  model: 'shared' | 'dedicated' | string
  provider: string
  active: boolean
  created_at: string
  /** Ramal a que este número está atribuído (Fase 2 — ver Extension), ou null se livre. */
  extension_id: string | null
}
export interface VoiceCdr {
  id: string
  direction: string
  caller_number: string
  did_e164: string
  duration_secs: number
  cost_estimate: number
  started_at: string
  ended_at: string | null
}
export type VoicePeriod = 'week' | 'month' | 'quarter' | 'year'
export interface VoiceBilling {
  period: string
  calls: number
  total_minutes: number
  total_cost: number
  currency_note: string
}

export const createVoiceRoom = (orgId: string, roomCode: string, didId?: string) =>
  request<VoiceRoomCreated>(`/api/orgs/${orgId}/voice/rooms`, {
    method: 'POST',
    body: JSON.stringify({ room_code: roomCode, ...(didId ? { did_id: didId } : {}) }),
  })
export const getVoiceRoom = (orgId: string, id: string) => request<VoiceRoom>(`/api/orgs/${orgId}/voice/rooms/${id}`)
export const voiceRoomParticipants = (orgId: string, id: string) =>
  request<VoiceParticipant[]>(`/api/orgs/${orgId}/voice/rooms/${id}/participants`)
export const closeVoiceRoom = (orgId: string, id: string) =>
  request<{ ok: boolean }>(`/api/orgs/${orgId}/voice/rooms/${id}/close`, { method: 'POST' })
export const listVoiceDids = (orgId: string, signal?: AbortSignal) =>
  request<VoiceDid[]>(`/api/orgs/${orgId}/voice/dids`, { signal })
export const createVoiceDid = (
  orgId: string,
  did: { e164: string; market?: string; model?: 'shared' | 'dedicated'; provider?: string; org_scoped?: boolean },
) => request<VoiceDid>(`/api/orgs/${orgId}/voice/dids`, { method: 'POST', body: JSON.stringify(did) })
export const listVoiceCdr = (orgId: string, signal?: AbortSignal) =>
  request<VoiceCdr[]>(`/api/orgs/${orgId}/voice/call-records`, { signal })
export const voiceBilling = (orgId: string, period: VoicePeriod = 'month', signal?: AbortSignal) =>
  request<VoiceBilling>(`/api/orgs/${orgId}/voice/billing?period=${period}`, { signal })

// ---------- Ramais internos (extensão SIP — chamada ramal-a-ramal, Fase 1) ----------
// Só interno: SEM PSTN e SEM ponte para salas de reunião (fases seguintes do
// mesmo plano). Ver server/src/ramais.rs para a fronteira exata.

export interface Extension {
  id: string
  org_id: string
  member_id: string
  member_username: string
  member_email: string
  extension: string
  sip_username: string
  label: string
  active: boolean
  created_at: string
}
/** Resposta de criação/regeneração: só existe UMA VEZ — copiar para o softphone. */
export interface ExtensionCreated extends Extension {
  sip_password: string
  sip_domain: string
}

export const listExtensions = (orgId: string, signal?: AbortSignal) =>
  request<Extension[]>(`/api/orgs/${orgId}/extensions`, { signal })
export const createExtension = (
  orgId: string,
  body: { member_id: string; extension: string; label?: string },
) => request<ExtensionCreated>(`/api/orgs/${orgId}/extensions`, { method: 'POST', body: JSON.stringify(body) })
export const updateExtension = (orgId: string, id: string, body: { label?: string; active?: boolean }) =>
  request<Extension>(`/api/orgs/${orgId}/extensions/${id}`, { method: 'PATCH', body: JSON.stringify(body) })
export const regenerateExtensionPassword = (orgId: string, id: string) =>
  request<ExtensionCreated>(`/api/orgs/${orgId}/extensions/${id}/regenerate-password`, { method: 'POST' })
export const deleteExtension = (orgId: string, id: string) =>
  requestEmpty(`/api/orgs/${orgId}/extensions/${id}`, { method: 'DELETE' })

// ---------- Fase 2: ramal alcançável do PSTN via DID dedicado ----------
// Só voz directa (bridge ao ramal, sem PIN) — continua SEM ponte para salas
// de reunião em vídeo. Ver server/src/ramais.rs para a fronteira exacta.

export interface ExtensionDidInfo {
  did_id: string
  e164: string
}

export const assignExtensionDid = (orgId: string, id: string, didId: string) =>
  request<ExtensionDidInfo>(`/api/orgs/${orgId}/extensions/${id}/did`, {
    method: 'PUT',
    body: JSON.stringify({ did_id: didId }),
  })
export const unassignExtensionDid = (orgId: string, id: string) =>
  requestEmpty(`/api/orgs/${orgId}/extensions/${id}/did`, { method: 'DELETE' })

/** Tecto de upload de uma gravação no servidor (recordings.rs MAX_RECORDING_BYTES). */
export const MAX_RECORDING_UPLOAD_BYTES = 512 * 1024 * 1024

// ---------- delonix-meet-backend/sms-contactos ----------
// SMS a contactos e de reunião (ADR-0005 §Contactos). Só o cliente de API: os
// ecrãs são da UI nova (`frontend/ui-template-rebuild`). O número de um
// contacto NUNCA sai daqui — manda-se o `user_id` e o servidor resolve-o.

export type SmsPurpose = 'direct' | 'contact' | 'meeting_invite' | 'meeting_reminder'
export type SmsStatus = 'queued' | 'claimed' | 'sent' | 'failed'

export interface SmsMessage {
  id: string
  /** E.164 para admin; mascarado (`+244*******00`) para um membro. */
  to: string
  body: string
  encoding: 'gsm7' | 'ucs2'
  segments: number
  route: 'usb' | 'operator'
  operator: string | null
  device_id: string | null
  status: SmsStatus
  error: string | null
  provider_ref: string | null
  created_at: string
  sent_at: string | null
  purpose: SmsPurpose
  recipient_user_id: string | null
  meeting_id: string | null
  created_by: string | null
}

/** Campos que `GET /api/orgs/{org_id}/employees` acrescenta a cada `Employee`. */
export interface EmployeeSmsFields {
  /** Só para admin ou o próprio; `null` para colegas. */
  phone: string | null
  phone_source: 'odoo' | 'manual' | null
  /** Tem número e não desligou SMS de contactos. */
  can_sms: boolean
}

export type SmsSendPolicy = 'admins' | 'members'

/** Códigos estáveis no início de `error` (ver `smsErrorCode`). */
export type SmsErrorCode =
  | 'sms.recipient_opted_out'
  | 'sms.recipient_no_phone'
  | 'sms.target_ambiguous'
  | 'sms.target_missing'
  | 'sms.idempotency_key_in_use'
  | 'sms.reminder_recurring_unsupported'
  | 'sms.no_org'

/** Extrai o código `sms.*` do texto de erro do servidor, se houver. */
export function smsErrorCode(message: string): SmsErrorCode | null {
  const m = /^(sms\.[a-z_]+)(?::|$)/.exec(message.trim())
  return (m?.[1] as SmsErrorCode | undefined) ?? null
}

/** Envia a um contacto da org. `idempotencyKey`: uma por intenção de envio (repetir não duplica). */
export const sendSmsToContact = (
  orgId: string,
  body: { user_id: string; body: string; route?: 'auto' | 'usb' | 'operator' },
  idempotencyKey?: string,
) =>
  request<SmsMessage>(`/api/orgs/${orgId}/sms/messages`, {
    method: 'POST',
    body: JSON.stringify(body),
    headers: idempotencyKey ? { 'Idempotency-Key': idempotencyKey } : {},
  })

/** Admin vê todas as mensagens da org; membro vê só as que enviou. */
export const listSmsMessages = (orgId: string, pageSize = 50) =>
  request<{ items: SmsMessage[]; next_page_token: string | null }>(
    `/api/orgs/${orgId}/sms/messages?page_size=${pageSize}`,
  )

export const getSmsMessage = (orgId: string, messageId: string) =>
  request<SmsMessage>(`/api/orgs/${orgId}/sms/messages/${messageId}`)

export const getSmsPolicy = (orgId: string) =>
  request<{ send_policy: SmsSendPolicy }>(`/api/orgs/${orgId}/sms/policy`)

export const setSmsPolicy = (orgId: string, sendPolicy: SmsSendPolicy) =>
  request<{ send_policy: SmsSendPolicy }>(`/api/orgs/${orgId}/sms/policy`, {
    method: 'PUT',
    body: JSON.stringify({ send_policy: sendPolicy }),
  })

// ---------- SMS: gateways USB/telefone, dispositivos e encaminhamento ----------

export interface SmsGateway {
  id: string
  name: string
  prefix: string
  created_at: string
  last_seen_at: string | null
  online: boolean
}
/** Devolvido só na criação — o token completo (`dlxg_...`) não volta a aparecer. */
export interface CreatedSmsGateway {
  id: string
  name: string
  prefix: string
  token: string
}
export interface SmsDevice {
  id: string
  gateway_id: string
  gateway_name: string
  device_key: string
  vendor_id: string
  product_id: string
  manufacturer: string | null
  product: string | null
  serial: string | null
  kind: string
  transport: string
  port: string | null
  capable: boolean
  reason: string | null
  operator_name: string | null
  signal_percent: number | null
  last_seen_at: string
  online: boolean
  selected: boolean
}
export interface SmsOperatorInfo {
  operator: string
  label: string
  prefixes: string[]
  configured: boolean
}
export interface SmsRoute {
  device_id: string | null
  operators: SmsOperatorInfo[]
}

export const listSmsGateways = (orgId: string, signal?: AbortSignal) =>
  request<SmsGateway[]>(`/api/orgs/${orgId}/sms/gateways`, { signal })

export const createSmsGateway = (orgId: string, name?: string) =>
  request<CreatedSmsGateway>(`/api/orgs/${orgId}/sms/gateways`, {
    method: 'POST',
    body: JSON.stringify({ name: name ?? '' }),
  })

export const revokeSmsGateway = (orgId: string, gatewayId: string) =>
  requestEmpty(`/api/orgs/${orgId}/sms/gateways/${gatewayId}`, { method: 'DELETE' })

export const listSmsDevices = (orgId: string, signal?: AbortSignal) =>
  request<SmsDevice[]>(`/api/orgs/${orgId}/sms/devices`, { signal })

export const getSmsRoute = (orgId: string, signal?: AbortSignal) =>
  request<SmsRoute>(`/api/orgs/${orgId}/sms/route`, { signal })

export const putSmsRoute = (orgId: string, deviceId: string | null) =>
  request<SmsRoute>(`/api/orgs/${orgId}/sms/route`, {
    method: 'PUT',
    body: JSON.stringify({ device_id: deviceId }),
  })

/** O próprio ou um admin. `phone: null` apaga (fica `manual`); `follow_directory` devolve o campo ao Odoo. */
export const setMemberPhone = (
  orgId: string,
  userId: string,
  change: { phone: string | null } | { follow_directory: true },
) =>
  request<{ user_id: string; phone: string | null; phone_source: 'manual' | null }>(
    `/api/orgs/${orgId}/members/${userId}/phone`,
    { method: 'PUT', body: JSON.stringify(change) },
  )

export interface SmsPreferences {
  contact_opt_out: boolean
  meeting_opt_out: boolean
  phones: { org_id: string; org_name: string; phone: string | null; phone_source: 'odoo' | 'manual' | null }[]
}

export const getSmsPreferences = () => request<SmsPreferences>('/api/users/me/sms-preferences')

export const setSmsPreferences = (prefs: { contact_opt_out?: boolean; meeting_opt_out?: boolean }) =>
  request<SmsPreferences>('/api/users/me/sms-preferences', { method: 'PUT', body: JSON.stringify(prefs) })

export interface MeetingSmsOptions {
  sms_invite?: boolean
  /** 5–1440; recusado (422) com recorrência. */
  sms_reminder_min?: number | null
}

export interface MeetingSmsReport {
  invite: { queued: number; skipped: { user_id: string; reason: string }[] } | null
  reminder_min: number | null
}

/** `createMeeting` com as opções de SMS; `sms` só vem quando foram pedidas. */
export const createMeetingWithSms = (m: Parameters<typeof createMeeting>[0] & MeetingSmsOptions) =>
  request<Meeting & { conflicts: Conflicts; sms?: MeetingSmsReport }>('/api/meetings', {
    method: 'POST',
    body: JSON.stringify(m),
  })

// ---------------------------------------------------------------------------
//  Pesquisa — contrato `docs/reference/pesquisa.md` (ADR-0007) do backend.
//  Ctrl+K (`/api/search`), descrição das listas (`/api/search/schemas`), as
//  listas estilo Odoo (parâmetros uniformes na própria colecção) e os
//  favoritos (`/api/users/me/saved-searches`).
// ---------------------------------------------------------------------------

/** Código estável do envelope de erro plano (ADR-0006 §3): `search.invalid_filter`… */
export function apiErrorCode(e: unknown): string | null {
  if (!(e instanceof ApiError)) return null
  const b = e.body as { code?: unknown } | null
  return b && typeof b === 'object' && typeof b.code === 'string' ? b.code : null
}

export type SearchType =
  | 'meetings'
  | 'recordings'
  | 'people'
  | 'whiteboards'
  | 'rooms'
  | 'messages'
  | 'stream_destinations'
  | 'webhooks'
  | 'audit_events'

/** Ordem fixa dos grupos (a da tabela do contrato). */
export const SEARCH_TYPES: SearchType[] = [
  'meetings',
  'recordings',
  'people',
  'whiteboards',
  'rooms',
  'messages',
  'stream_destinations',
  'webhooks',
  'audit_events',
]

/** Texto partido em segmentos; a UI escapa cada um e realça os `match`. Nunca HTML. */
export interface HighlightSegment {
  text: string
  match: boolean
}

export interface SearchHit {
  type: SearchType
  id: string
  title: string
  subtitle: string | null
  highlight: HighlightSegment[]
  matched_in: string
  score: number
  /** Ids para abrir: `{recording_id, at_secs}`, `{room_code, message_id, created_at}`… */
  target: Record<string, string | number | null>
  href: string
  occurred_at: string | null
}

export interface SearchResultGroup {
  type: SearchType
  count: number
  count_kind: 'exact' | 'at_least'
  more_href: string
  items: SearchHit[]
}

export interface GlobalSearchResult {
  query: string
  took_ms: number
  groups: SearchResultGroup[]
  skipped: { type: SearchType; code: string }[]
}

export const globalSearch = (q: string, opts: { types?: SearchType[]; limit?: number } = {}, signal?: AbortSignal) => {
  const p = new URLSearchParams({ q })
  if (opts.types?.length) p.set('types', opts.types.join(','))
  if (opts.limit) p.set('limit', String(opts.limit))
  return request<GlobalSearchResult>(`/api/search?${p}`, { signal })
}

export type SearchFieldType = 'text' | 'enum' | 'number' | 'datetime' | 'bool' | 'user' | 'ref'
export type SearchOperator =
  | 'eq'
  | 'ne'
  | 'contains'
  | 'not_contains'
  | 'starts_with'
  | 'in'
  | 'not_in'
  | 'is_set'
  | 'is_not_set'
  | 'lt'
  | 'lte'
  | 'gt'
  | 'gte'
  | 'between'
  | 'in_period'
export type DateGranularity = 'day' | 'week' | 'month' | 'quarter' | 'year'

export interface SearchSchemaField {
  name: string
  label: string
  type: SearchFieldType
  operators: SearchOperator[]
  filterable: boolean
  sortable: boolean
  groupable: boolean
  granularities?: DateGranularity[]
  /** `['sum']`, `['sum','avg']`… sobre o conjunto filtrado do grupo. */
  aggregates: string[]
  options?: { value: string; label: string }[]
}

/** Nó do domínio: `[campo, op]`, `[campo, op, valor]`, `{and}`, `{or}`, `{not}`. Lista no topo = E. */
export type DomainNode =
  | [string, SearchOperator]
  | [string, SearchOperator, unknown]
  | { and: DomainNode[] }
  | { or: DomainNode[] }
  | { not: DomainNode }
export type Domain = DomainNode | DomainNode[]

export interface SearchSchema {
  resource: string
  label: string
  /** `/api/recordings`, ou com `{org_id}` quando `org_scoped`. */
  collection: string
  org_scoped: boolean
  timezone: string
  text_search: { fields: string[]; typo_tolerant: boolean }
  fields: SearchSchemaField[]
  filters: { name: string; label: string; group: string; filter: Domain }[]
  group_by: { value: string; label: string }[]
  default_order: string[]
  periods: string[]
}

export const searchSchema = (resource: string, signal?: AbortSignal) =>
  request<SearchSchema>(`/api/search/schemas/${encodeURIComponent(resource)}`, { signal })

export interface ListQuery {
  q?: string
  filter?: Domain | null
  filters?: string[]
  group_by?: string[]
  order_by?: string[]
  page_size?: number
  page_token?: string | null
  groups_page_token?: string | null
}

export interface ListGroup {
  key: string | null
  label: string | null
  count: number
  aggregates: Record<string, Record<string, number>>
  range?: { from: string; to: string }
  /** O nó a JUNTAR ao `filter` corrente para abrir o grupo. */
  filter: Domain
  /** O que falta agrupar dentro deste grupo. */
  group_by: string[]
}

export interface ListEnvelope<T> {
  items: (T & { search?: { score: number; highlight: HighlightSegment[] } })[]
  next_page_token: string | null
  total: number
  total_kind: 'exact' | 'at_least'
  groups?: ListGroup[]
  next_groups_page_token?: string | null
}

/** Caminho da colecção a partir do schema — nunca escrito à mão por ecrã. */
export function collectionPath(schema: Pick<SearchSchema, 'collection' | 'org_scoped'>, orgId?: string | null): string {
  if (!schema.collection.startsWith('/api/')) throw new Error('collection fora de /api')
  if (!schema.org_scoped) return schema.collection
  if (!orgId) throw new Error('org_id em falta')
  return schema.collection.replace('{org_id}', encodeURIComponent(orgId))
}

/** Parâmetros da lista. Leva SEMPRE `page_size`: assim a resposta é o envelope, nunca o array herdado. */
export function listQueryString(query: ListQuery): string {
  const p = new URLSearchParams()
  if (query.q?.trim()) p.set('q', query.q.trim())
  const f = query.filter
  if (f && !(Array.isArray(f) && f.length === 0)) p.set('filter', JSON.stringify(f))
  if (query.filters?.length) p.set('filters', query.filters.join(','))
  if (query.group_by?.length) p.set('group_by', query.group_by.join(','))
  if (query.order_by?.length) p.set('order_by', query.order_by.join(','))
  p.set('page_size', String(query.page_size ?? 50))
  if (query.page_token) p.set('page_token', query.page_token)
  if (query.groups_page_token) p.set('groups_page_token', query.groups_page_token)
  return p.toString()
}

export const searchList = <T>(path: string, query: ListQuery, signal?: AbortSignal) =>
  request<ListEnvelope<T>>(`${path}?${listQueryString(query)}`, { signal })

export interface SavedSearchQuery {
  q?: string
  filter?: Domain | null
  filters?: string[]
  group_by?: string[]
  order_by?: string[]
}

export interface SavedSearch {
  id: string
  resource: string
  name: string
  query: SavedSearchQuery
  shared: boolean
  is_default: boolean
  owner: { id: string; username: string }
  editable: boolean
  valid: boolean
  invalid_code: string | null
  created_at: string
  updated_at: string
}

export const listSavedSearches = (resource: string, signal?: AbortSignal) =>
  request<Page<SavedSearch>>(`/api/users/me/saved-searches?resource=${encodeURIComponent(resource)}&page_size=100`, { signal })

export const createSavedSearch = (body: { resource: string; name: string; query: SavedSearchQuery; shared: boolean; is_default: boolean }) =>
  request<SavedSearch>('/api/users/me/saved-searches', { method: 'POST', body: JSON.stringify(body) })

export const updateSavedSearch = (
  id: string,
  body: Partial<{ name: string; query: SavedSearchQuery; shared: boolean; is_default: boolean }>,
) => request<SavedSearch>(`/api/users/me/saved-searches/${encodeURIComponent(id)}`, { method: 'PATCH', body: JSON.stringify(body) })

export const deleteSavedSearch = (id: string) =>
  request<void>(`/api/users/me/saved-searches/${encodeURIComponent(id)}`, { method: 'DELETE' })
