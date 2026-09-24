// Prova da conversa directa (pedido do dono do produto): A manda uma privada a
// B, C não a vê; B responde em privado; recarregar mantém-na para A e B e não
// para C (histórico filtrado no servidor).
//   API=http://127.0.0.1:8211 node fidelidade/chat-privado.mjs --app http://127.0.0.1:5602
import { chromium } from '@playwright/test'
import { existsSync, readFileSync, writeFileSync } from 'node:fs'
const DIR = new URL('./', import.meta.url).pathname
const args = process.argv.slice(2)
const appIdx = args.indexOf('--app')
const APP = appIdx >= 0 ? args.splice(appIdx, 2)[1] : 'http://127.0.0.1:5602'
const API = process.env.API ?? 'http://127.0.0.1:8211'
const TOKENS = process.env.TOKENS ?? `${DIR}.tokens-8211.json`
const log = (...a) => console.log(new Date().toISOString().slice(11, 19), ...a)
const esperar = (ms) => new Promise((r) => setTimeout(r, ms))
let falhas = 0
const ok = (c, msg) => {
  if (!c) falhas++
  log(c ? 'OK  ' : 'FALHA', msg)
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
async function entrar(p, code, anfitriao = null, recarregar = false) {
  if (recarregar) await p.reload()
  else await p.goto(`${APP}/#/r/${code}`)
  // Uma reentrada recente (< 60 s) salta a pré-entrada: entra-se directo.
  const botao = p.getByRole('button', { name: /^entrar na sess/i })
  const temBotao = await botao.waitFor({ timeout: recarregar ? 6000 : 30000 }).then(() => true).catch(() => false)
  if (temBotao) {
    await p.getByRole('button', { name: /^desligar c[aâ]mara/i }).first().click({ timeout: 8000 }).catch(() => {})
    await botao.click()
  }
  await p.locator('.rm-shell').waitFor({ timeout: 30000 })
  if (anfitriao) await anfitriao.locator('.rm-admit-accept').first().click({ timeout: 15000 }).catch(() => {})
  await p.waitForFunction(() => !/À espera que o anfitrião/i.test(document.body.innerText || ''), null, { timeout: 30000 }).catch(() => {})
  await esperar(1500)
}
const abrirChat = async (p) => {
  if (await p.locator('.rm-chat').count()) return
  await p.getByRole('button', { name: /^chat$/i }).first().click({ timeout: 8000 })
  await p.locator('.rm-chat').waitFor({ timeout: 8000 })
}
const textoDoChat = (p) => p.locator('.rm-chat__list').innerText().catch(() => '')

const A = await sessao('demo@delonix.co.ao', 'demo12345')
const B = await sessao('joaquim.ferreira@delonix.co.ao', 'Delonix-UI-2026!')
const C = await sessao('teresa.kiala@delonix.co.ao', 'Delonix-UI-2026!')
const sala = await api('/api/rooms', { method: 'POST', token: A.token, body: { name: 'Conversa directa', topology: 'sfu', waiting_room: false, e2ee: false, format: 'training' } })
const code = sala.data.code
log('sala', code)
const pa = await pagina(A)
await entrar(pa, code)
const pb = await pagina(B)
await entrar(pb, code, pa)
const pc = await pagina(C)
await entrar(pc, code, pa)
await esperar(1500)

// A → B, em privado, pelo «Para».
await abrirChat(pa)
await pa.locator('#rm-chat-to').selectOption({ label: 'Joaquim Ferreira' })
const campoA = pa.locator('.rm-chat__input textarea')
await campoA.fill('Segredo só para o Joaquim')
await campoA.press('Enter')
// Pública de C (controlo: a pública chega a todos).
await abrirChat(pc)
await pc.locator('.rm-chat__input textarea').fill('Mensagem pública da Teresa')
await pc.locator('.rm-chat__input textarea').press('Enter')
await esperar(2000)
await abrirChat(pb)
await esperar(800)
ok((await textoDoChat(pb)).includes('Segredo só para o Joaquim'), 'B recebe a privada de A')
ok(/Privada · para ti/i.test(await textoDoChat(pb)), 'B vê-a marcada «Privada · para ti»')
ok(!(await textoDoChat(pc)).includes('Segredo só para o Joaquim'), 'C NÃO vê a privada de A a B')
ok((await textoDoChat(pb)).includes('Mensagem pública da Teresa'), 'a pública de C chega a B (controlo)')

// B responde no fio da privada, sem escolher «Para»: continua privada.
const msgB = pb.locator('.rm-chat__msg', { hasText: 'Segredo só para o Joaquim' })
await msgB.hover()
await msgB.getByRole('button', { name: 'Responder', exact: true }).click({ timeout: 5000 })
await pb.locator('.rm-chat__input textarea').fill('Resposta privada do Joaquim')
await pb.locator('.rm-chat__input textarea').press('Enter')
await esperar(2000)
ok((await textoDoChat(pa)).includes('Resposta privada do Joaquim'), 'A recebe a resposta privada de B')
ok(!(await textoDoChat(pc)).includes('Resposta privada do Joaquim'), 'C NÃO vê a resposta privada')

// Recarregar: o histórico do servidor devolve as privadas só ao par.
await pa.screenshot({ path: `${DIR}app/chat-privado-A.png` })
for (const [p, s] of [[pa, null], [pb, pa], [pc, pa]]) await entrar(p, code, s, true)
await esperar(1500)
for (const p of [pa, pb, pc]) await abrirChat(p)
await esperar(2500)
const [ta, tb, tc] = [await textoDoChat(pa), await textoDoChat(pb), await textoDoChat(pc)]
ok(ta.includes('Segredo só para o Joaquim') && ta.includes('Resposta privada do Joaquim'), 'depois de recarregar, A continua a ver a conversa directa')
ok(tb.includes('Segredo só para o Joaquim') && tb.includes('Resposta privada do Joaquim'), 'depois de recarregar, B continua a ver a conversa directa')
ok(!tc.includes('Segredo') && !tc.includes('Resposta privada') && tc.includes('Mensagem pública da Teresa'), 'depois de recarregar, C vê a pública e NÃO as privadas')
// E pela API, directamente: o filtro é do servidor, não da interface.
const histC = await api(`/api/rooms/${code}/chat`, { token: C.token })
ok(histC.ok && !histC.data.some((m) => m.to_user_id), 'GET /chat de C não traz nenhuma privada alheia')
const histB = await api(`/api/rooms/${code}/chat`, { token: B.token })
ok(histB.ok && histB.data.filter((m) => m.to_user_id).length === 2, 'GET /chat de B traz as duas privadas do par')
await pb.screenshot({ path: `${DIR}app/chat-privado-B.png` })
await b.close()
log(falhas ? `${falhas} falha(s)` : 'tudo verde')
process.exit(falhas ? 1 : 0)
