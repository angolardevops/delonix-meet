// Fidelidade da SALA (ramo frontend/l2-sala): cenário com várias pessoas reais
// contra o servidor de validação, e o ecrã de cada template ao lado da app.
//
//   node fidelidade/sala.mjs --app http://127.0.0.1:5601 [Doc...]
//
// Cada pessoa é um contexto Playwright com câmara e microfone FALSOS
// (--use-fake-device-for-media-stream). Entra-se pela API (um login por conta,
// guardado para a próxima corrida) e a sessão é injectada no localStorage.
// Saída: fidelidade/app/<Doc>.png e fidelidade/lado-a-lado/<Doc>.png.
import { chromium } from '@playwright/test'
import { existsSync, readFileSync, writeFileSync } from 'node:fs'

const DIR = new URL('./', import.meta.url).pathname
const REF = new URL('../../notas-ui-template/ref/', import.meta.url).pathname
const args = process.argv.slice(2)
const appIdx = args.indexOf('--app')
const APP = appIdx >= 0 ? args.splice(appIdx, 2)[1] : 'http://127.0.0.1:5601'
const API = process.env.API ?? 'http://127.0.0.1:8190'
const want = new Set(args.length ? args : ['DelonixPrejoin', 'DelonixRoomGrid', 'DelonixRoomChat', 'DelonixWhiteboard', 'DelonixBoardShared', 'DelonixModeration', 'DelonixMobile'])
const TOKENS = process.env.TOKENS ?? `${DIR}.tokens-sala.json`
const PASS = 'Delonix-UI-2026!'
const PESSOAS = [
  ['demo@delonix.co.ao', 'demo12345'],
  ['joaquim.ferreira@delonix.co.ao', PASS],
  ['teresa.kiala@delonix.co.ao', PASS],
  ['domingos.nzuzi@delonix.co.ao', PASS],
  ['luisa.cardoso@delonix.co.ao', PASS],
  ['paulo.sebastiao@delonix.co.ao', PASS],
]

const log = (...a) => console.log(new Date().toISOString().slice(11, 19), ...a)
const esperar = (ms) => new Promise((r) => setTimeout(r, ms))

async function api(path, { method = 'GET', body, token } = {}) {
  const headers = token ? { Authorization: `Bearer ${token}` } : {}
  if (body) headers['Content-Type'] = 'application/json'
  const r = await fetch(API + path, { method, headers, body: body ? JSON.stringify(body) : undefined })
  const t = await r.text()
  let d = t
  try { d = JSON.parse(t) } catch { /* texto */ }
  return { ok: r.ok, status: r.status, data: d }
}

async function sessoes() {
  let cache = existsSync(TOKENS) ? JSON.parse(readFileSync(TOKENS, 'utf8')) : {}
  const out = []
  for (const [email, pass] of PESSOAS) {
    let s = cache[email]
    // Tokens de acesso duram 15 min: renova-se com margem.
    if (!s || Date.now() - s.at > 10 * 60_000) {
      const r = await api('/api/auth/login', { method: 'POST', body: { email, password: pass } })
      if (!r.ok) throw new Error(`login ${email}: ${r.status} ${JSON.stringify(r.data)}`)
      s = { token: r.data.access_token, user: r.data.user, at: Date.now() }
      cache[email] = s
    }
    out.push(s)
  }
  writeFileSync(TOKENS, JSON.stringify(cache))
  return out
}

const b = await chromium.launch({ args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream', '--autoplay-policy=no-user-gesture-required'] })

async function pagina(s, size = { width: 1440, height: 900 }) {
  const ctx = await b.newContext({ viewport: size, permissions: ['camera', 'microphone'] })
  await ctx.addInitScript((sess) => {
    localStorage.setItem('dx_access', sess.token)
    localStorage.setItem('dx_user', JSON.stringify(sess.user))
    localStorage.setItem('dx_tour_v1', 'done')
  }, s)
  const p = await ctx.newPage()
  p.on('pageerror', (e) => log('pageerror', s.user.username, e.message))
  return p
}

async function fotografar(p, doc) {
  const f = `${DIR}app/${doc}.png`
  await p.screenshot({ path: f })
  const ref = `${REF}${doc}.png`
  if (!existsSync(ref)) return log('só app', doc)
  const img = (x) => 'data:image/png;base64,' + readFileSync(x).toString('base64')
  const vp = p.viewportSize()
  const cmp = await b.newPage({ viewport: { width: 1440 + vp.width + 16, height: Math.max(900, vp.height) + 40 } })
  await cmp.setContent(`<body style="margin:0;background:#222;font:12px monospace;color:#ccc;display:flex;gap:16px;align-items:flex-start">
    <div><div style="height:20px;padding:2px 6px">TEMPLATE · ${doc}</div><img src="${img(ref)}" width="1440" height="900"></div>
    <div><div style="height:20px;padding:2px 6px">APP · ${APP}</div><img src="${img(f)}" width="${vp.width}" height="${vp.height}"></div></body>`)
  await cmp.screenshot({ path: `${DIR}lado-a-lado/${doc}.png` })
  await cmp.close()
  log('ok', doc)
}

const entrar = async (p, { camara = !!process.env.CAM } = {}) => {
  const botao = p.getByRole('button', { name: /^entrar na sess/i })
  await botao.waitFor({ timeout: 30000 })
  // Como no template, as câmaras vêm desligadas (o avatar é o que se compara).
  if (!camara) await p.getByRole('button', { name: /^desligar c[aâ]mara/i }).first().click({ timeout: 8000 }).catch(() => {})
  await botao.click({ timeout: 30000 })
}

// ── Cenário ────────────────────────────────────────────────────────────────
const S = await sessoes()
const [host, ...outros] = S
const sala = await api('/api/rooms', {
  method: 'POST',
  token: host.token,
  body: { name: 'Formação — Arquitectura de Voz Delonix', topology: 'sfu', waiting_room: true, e2ee: false, format: 'training' },
})
if (!sala.ok) throw new Error(`sala: ${sala.status} ${JSON.stringify(sala.data)}`)
const code = sala.data.code
log('sala', code)

// Os convidados chegam primeiro: ficam na sala de espera (a sala tem-na ligada).
const convidados = []
for (const s of outros) {
  const p = await pagina(s, { width: 900, height: 700 })
  await p.goto(`${APP}/#/r/${code}`)
  await entrar(p)
  convidados.push(p)
}
await esperar(2500)

// Anfitrião na pré-entrada: sonda de rede e «N pessoas já na sala de espera».
const hp = await pagina(host)
await hp.goto(`${APP}/#/r/${code}`)
await hp.getByRole('button', { name: /^entrar na sess/i }).waitFor({ timeout: 30000 })
await esperar(5000)
if (want.has('DelonixPrejoin')) await fotografar(hp, 'DelonixPrejoin')

await entrar(hp)
await hp.waitForSelector('.rm-shell', { timeout: 30000 })
await esperar(2500)

// Admite três e deixa dois à espera (Luísa e Paulo).
const cenario = process.env.CENARIO ? await import(process.env.CENARIO) : null
if (cenario) await cenario.default({ hp, convidados, code, S, api, esperar, log, fotografar, want, pagina, APP })

await b.close()
