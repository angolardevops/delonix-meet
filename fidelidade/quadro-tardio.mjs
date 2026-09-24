// Defeito reportado: quem entra/abre o quadro DEPOIS não vê os traços anteriores,
// e o cursor de quem desenha não aparece ao anfitrião.
//   node fidelidade/quadro-tardio.mjs [--app http://127.0.0.1:5601]
// A (anfitrião, Demo) abre o quadro e desenha N traços; B (Joaquim) entra
// depois; conta-se o que B tem no quadro (objectos no DOM de teste e píxeis
// pintados no canvas). Depois B move o rato no quadro e verifica-se o cursor
// com o nome em A, e vice-versa.
import { chromium } from '@playwright/test'
import { existsSync, readFileSync, writeFileSync } from 'node:fs'
const DIR = new URL('./', import.meta.url).pathname
const args = process.argv.slice(2)
const appIdx = args.indexOf('--app')
const APP = appIdx >= 0 ? args.splice(appIdx, 2)[1] : 'http://127.0.0.1:5601'
const API = process.env.API ?? 'http://127.0.0.1:8190'
const N = Number(process.env.N ?? 6)
const TOKENS = process.env.TOKENS ?? `${DIR}.tokens-sala.json`
const log = (...a) => console.log(new Date().toISOString().slice(11, 19), ...a)
const esperar = (ms) => new Promise((r) => setTimeout(r, ms))
let falhas = 0
const ok = (c, msg, extra = '') => {
  if (!c) falhas++
  log(c ? 'OK  ' : 'FALHA', msg, extra)
}

async function api(path, { method = 'GET', body, token } = {}) {
  const headers = token ? { Authorization: `Bearer ${token}` } : {}
  if (body) headers['Content-Type'] = 'application/json'
  const r = await fetch(API + path, { method, headers, body: body ? JSON.stringify(body) : undefined })
  return { ok: r.ok, status: r.status, data: await r.json().catch(() => null) }
}
async function sessao(email, pass) {
  const cache = existsSync(TOKENS) ? JSON.parse(readFileSync(TOKENS, 'utf8')) : {}
  let s = cache[email]
  if (!s || Date.now() - s.at > 10 * 60_000) {
    const r = await api('/api/auth/login', { method: 'POST', body: { email, password: pass } })
    if (!r.ok) throw new Error(`login ${email}: ${r.status}`)
    s = { token: r.data.access_token, user: r.data.user, at: Date.now() }
    cache[email] = s
    writeFileSync(TOKENS, JSON.stringify(cache))
  }
  return s
}

const b = await chromium.launch({ args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'] })
async function pagina(s) {
  const ctx = await b.newContext({ viewport: { width: 1440, height: 900 }, permissions: ['camera', 'microphone'] })
  await ctx.addInitScript((sess) => {
    localStorage.setItem('dx_access', sess.token)
    localStorage.setItem('dx_user', JSON.stringify(sess.user))
    localStorage.setItem('dx_tour_v1', 'done')
  }, s)
  const p = await ctx.newPage()
  p.on('pageerror', (e) => log('pageerror', s.user.username, e.message))
  return p
}
async function entrar(p, code, anfitriao = null) {
  await p.goto(`${APP}/#/r/${code}`)
  const botao = p.getByRole('button', { name: /^entrar na sess/i })
  await botao.waitFor({ timeout: 30000 })
  await p.getByRole('button', { name: /^desligar c[aâ]mara/i }).first().click({ timeout: 8000 }).catch(() => {})
  await botao.click()
  await p.locator('.rm-shell').waitFor({ timeout: 30000 })
  // Sem entrada directa, fica à porta: o anfitrião admite (o aviso fica por cima do quadro).
  if (anfitriao) await anfitriao.locator('.rm-admit-accept').first().click({ timeout: 15000 }).catch(() => {})
  await p.waitForFunction(() => !/À espera que o anfitrião/i.test(document.body.innerText || ''), null, { timeout: 30000 }).catch(() => {})
  await esperar(2500)
}
/** Píxeis não brancos no canvas do quadro — o que a pessoa VÊ pintado. */
const pintados = (p) =>
  p.evaluate(() => {
    const c = document.querySelector('.rm-wb__sheet canvas')
    if (!c || !c.width) return -1
    const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data
    let n = 0
    for (let i = 3; i < d.length; i += 4) if (d[i] > 0) n++
    return n
  })
const objectos = (p) => p.evaluate(() => Number(document.querySelector('.rm-wb')?.getAttribute('data-objects') ?? -1))

const A = await sessao('demo@delonix.co.ao', 'demo12345')
const B = await sessao('joaquim.ferreira@delonix.co.ao', 'Delonix-UI-2026!')
const sala = await api('/api/rooms', { method: 'POST', token: A.token, body: { name: 'Quadro tardio', topology: 'sfu', waiting_room: false, e2ee: false, format: 'training' } })
const code = sala.data.code
log('sala', code)

const pa = await pagina(A)
await entrar(pa, code)
await pa.getByRole('button', { name: /^quadro branco$/i }).first().click()
await pa.locator('.rm-wb__sheet canvas').first().waitFor({ timeout: 10000 })
await esperar(800)
const box = await pa.locator('.rm-wb__live').boundingBox()
for (let i = 0; i < N; i++) {
  const y = box.y + 60 + i * 40
  await pa.mouse.move(box.x + 80, y)
  await pa.mouse.down()
  for (let k = 1; k <= 10; k++) await pa.mouse.move(box.x + 80 + k * 30, y + (k % 2) * 12)
  await pa.mouse.up()
  await esperar(150)
}
await esperar(800)
const pixA = await pintados(pa)
log('A desenhou', N, 'traços; píxeis pintados em A:', pixA, 'objectos:', await objectos(pa))

// B entra DEPOIS dos traços.
const pb = await pagina(B)
const recebidas = []
pb.on('console', (m) => {
  const t = m.text()
  if (t.startsWith('[signal] <-')) recebidas.push(t.slice(12))
})
await entrar(pb, code, pa)
// O quadro de B abre com o wb-open de A (A fecha e volta a abrir, como na sessão real).
await pa.getByRole('button', { name: /^quadro branco$/i }).first().click()
await esperar(600)
await pa.getByRole('button', { name: /^quadro branco$/i }).first().click()
await pb.locator('.rm-wb__sheet canvas').first().waitFor({ timeout: 15000 }).catch(() => {})
await esperar(1500)
const pixB = await pintados(pb)
const objB = await objectos(pb)
log('B: píxeis pintados', pixB, 'objectos', objB)
log('B recebeu:', recebidas.join(' | '))
ok(pixB > pixA * 0.5, `B vê os ${N} traços anteriores ao abrir o quadro`, `(A ${pixA} px, B ${pixB} px)`)

// Cursores: B move o rato no quadro → A vê o nome de B; e o inverso.
const boxB = await pb.locator('.rm-wb__live').boundingBox().catch(() => null)
if (boxB) for (let k = 0; k < 12; k++) {
  await pb.mouse.move(boxB.x + 200 + k * 10, boxB.y + 200)
  await esperar(60)
}
const cursorEmA = await pa.locator('.rm-wbcursor', { hasText: 'Joaquim' }).first().isVisible({ timeout: 3000 }).catch(() => false)
ok(cursorEmA, 'o anfitrião (A) vê o cursor de B com o nome')
for (let k = 0; k < 12; k++) {
  await pa.mouse.move(box.x + 300 + k * 10, box.y + 300)
  await esperar(60)
}
const cursorEmB = await pb.locator('.rm-wbcursor', { hasText: 'Demo' }).first().isVisible({ timeout: 3000 }).catch(() => false)
ok(cursorEmB, 'B vê o cursor do anfitrião com o nome')

await pa.screenshot({ path: `${DIR}app/quadro-tardio-A.png` })
await pb.screenshot({ path: `${DIR}app/quadro-tardio-B.png` })
await b.close()
log(falhas ? `${falhas} falha(s)` : 'tudo verde')
process.exit(falhas ? 1 : 0)
