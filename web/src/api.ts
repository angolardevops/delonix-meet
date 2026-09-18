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
  request<{ ok: boolean }>('/api/users/me/mfa/disable', {
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

export const updateEmployee = (orgId: string, userId: string, data: { role?: string; title?: string; branch_id?: string | null }) =>
  request<Employee>(`/api/orgs/${orgId}/members/${userId}`, { method: 'PATCH', body: JSON.stringify(data) })

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

/**
 * Quarentena POR ORGANIZAÇÃO. O `org_id` era um parâmetro de query opcional e
 * passou a fazer parte do caminho: o servidor responde por UMA organização de
 * cada vez (`quarantine_analytics` fixa `admin_orgs = vec![org_id]`), e o
 * âmbito «todas as que administro» deixou de existir do lado de lá.
 */
export const quarantineAnalytics = (period: 'week' | 'month' | 'quarter' | 'year', orgId: string) =>
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
  request<{ ok: boolean }>(`/api/orgs/${orgId}/integrations/odoo`, {
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
  request<{ ok: boolean }>('/api/operator/v1/storage', {
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

/** O servidor responde `204` sem corpo — daí o `requestEmpty`. */
export async function deleteActionItem(meetingId: string, itemId: string): Promise<void> {
  await requestEmpty(`/api/meetings/${meetingId}/action-plan/items/${itemId}`, { method: 'DELETE' })
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
 * Destino de directo GUARDADO pela organização (`server/src/stream_destinations.rs`).
 *
 * A chave de emissão NÃO vem aqui, nem cifrada: depois de guardada só volta
 * `key_set: true`. Para emitir com ele, passa `{ id }` no `Destino` do
 * `studio/directo.ts` — o servidor decifra a chave do lado dele.
 */
export type StreamPlatform = 'youtube' | 'facebook' | 'linkedin' | 'twitch' | 'rtmp'

export interface StreamDestination {
  id: string
  org_id: string
  label: string
  platform: StreamPlatform
  /** URL base, sem a chave. */
  rtmp_url: string
  key_set: true
  created_by: string | null
  created_at: string
  updated_at: string
  last_used_at: string | null
  /** Como acabou a última emissão que o usou: `ok` (esteve no ar) ou `erro`. */
  last_status: 'ok' | 'erro' | null
  last_error: string | null
}

export interface StreamDestinationCreate {
  label: string
  /** Omitido: o servidor deriva do host do URL. */
  platform?: StreamPlatform
  rtmp_url: string
  stream_key: string
}

/** Omitir um campo mantém-no; `stream_key` presente SUBSTITUI a chave guardada. */
export type StreamDestinationPatch = Partial<StreamDestinationCreate>

/** Lista (membro da organização). No máximo 50 por organização. */
export const listStreamDestinations = (orgId: string, signal?: AbortSignal) =>
  request<StreamDestination[]>(`/api/orgs/${orgId}/stream-destinations`, { signal })

export const getStreamDestination = (orgId: string, id: string, signal?: AbortSignal) =>
  request<StreamDestination>(`/api/orgs/${orgId}/stream-destinations/${id}`, { signal })

/** Cria (administrador). `409` = nome repetido ou tecto; `503` = servidor sem `SECRETS_KEY`. */
export const createStreamDestination = (orgId: string, body: StreamDestinationCreate) =>
  request<StreamDestination>(`/api/orgs/${orgId}/stream-destinations`, {
    method: 'POST',
    body: JSON.stringify(body),
  })

/** Altera (administrador). */
export const updateStreamDestination = (orgId: string, id: string, patch: StreamDestinationPatch) =>
  request<StreamDestination>(`/api/orgs/${orgId}/stream-destinations/${id}`, {
    method: 'PATCH',
    body: JSON.stringify(patch),
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
 * `GET /api/orgs/{org}/audit-events/verification` — recalcula a cadeia de
 * hashes do registo de auditoria (migração 0037). Só admins. `intact: false`
 * diz em que registo a cadeia partiu.
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

// As salas de voz deixaram de viver em `/api/voice/rooms` e passaram a ser um
// sub-recurso da organização: o `orgId` é agora parte do caminho.
export const createVoiceRoom = (orgId: string, roomCode: string, didId?: string) =>
  request<VoiceRoomCreated>(`/api/orgs/${orgId}/voice/rooms`, {
    method: 'POST',
    body: JSON.stringify({ room_code: roomCode, ...(didId ? { did_id: didId } : {}) }),
  })
export const getVoiceRoom = (orgId: string, id: string) =>
  request<VoiceRoom>(`/api/orgs/${orgId}/voice/rooms/${id}`)
export const voiceRoomParticipants = (orgId: string, id: string) =>
  request<VoiceParticipant[]>(`/api/orgs/${orgId}/voice/rooms/${id}/participants`)
/** Encerrar responde `204` sem corpo. */
export const closeVoiceRoom = (orgId: string, id: string) =>
  requestEmpty(`/api/orgs/${orgId}/voice/rooms/${id}/close`, { method: 'POST' })
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

/** Tecto de upload de uma gravação no servidor (recordings.rs MAX_RECORDING_BYTES). */
export const MAX_RECORDING_UPLOAD_BYTES = 512 * 1024 * 1024
