// Agenda, acta e administração de pessoas pela interface, contra o backend novo (#90/#92).
//
//   1. Acta: o anfitrião sai da reunião e a acta fica gravada (`PUT /api/rooms/{code}/minutes`).
//   2. Agenda: o convidado responde (`PUT …/invitees/me`), o dono acrescenta e
//      remove pontos (`agenda-items`), mexe no plano de acção
//      (`action-plan/items/{id}` por reunião) e descarrega o `.ics`
//      (`calendar.ics`) — e o ficheiro é iCalendar, não JSON.
//   3. Pessoas: adicionar, mudar o papel e remover (`/members`, DELETE → 204).
//
// Uso: API=http://127.0.0.1:8460 APP=http://127.0.0.1:5460 node e2e/backend-novo-agenda.mjs
import { chromium } from '@playwright/test'
import { readFileSync } from 'node:fs'
import { criarConta, entrar, PASSWORD } from './sessao.mjs'

const API = process.env.API ?? 'http://127.0.0.1:8460'
const APP = process.env.APP ?? 'http://127.0.0.1:5460'
const WS = API.replace(/^http/, 'ws')
let falhas = 0
const ok = (c, n, d) => {
  console.log(`  ${c ? '✓' : '✗'} ${n}${d ? `  — ${d}` : ''}`)
  if (!c) falhas++
}
async function api(path, { token, method = 'GET', body } = {}) {
  const r = await fetch(`${API}${path}`, {
    method,
    headers: { 'Content-Type': 'application/json', ...(token ? { Authorization: `Bearer ${token}` } : {}) },
    body: body ? JSON.stringify(body) : undefined,
  })
  const txt = await r.text()
  let json = null
  try { json = JSON.parse(txt) } catch { /* sem corpo */ }
  return { status: r.status, json, txt }
}

const dono = await criarConta(API, 'agd')
const login = async (email) => (await api('/api/auth/login', { method: 'POST', body: { email, password: PASSWORD } })).json
const sA = await login(dono.email)
const tA = sA.access_token
const orgId = (await api('/api/orgs', { token: tA })).json[0].id
const dominio = dono.email.split('@')[1]
const emailB = `conv${Date.now().toString(36)}@${dominio}`
await api(`/api/orgs/${orgId}/members`, { token: tA, method: 'POST', body: { email: emailB, username: emailB.split('@')[0], password: PASSWORD } })
const sB = await login(emailB)

const browser = await chromium.launch({
  args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'],
})
const erros = []
async function pagina(nome, conta) {
  const ctx = await browser.newContext({ permissions: ['camera', 'microphone'], viewport: { width: 1440, height: 900 }, locale: 'pt-PT', acceptDownloads: true })
  const page = await ctx.newPage()
  const pedidos = []
  page.on('response', (r) => {
    const u = new URL(r.url())
    if (!u.pathname.startsWith('/api/')) return
    pedidos.push(`${r.request().method()} ${u.pathname} ${r.status()}`)
    if (r.status() >= 400) erros.push(`${nome} ${r.request().method()} ${u.pathname} → ${r.status()}`)
  })
  await entrar(page, APP, conta)
  return { nome, page, pedidos }
}
const viu = (p, re) => p.pedidos.find((x) => re.test(x))

// ---------- 1. acta ao sair ----------
console.log('· acta ao sair')
const inicio = new Date(Date.now() + 60_000).toISOString()
const reuniao = (await api('/api/meetings', {
  token: tA, method: 'POST',
  body: { title: 'Reunião com acta', description: 'prova', kind: 'video', starts_at: inicio, duration_min: 30, invitee_ids: [sB.user.id] },
})).json
const arranque = (await api(`/api/meetings/${reuniao.id}/start`, { token: tA, method: 'POST' })).json
const code = arranque.code
const A = await pagina('A', dono)
await A.page.goto(`${APP}/#/r/${code}`, { waitUntil: 'domcontentloaded' })
await A.page.getByRole('button', { name: /entrar na sessão/i }).first().click({ timeout: 60000 })
await A.page.waitForFunction(() => !document.querySelector('.rm-prejoin'), null, { timeout: 90000 })
// B (convidado, entrada directa) fala: frases finais pelo /ws, como o transcritor dele faria.
const joinB = (await api(`/api/rooms/${code}/join`, { token: sB.access_token, method: 'POST' })).json
const wsB = new WebSocket(`${WS}/ws?token=${joinB.room_token}&room=${code}`)
await new Promise((r) => (wsB.onopen = r))
await A.page.waitForTimeout(1500)
const frase = `decidimos lançar na sexta ${Date.now().toString(36)}`
wsB.send(JSON.stringify({ type: 'transcript', text: frase }))
wsB.send(JSON.stringify({ type: 'transcript', text: 'a Ana trata do orçamento' }))
await A.page.waitForTimeout(1500)
const put = A.page.waitForResponse((r) => r.url().includes(`/api/rooms/${code}/minutes`) && r.request().method() === 'PUT', { timeout: 30000 }).catch(() => null)
await A.page.getByRole('button', { name: /sair da chamada/i }).first().click()
const resp = await put
ok(resp?.status() === 204, 'sair grava a acta com PUT → 204', resp ? `HTTP ${resp.status()}` : 'nenhum PUT')
const notas = await api(`/api/rooms/${code}/minutes`, { token: tA })
ok(notas.status === 200 && JSON.stringify(notas.json).includes(frase), 'a acta lida da API tem o que se disse', `HTTP ${notas.status} ${notas.txt.slice(0, 160)}`)
wsB.close()

// ---------- 2. agenda: RSVP, pontos, plano de acção, .ics ----------
console.log('· agenda')
const r2 = (await api('/api/meetings', {
  token: tA, method: 'POST',
  body: { title: 'Planeamento', kind: 'video', starts_at: new Date(Date.now() + 86400_000).toISOString(), duration_min: 45, invitee_ids: [sB.user.id] },
})).json
const B = await pagina('B', { email: emailB, password: PASSWORD })
await B.page.goto(`${APP}/#/calendar/m/${r2.id}`, { waitUntil: 'domcontentloaded' })
await B.page.getByRole('button', { name: /^aceitar$/i }).first().click({ timeout: 30000 })
await B.page.waitForTimeout(1500)
ok(!!viu(B, /^PUT \/api\/meetings\/[^/]+\/invitees\/me 200$/), 'o convidado aceita pela UI → PUT invitees/me 200', viu(B, /invitees\/me/))
const convidados = (await api(`/api/meetings/${r2.id}/invitees`, { token: tA })).json
ok(convidados.find((c) => c.user_id === sB.user.id)?.status === 'accepted', 'o dono vê o convite aceite')

await A.page.goto(`${APP}/#/calendar/m/${r2.id}`, { waitUntil: 'domcontentloaded' })
await A.page.getByRole('tab', { name: /pontos da agenda/i }).click({ timeout: 30000 })
await A.page.getByLabel('Novo ponto').fill('Orçamento do trimestre')
await A.page.getByRole('button', { name: /^adicionar$/i }).first().click()
await A.page.getByText('Orçamento do trimestre').first().waitFor({ timeout: 15000 })
ok(!!viu(A, /^POST \/api\/meetings\/[^/]+\/agenda-items 200$/), 'acrescentar ponto → POST agenda-items 200')
await A.page.getByRole('button', { name: 'Remover «Orçamento do trimestre»' }).click()
await A.page.waitForTimeout(1500)
ok(!!viu(A, /^DELETE \/api\/meetings\/[^/]+\/agenda-items\/[^/]+ 204$/), 'remover ponto → DELETE agenda-items 204')

await A.page.getByRole('tab', { name: /plano de acção/i }).click()
await A.page.getByLabel('O quê').fill('Enviar proposta')
await A.page.getByRole('button', { name: /^adicionar$/i }).first().click()
await A.page.getByText('Enviar proposta').first().waitFor({ timeout: 15000 })
await A.page.getByTitle(/mudar para «em curso»/i).first().click()
await A.page.waitForTimeout(1500)
ok(!!viu(A, /^PATCH \/api\/meetings\/[^/]+\/action-plan\/items\/[^/]+ 200$/), 'mudar estado → PATCH action-plan/items/{id} 200')
await A.page.getByRole('button', { name: 'Remover «Enviar proposta»' }).click()
await A.page.waitForTimeout(1500)
ok(!!viu(A, /^DELETE \/api\/meetings\/[^/]+\/action-plan\/items\/[^/]+ 204$/), 'remover acção → DELETE action-plan/items/{id} 204')

const [download] = await Promise.all([
  A.page.waitForEvent('download', { timeout: 20000 }).catch(() => null),
  A.page.getByRole('button', { name: /adicionar ao calendário/i }).click(),
])
const ics = download ? readFileSync(await download.path(), 'utf8') : ''
ok(ics.startsWith('BEGIN:VCALENDAR') && ics.includes('Planeamento'), '«Adicionar ao calendário» descarrega iCalendar', download ? `${download.suggestedFilename()} ${ics.slice(0, 40).replace(/\r?\n/g, ' ')}` : 'sem download')

// ---------- 3. pessoas: adicionar, papel, remover ----------
console.log('· pessoas')
const novo = `nova${Date.now().toString(36)}`
await A.page.goto(`${APP}/#/admin`, { waitUntil: 'domcontentloaded' })
await A.page.getByRole('button', { name: 'Adicionar pessoa' }).click({ timeout: 30000 })
await A.page.fill('#org-add-email', `${novo}@${dominio}`)
await A.page.fill('#org-add-name', novo)
await A.page.fill('#org-add-pw', PASSWORD)
await A.page.getByRole('button', { name: 'Adicionar pessoa' }).last().click()
await A.page.getByText(novo).first().waitFor({ timeout: 15000 })
ok(!!viu(A, /^POST \/api\/orgs\/[^/]+\/members 200$/), 'adicionar → POST members 200')
await A.page.getByRole('button', { name: `Editar ${novo}` }).click()
await A.page.selectOption('#org-edit-role', 'admin')
await A.page.locator('.dx-dialog button[type=submit]', { hasText: 'Guardar' }).click()
await A.page.waitForTimeout(1500)
ok(!!viu(A, /^PATCH \/api\/orgs\/[^/]+\/members\/[^/]+ 200$/), 'mudar papel → PATCH members/{id} 200')
const lista = (await api(`/api/orgs/${orgId}/members`, { token: tA })).json
ok(lista.find((m) => m.username === novo)?.role === 'admin', 'a API confirma o papel novo')
await A.page.getByRole('button', { name: `Remover ${novo}` }).click()
await A.page.getByRole('button', { name: 'Remover da organização' }).click()
await A.page.waitForTimeout(1500)
ok(!!viu(A, /^DELETE \/api\/orgs\/[^/]+\/members\/[^/]+ 204$/), 'remover → DELETE members/{id} 204')
const lista2 = (await api(`/api/orgs/${orgId}/members`, { token: tA })).json
ok(!lista2.some((m) => m.username === novo), 'a pessoa sai da lista')
ok((await A.page.locator('.dx-alert, [role=alert]').filter({ hasText: /não foi possível|erro/i }).count()) === 0, 'sem mensagem de erro no ecrã')

console.log('\n· respostas ≥ 400 da API vistas pelos browsers:')
for (const e of [...new Set(erros)]) console.log(`    ${e}`)
await browser.close()
console.log(falhas ? `\n=== ${falhas} FALHARAM ===` : '\n=== TUDO VERDE ===')
process.exit(falhas ? 1 : 0)
