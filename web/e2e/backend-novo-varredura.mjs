// Varredura: percorre os ecrãs da consola contra o backend novo e regista TODAS
// as respostas ≥ 400 da API e os pedidos a rotas que o backend não tem.
//
// Corre duas vezes: como administrador da organização e como membro. Semeia
// uma reunião, um quadro e uma gravação para as listas não virem vazias.
//
// Uso: API=http://127.0.0.1:8460 APP=http://127.0.0.1:5460 node e2e/backend-novo-varredura.mjs
import { chromium } from '@playwright/test'
import { spawnSync } from 'node:child_process'
import { mkdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { criarConta, entrar, PASSWORD } from './sessao.mjs'

const API = process.env.API ?? 'http://127.0.0.1:8460'
const APP = process.env.APP ?? 'http://127.0.0.1:5460'
// Fora do /tmp (esvaziado a cada arranque) e fora do que o git vê.
const PASTA = process.env.PASTA ?? join(process.cwd(), 'node_modules', '.cache', 'varredura')
mkdirSync(PASTA, { recursive: true })
async function api(path, { token, method = 'GET', body, raw, type } = {}) {
  const r = await fetch(`${API}${path}`, {
    method,
    headers: { 'Content-Type': type ?? 'application/json', ...(token ? { Authorization: `Bearer ${token}` } : {}) },
    body: raw ?? (body ? JSON.stringify(body) : undefined),
  })
  const txt = await r.text()
  let json = null
  try { json = JSON.parse(txt) } catch { /* sem corpo */ }
  return { status: r.status, json, txt }
}

const dono = await criarConta(API, 'varr')
const sA = (await api('/api/auth/login', { method: 'POST', body: { email: dono.email, password: PASSWORD } })).json
const orgId = (await api('/api/orgs', { token: sA.access_token })).json[0].id
const emailM = `membro${Date.now().toString(36)}@${dono.email.split('@')[1]}`
await api(`/api/orgs/${orgId}/members`, { token: sA.access_token, method: 'POST', body: { email: emailM, username: emailM.split('@')[0], password: PASSWORD } })
const sM = (await api('/api/auth/login', { method: 'POST', body: { email: emailM, password: PASSWORD } })).json

// Sementes.
const sala = (await api('/api/rooms', { token: sA.access_token, method: 'POST', body: { name: 'varredura', topology: 'sfu' } })).json
await api('/api/meetings', {
  token: sA.access_token, method: 'POST',
  body: { title: 'Semana', kind: 'video', starts_at: new Date(Date.now() + 3600_000).toISOString(), duration_min: 30, invitee_ids: [sM.user.id] },
})
const png = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=='
await api('/api/whiteboards', { token: sA.access_token, method: 'POST', body: { title: 'Quadro', room_code: sala.code, png_base64: png } })
await api(`/api/rooms/${sala.code}/join`, { token: sA.access_token, method: 'POST' }) // só quem participou carrega gravações
const webm = join(PASTA, 'g.webm')
spawnSync('ffmpeg', ['-v', 'error', '-y', '-f', 'lavfi', '-i', 'testsrc2=size=320x180:rate=15', '-t', '2', '-c:v', 'libvpx', webm])
const grav = await api(`/api/rooms/${sala.code}/recordings?name=varredura.webm`, { token: sA.access_token, method: 'POST', raw: readFileSync(webm), type: 'video/webm' })
console.log(`· sementes: sala ${sala.code}, gravação ${grav.status} ${grav.json?.id ?? grav.txt.slice(0, 80)}`)

const ECRAS = [
  '#/', '#/calendar', '#/calendar/new', '#/studio', '#/recordings', grav.json?.id ? `#/recordings/${grav.json.id}` : null,
  '#/whiteboards', '#/whiteboards/diagram', '#/directory', '#/integrations', '#/analytics', '#/admin', '#/ai',
  `#/r/${sala.code}`, `#/lobby/${sala.code}`, '#/status', '#/api-docs', '#/legal',
].filter(Boolean)

const browser = await chromium.launch({ args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'] })
const resultado = {}
for (const [quem, conta] of [['admin da org', dono], ['membro', { email: emailM, password: PASSWORD }]]) {
  const ctx = await browser.newContext({ permissions: ['camera', 'microphone'], viewport: { width: 1440, height: 900 }, locale: 'pt-PT' })
  const page = await ctx.newPage()
  let ecra = 'login'
  const vistos = []
  page.on('response', (r) => {
    const u = new URL(r.url())
    if (u.pathname.startsWith('/api/') && r.status() >= 400) {
      const caminho = u.pathname.replace(/[0-9a-f]{8}-[0-9a-f-]{27}/g, '{id}').replace(sala.code, '{code}')
      vistos.push(`${r.status()} ${r.request().method()} ${caminho}  ← ${ecra}`)
    }
  })
  page.on('pageerror', (e) => vistos.push(`ERRO DE PÁGINA ${e.message.slice(0, 120)}  ← ${ecra}`))
  await entrar(page, APP, conta)
  for (const h of ECRAS) {
    ecra = h
    await page.goto(`${APP}/${h}`, { waitUntil: 'domcontentloaded' })
    await page.waitForTimeout(h.startsWith('#/r/') || h.startsWith('#/studio') ? 6000 : 3500)
  }
  resultado[quem] = [...new Set(vistos)].sort()
  await ctx.close()
}
await browser.close()
for (const [quem, l] of Object.entries(resultado)) {
  console.log(`\n· ${quem}: ${l.length} respostas ≥ 400 distintas`)
  for (const x of l) console.log(`    ${x}`)
}
