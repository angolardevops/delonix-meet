// Sala contra a linha de backend nova (#90 + #92): media a fluir e conversa directa.
//
// O que prova, pela interface real e com três browsers:
//   1. A e B entram (`/api/ice-servers`, `join`, `/ws`) e há `framesDecoded` a
//      SUBIR nos dois sentidos durante ≥ 30 s (getStats de cada RTCPeerConnection).
//   2. A escreve em privado a B: B recebe; C não recebe nada, nem pelo /ws
//      (tramas capturadas) nem pelo histórico `GET /api/rooms/{code}/messages`.
//   3. B responde em privado e reage: o fio e a reacção ficam no par.
//   4. Varredura: todas as respostas ≥ 400 da API vistas pelos três browsers.
//
// Uso: API=http://127.0.0.1:8460 APP=http://127.0.0.1:5460 node e2e/backend-novo-sala.mjs
import { chromium } from '@playwright/test'
import { criarConta, entrar, PASSWORD } from './sessao.mjs'

const API = process.env.API ?? 'http://127.0.0.1:8460'
const APP = process.env.APP ?? 'http://127.0.0.1:5460'
const SEGUNDOS = Number(process.env.SEGUNDOS ?? 30)
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
  try { json = JSON.parse(txt) } catch { /* corpo não-JSON */ }
  return { status: r.status, json, txt }
}

// ---------- montagem: uma organização, três pessoas, uma sala ----------
const dono = await criarConta(API, 'sala')
const login = async (email) => (await api('/api/auth/login', { method: 'POST', body: { email, password: PASSWORD } })).json
const tokA = (await login(dono.email)).access_token
const orgs = (await api('/api/orgs', { token: tokA })).json
const orgId = orgs[0].id
const dominio = dono.email.split('@')[1]
const membro = async (nome) => {
  const email = `${nome}@${dominio}`
  const r = await api(`/api/orgs/${orgId}/members`, { token: tokA, method: 'POST', body: { email, username: nome, password: PASSWORD } })
  if (r.status >= 300) throw new Error(`membro ${nome}: ${r.status} ${r.txt}`)
  return { email, password: PASSWORD }
}
const contaB = await membro(`bea${Date.now().toString(36)}`)
const contaC = await membro(`caio${Date.now().toString(36)}`)
const sala = (await api('/api/rooms', { token: tokA, method: 'POST', body: { name: 'prova backend novo', topology: 'sfu', waiting_room: false, e2ee: false } })).json
console.log(`· sala ${sala.code} na org ${orgId}`)

const browser = await chromium.launch({
  args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream', '--autoplay-policy=no-user-gesture-required'],
})
const erros = []
async function pagina(nome, conta) {
  const ctx = await browser.newContext({ permissions: ['camera', 'microphone'], viewport: { width: 1440, height: 900 }, locale: 'pt-PT' })
  // Guarda cada RTCPeerConnection para o getStats.
  await ctx.addInitScript(() => {
    const Orig = window.RTCPeerConnection
    window.__pcs = []
    window.RTCPeerConnection = function (...a) {
      const pc = new Orig(...a)
      window.__pcs.push(pc)
      return pc
    }
    window.RTCPeerConnection.prototype = Orig.prototype
  })
  const page = await ctx.newPage()
  page.on('response', (r) => {
    const u = new URL(r.url())
    if (u.pathname.startsWith('/api/') && r.status() >= 400) erros.push(`${nome} ${r.request().method()} ${u.pathname} → ${r.status()}`)
  })
  const tramas = { in: [], out: [] }
  page.on('websocket', (ws) => {
    if (!ws.url().includes('/ws?')) return
    ws.on('framereceived', (f) => typeof f.payload === 'string' && tramas.in.push(f.payload))
    ws.on('framesent', (f) => typeof f.payload === 'string' && tramas.out.push(f.payload))
  })
  await entrar(page, APP, conta)
  return { nome, page, tramas }
}

const A = await pagina('A', dono)
const B = await pagina('B', contaB)
const C = await pagina('C', contaC)

async function carregarEntrar(p) {
  await p.page.goto(`${APP}/#/r/${sala.code}`, { waitUntil: 'domcontentloaded' })
  await p.page.getByRole('button', { name: /entrar na sessão/i }).first().click({ timeout: 60000 })
}
const dentro = (p) =>
  p.page
    .waitForFunction(() => !document.querySelector('.rm-prejoin') && !document.querySelector('.rm-waiting'), null, { timeout: 90000 })
    .then(() => true, () => false)
await carregarEntrar(A)
ok(await dentro(A), 'A (dono) entrou na sala')
// Membros da organização SEM convite esperam (entrada directa é só do dono,
// convidados e admissores — `rooms.rs`). O dono admite os dois de uma vez: é
// a mensagem `admit-all` do #92.
await carregarEntrar(B)
await carregarEntrar(C)
const botaoTodos = A.page.getByRole('button', { name: /admitir todos/i }).first()
const viuTodos = await botaoTodos.waitFor({ timeout: 30000 }).then(() => true, () => false)
ok(viuTodos, 'A vê «Admitir todos» com B e C à espera')
if (viuTodos) await botaoTodos.click()
ok(A.tramas.out.some((f) => f.includes('"admit-all"')), 'a UI envia `admit-all`')
ok(await dentro(B), 'B foi admitido e está na sala')
ok(await dentro(C), 'C foi admitido e está na sala')

// ---------- 1. media nos dois sentidos ----------
const frames = (p) =>
  p.page.evaluate(async () => {
    let video = 0
    let audio = 0
    for (const pc of window.__pcs ?? []) {
      if (pc.connectionState === 'closed') continue
      const st = await pc.getStats()
      st.forEach((r) => {
        if (r.type === 'inbound-rtp' && r.kind === 'video') video += r.framesDecoded ?? 0
        if (r.type === 'inbound-rtp' && r.kind === 'audio') audio += r.packetsReceived ?? 0
      })
    }
    return { video, audio }
  })
await A.page.waitForTimeout(5000)
const inicio = { A: await frames(A), B: await frames(B) }
const serie = []
for (let s = 0; s < SEGUNDOS; s += 5) {
  await A.page.waitForTimeout(5000)
  serie.push({ t: s + 5, A: (await frames(A)).video, B: (await frames(B)).video })
}
const fim = { A: await frames(A), B: await frames(B) }
console.log(`  · framesDecoded (vídeo recebido) por 5 s: ${JSON.stringify(serie)}`)
const subiuSempre = (k) => serie.every((x, i) => (i === 0 ? x[k] > inicio[k].video : x[k] > serie[i - 1][k]))
ok(fim.A.video - inicio.A.video > 0 && subiuSempre('A'), `A decodifica vídeo de B durante ${SEGUNDOS} s`, `${inicio.A.video} → ${fim.A.video}`)
ok(fim.B.video - inicio.B.video > 0 && subiuSempre('B'), `B decodifica vídeo de A durante ${SEGUNDOS} s`, `${inicio.B.video} → ${fim.B.video}`)
ok(fim.A.audio > inicio.A.audio && fim.B.audio > inicio.B.audio, 'áudio recebido nos dois sentidos', `A ${inicio.A.audio}→${fim.A.audio} B ${inicio.B.audio}→${fim.B.audio}`)

// ---------- 2. conversa directa A → B ----------
async function abrirChat(p) {
  const tab = p.page.getByRole('tab', { name: /^chat/i })
  if (await tab.count()) await tab.first().click()
  else await p.page.getByRole('button', { name: /^chat/i }).first().click()
  await p.page.waitForSelector('#rm-chat-to', { timeout: 20000 })
}
for (const p of [A, B, C]) await abrirChat(p)
const peerIdDe = async (quem, alvo) =>
  quem.page.evaluate((nome) => {
    const o = [...document.querySelectorAll('#rm-chat-to option')].find((x) => x.textContent?.trim() === nome)
    return o?.getAttribute('value') ?? null
  }, alvo)
const nomeB = contaB.email.split('@')[0]
const nomeA = (await api('/api/users/me', { token: tokA })).json.username
const idB = await peerIdDe(A, nomeB)
ok(!!idB, 'A vê B na lista «Para»', idB ?? 'sem opção')
const segredo = `segredo-${Date.now().toString(36)}`
const cInAntes = C.tramas.in.length
await A.page.selectOption('#rm-chat-to', idB)
await A.page.locator('.rm-chat__input textarea').fill(segredo)
await A.page.locator('.rm-chat__input textarea').press('Enter')
const saiu = await A.page.waitForFunction(() => true, null).then(() => A.tramas.out.find((f) => f.includes(segredo)))
ok(!!saiu && JSON.parse(saiu).to === idB, 'a trama enviada leva `to` = peer_id de B', saiu)
const chegouB = await B.page.getByText(segredo).first().waitFor({ timeout: 15000 }).then(() => true, () => false)
ok(chegouB, 'B recebe a mensagem privada')
await C.page.waitForTimeout(3000)
ok(!C.tramas.in.slice(cInAntes).some((f) => f.includes(segredo)), 'C NÃO recebe a privada pelo /ws', `${C.tramas.in.length - cInAntes} tramas recebidas entretanto`)
ok((await C.page.getByText(segredo).count()) === 0, 'C NÃO a vê no ecrã')
const confirmada = A.tramas.in.find((f) => f.includes('"chat-sent"'))
ok(!!confirmada, 'A recebe `chat-sent` (confirmação com id)', confirmada)

const hist = async (p) =>
  p.page.evaluate(async (code) => {
    const r = await fetch(`/api/rooms/${code}/messages`, { headers: { Authorization: `Bearer ${localStorage.getItem('dx_access')}` } })
    return { status: r.status, body: await r.json() }
  }, sala.code)
const hC = await hist(C)
ok(hC.status === 200 && !JSON.stringify(hC.body).includes(segredo), 'C NÃO a vê no histórico da API', `HTTP ${hC.status}, ${hC.body.length} mensagens`)
const hB = await hist(B)
const privB = hB.body.find((m) => m.message === segredo)
ok(!!privB && privB.to_username === nomeB && privB.to_user_id, 'B vê-a no histórico com to_user_id/to_username', JSON.stringify(privB))

// ---------- 3. B responde em privado e reage ----------
const resposta = `resposta-${Date.now().toString(36)}`
const linhaB = B.page.locator('.rm-chat__msg, .rm-chat__item, li, div').filter({ hasText: segredo }).last()
await linhaB.hover()
await B.page.getByRole('button', { name: /^responder$/i }).first().click()
await B.page.locator('.rm-chat__input textarea').fill(resposta)
await B.page.locator('.rm-chat__input textarea').press('Enter')
const saiuResp = await B.page.waitForTimeout(1500).then(() => B.tramas.out.find((f) => f.includes(resposta)))
ok(!!saiuResp && JSON.parse(saiuResp).reply_to === privB?.id, 'a resposta de B leva reply_to = id da privada', saiuResp)
await B.page.getByRole('button', { name: /reagir com 👍/i }).first().click()
await B.page.waitForTimeout(3000)
ok(A.tramas.in.some((f) => f.includes(resposta)), 'A recebe a resposta')
ok(!C.tramas.in.some((f) => f.includes(resposta)), 'C NÃO recebe a resposta')
const reacA = A.tramas.in.find((f) => f.includes('"chat-reactions"') && f.includes(privB?.id))
ok(!!reacA, 'A recebe `chat-reactions` da privada', reacA)
ok(!C.tramas.in.some((f) => f.includes('"chat-reactions"') && f.includes(privB?.id)), 'C NÃO recebe a reacção da privada')
const hA = await hist(A)
const respA = hA.body.find((m) => m.message === resposta)
ok(!!respA && respA.parent_id === privB?.id && respA.to_username === nomeA, 'no histórico de A a resposta é fio da privada e vai para A', JSON.stringify(respA))
const privA = hA.body.find((m) => m.message === segredo)
ok(privA?.reactions?.['👍'] === 1, 'a reacção fica gravada na privada', JSON.stringify(privA?.reactions))
const hC2 = await hist(C)
ok(!JSON.stringify(hC2.body).includes(resposta), 'C continua sem ver nada do par no histórico')

// Controlo: C está MESMO a ouvir — uma mensagem pública chega-lhe.
const publica = `publica-${Date.now().toString(36)}`
await A.page.selectOption('#rm-chat-to', '')
await A.page.locator('.rm-chat__input textarea').fill(publica)
await A.page.locator('.rm-chat__input textarea').press('Enter')
ok(await C.page.getByText(publica).first().waitFor({ timeout: 15000 }).then(() => true, () => false), 'controlo: C recebe uma mensagem PÚBLICA (o socket dele está vivo)')

// ---------- 4. fixar para todos e co-anfitrião, pela interface ----------
const tabPessoas = A.page.getByRole('tab', { name: /participantes/i })
if (await tabPessoas.count()) await tabPessoas.first().click()
else await A.page.getByRole('button', { name: /participantes/i }).first().click()
await A.page.getByRole('button', { name: `Fixar ${nomeB} no palco de toda a gente` }).first().click({ timeout: 20000 })
await C.page.waitForTimeout(1500)
ok(A.tramas.out.some((f) => f.includes('"spotlight"') && f.includes(idB)), 'o botão «Fixar para toda a gente» envia `spotlight`')
ok(C.tramas.in.some((f) => f.includes('"type":"spotlight"') && f.includes(idB)), 'C recebe o `spotlight` de B')
await A.page.getByRole('button', { name: `Permitir que ${nomeB} admita entradas` }).first().click({ timeout: 20000 })
await C.page.waitForTimeout(1500)
ok(A.tramas.out.some((f) => f.includes('"set-role"') && f.includes('"cohost"')), 'o botão de admissão envia `set-role cohost`')
const pr = C.tramas.in.map((f) => JSON.parse(f)).filter((m) => m.type === 'peer-role' && m.peer_id === idB).at(-1)
ok(pr?.role === 'cohost' && pr?.can_admit === true, 'C recebe `peer-role {role: cohost, can_admit: true}`', JSON.stringify(pr))
const joinedB = B.tramas.in.map((f) => JSON.parse(f)).find((m) => m.type === 'joined')
ok(joinedB?.started_at > 0, '`joined` de B traz `started_at`', String(joinedB?.started_at))

console.log('\n· respostas ≥ 400 da API vistas pelos browsers:')
for (const e of [...new Set(erros)]) console.log(`    ${e}`)
await browser.close()
console.log(falhas ? `\n=== ${falhas} FALHARAM ===` : '\n=== TUDO VERDE ===')
process.exit(falhas ? 1 : 0)
