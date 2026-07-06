export interface User {
  id: string
  email: string
  username: string
}

export interface Room {
  id: string
  code: string
  name: string
  owner_id: string
  topology: string
  waiting_room: boolean
  e2ee: boolean
}

export interface TokenPair {
  access_token: string
  refresh_token: string
  user: User
}

let accessToken: string | null = localStorage.getItem('dx_access')

export function currentUser(): User | null {
  const raw = localStorage.getItem('dx_user')
  return raw ? JSON.parse(raw) : null
}

function saveSession(t: TokenPair) {
  accessToken = t.access_token
  localStorage.setItem('dx_access', t.access_token)
  localStorage.setItem('dx_refresh', t.refresh_token)
  localStorage.setItem('dx_user', JSON.stringify(t.user))
}

export function logout() {
  accessToken = null
  localStorage.removeItem('dx_access')
  localStorage.removeItem('dx_refresh')
  localStorage.removeItem('dx_user')
}

async function request<T>(path: string, options: RequestInit = {}, retry = true): Promise<T> {
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    ...(options.headers as Record<string, string>),
  }
  if (accessToken) headers['Authorization'] = `Bearer ${accessToken}`
  const res = await fetch(path, { ...options, headers })
  if (res.status === 401 && retry && localStorage.getItem('dx_refresh')) {
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
  const res = await fetch('/api/auth/refresh', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ refresh_token: localStorage.getItem('dx_refresh') }),
  })
  if (!res.ok) {
    logout()
    throw new Error('session expired')
  }
  saveSession(await res.json())
}

export async function register(email: string, username: string, password: string): Promise<User> {
  const t = await request<TokenPair>('/api/auth/register', {
    method: 'POST',
    body: JSON.stringify({ email, username, password }),
  })
  saveSession(t)
  return t.user
}

export async function login(email: string, password: string): Promise<User> {
  const t = await request<TokenPair>('/api/auth/login', {
    method: 'POST',
    body: JSON.stringify({ email, password }),
  })
  saveSession(t)
  return t.user
}

export const createRoom = (name: string, topology: 'sfu' | 'mesh' = 'sfu', waitingRoom = false, e2ee = false) =>
  request<Room>('/api/rooms', {
    method: 'POST',
    body: JSON.stringify({ name, topology, waiting_room: waitingRoom, e2ee }),
  })

export const getRoom = (code: string) => request<Room>(`/api/rooms/${code}`)

export const joinRoom = (code: string) =>
  request<{ room: Room; room_token: string }>(`/api/rooms/${code}/join`, { method: 'POST' })

export const iceServers = () => request<RTCConfiguration>('/api/ice')

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
}

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

export const shareRecording = (id: string, userId: string) =>
  request(`/api/recordings/${id}/share`, { method: 'POST', body: JSON.stringify({ user_id: userId }) })

export const listRecordingShares = (id: string) => request<User[]>(`/api/recordings/${id}/share`)

export const unshareRecording = (id: string, userId: string) =>
  request(`/api/recordings/${id}/share/${userId}`, { method: 'DELETE' })

export const listMeetings = () => request<Meeting[]>('/api/meetings')

export const createMeeting = (m: {
  title: string
  description?: string
  kind: 'video' | 'voice'
  starts_at: string
  duration_min: number
  invitee_ids: string[]
  room_ref?: string | null
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
}
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
}
export interface Group {
  id: string
  org_id: string
  name: string
  member_count: number
}

export const myOrgs = () => request<OrgSummary[]>('/api/orgs')
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

export function accessTokenValue(): string | null {
  return accessToken
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

export async function downloadRecording(rec: Recording): Promise<void> {
  const res = await fetch(`/api/recordings/${rec.id}`, { headers: authHeader() })
  if (!res.ok) throw new Error('download failed')
  const url = URL.createObjectURL(await res.blob())
  const a = document.createElement('a')
  a.href = url
  a.download = rec.filename
  a.click()
  URL.revokeObjectURL(url)
}

function authHeader(): Record<string, string> {
  return accessToken ? { Authorization: `Bearer ${accessToken}` } : {}
}
