// Dados de demonstração para as páginas da consola (lote 2, frontend/l2-consola),
// SÓ pelas APIs reais do servidor de validação: reuniões com tipo de sessão,
// sala de espera, gravação automática e qualidade; uma reunião vinda do Odoo
// (`/api/v1/meetings` com `external_ref`, como o módulo nk_delonix_meet faz);
// uma resposta «tentativo»; e destinos de emissão guardados da organização.
// Idempotente pelo título / rótulo.
//
//   API=http://127.0.0.1:8190 node fidelidade/seed-consola.mjs
const API = process.env.API ?? 'http://127.0.0.1:8190'

async function call(path, { method = 'GET', body, token } = {}) {
  const headers = token ? { Authorization: `Bearer ${token}` } : {}
  if (body) headers['Content-Type'] = 'application/json'
  const r = await fetch(API + path, { method, headers, body: body ? JSON.stringify(body) : undefined })
  const t = await r.text()
  let d = null
  try { d = JSON.parse(t) } catch { d = t }
  return { ok: r.ok, status: r.status, data: d }
}
async function login(email, password) {
  const r = await call('/api/auth/login', { method: 'POST', body: { email, password } })
  if (!r.ok) throw new Error(`login ${email}: ${r.status}`)
  return r.data.access_token
}
const demo = await login('demo@delonix.co.ao', 'demo12345')
const teresa = await login('teresa.kiala@delonix.co.ao', 'Delonix-UI-2026!')
const org = (await call('/api/orgs', { token: demo })).data[0]
const uid = async (q) => (await call(`/api/users/search?q=${encodeURIComponent(q)}`, { token: demo })).data[0]?.id
const ids = {
  teresa: await uid('teresa'), joaquim: await uid('joaquim'), domingos: await uid('domingos'),
  luisa: await uid('luísa'), paulo: await uid('paulo'), ana: await uid('ana.mbala'),
}

// Horas em WAT (UTC+1): amanhã e depois de amanhã, a partir de agora.
const dia = (n, h, m) => {
  const d = new Date()
  d.setUTCDate(d.getUTCDate() + n)
  d.setUTCHours(h - 1, m, 0, 0)
  return d.toISOString()
}
const daquiA = (min) => new Date(Math.ceil((Date.now() + min * 60e3) / 300e3) * 300e3).toISOString()

const existentes = new Set(((await call('/api/meetings', { token: demo })).data ?? []).map((m) => m.title))
const REUNIOES = [
  { title: 'Formação — Arquitectura de Voz · sessão 4', starts_at: daquiA(10), duration_min: 90,
    format: 'hybrid', auto_record: true, record_quality: '2160p', waiting_room: false,
    invitee_ids: [ids.joaquim, ids.teresa, ids.domingos, ids.luisa, ids.paulo] },
  { title: 'Revisão de acessos — TI', starts_at: dia(1, 8, 30), duration_min: 30,
    format: 'meeting', waiting_room: true, auto_record: false, record_quality: '1080p',
    invitee_ids: [ids.teresa, ids.joaquim] },
  { title: 'Aula aberta — Portabilidade +244', starts_at: dia(1, 9, 0), duration_min: 60,
    format: 'broadcast', auto_record: true, record_quality: '1080p', waiting_room: false,
    invitee_ids: [ids.teresa] },
  { title: 'Onboarding — Ferramentas da equipa', starts_at: dia(2, 14, 30), duration_min: 60,
    format: 'training', auto_record: true, record_quality: '720p', waiting_room: true,
    invitee_ids: [ids.luisa, ids.paulo] },
]
for (const m of REUNIOES) {
  if (existentes.has(m.title)) { console.log('já existe', m.title); continue }
  const r = await call('/api/meetings', { method: 'POST', token: demo, body: { kind: 'video', ...m, invitee_ids: m.invitee_ids.filter(Boolean) } })
  console.log('reunião', r.status, m.title)
  if (m.title.startsWith('Revisão de acessos') && r.ok) {
    const t = await call(`/api/meetings/${r.data.id}/respond`, { method: 'POST', token: teresa, body: { status: 'tentative', reason: '' } })
    console.log('  teresa → tentativo', t.status)
  }
}

// Reunião criada pela integração Odoo (superfície v1 com chave de API).
const ODOO = 'Comité de direcção (Odoo)'
if (!existentes.has(ODOO)) {
  const chave = await call(`/api/orgs/${org.id}/api-keys`, { method: 'POST', token: demo, body: { name: 'seed-consola-odoo' } })
  const v1 = await call('/api/v1/meetings', {
    method: 'POST', token: chave.data?.key,
    body: { title: ODOO, starts_at: dia(1, 11, 30), duration_min: 45, host_email: 'demo@delonix.co.ao',
      external_ref: 'odoo:delonix_prod:calendar.event:4821', invitees: [{ email: 'teresa.kiala@delonix.co.ao' }] },
  })
  console.log('reunião odoo', v1.status)
} else console.log('já existe', ODOO)

// Destinos de emissão guardados (a chave fica cifrada no servidor; volta só `key_set`).
const dest = (await call(`/api/orgs/${org.id}/stream-destinations`, { token: demo })).data ?? []
const rotulos = new Set(Array.isArray(dest) ? dest.map((d) => d.label) : [])
const DESTINOS = [
  { label: 'YouTube · Delonix Angola', rtmp_url: 'rtmp://a.rtmp.youtube.com/live2', stream_key: 'demo-yt-0001' },
  { label: 'Facebook · Página', rtmp_url: 'rtmps://live-api-s.facebook.com:443/rtmp', stream_key: 'demo-fb-0001' },
  { label: 'LinkedIn · Empresa', platform: 'linkedin', rtmp_url: 'rtmps://1-live.linkedin.com/live', stream_key: 'demo-li-0001' },
  { label: 'Servidor interno · RTMP', platform: 'rtmp', rtmp_url: 'rtmp://127.0.0.1:1935/live', stream_key: 'demo-srv-0001' },
]
for (const d of DESTINOS) {
  if (rotulos.has(d.label)) { console.log('já existe', d.label); continue }
  const r = await call(`/api/orgs/${org.id}/stream-destinations`, { method: 'POST', token: demo, body: d })
  console.log('destino', r.status, d.label, r.ok ? '' : JSON.stringify(r.data))
}
