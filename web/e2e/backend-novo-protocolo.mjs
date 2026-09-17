// O protocolo da sala da UI (`signaling.ts`, `ClientMsgB1`/`ServerMsgB1`) contra
// o `/ws` da linha de backend nova (#92). Sem browser: três sockets reais com
// as MESMAS formas de mensagem que a UI manda, e o que o servidor devolve a
// cada um. Cobre o que o `backend-novo-sala.mjs` não conduz pela interface.
//
// Uso: API=http://127.0.0.1:8460 node e2e/backend-novo-protocolo.mjs
import { criarConta, PASSWORD } from './sessao.mjs'

const API = process.env.API ?? 'http://127.0.0.1:8460'
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
const esperar = (ms) => new Promise((r) => setTimeout(r, ms))

const dono = await criarConta(API, 'prot')
const login = async (email) => (await api('/api/auth/login', { method: 'POST', body: { email, password: PASSWORD } })).json.access_token
const tA = await login(dono.email)
const orgId = (await api('/api/orgs', { token: tA })).json[0].id
const dominio = dono.email.split('@')[1]
const novo = async (n) => {
  const email = `${n}${Date.now().toString(36)}@${dominio}`
  const r = await api(`/api/orgs/${orgId}/members`, { token: tA, method: 'POST', body: { email, username: email.split('@')[0], password: PASSWORD } })
  if (r.status >= 300) throw new Error(`${r.status} ${r.txt}`)
  return login(email)
}
const tB = await novo('pb')
const tC = await novo('pc')
const sala = (await api('/api/rooms', { token: tA, method: 'POST', body: { name: 'protocolo', topology: 'sfu', waiting_room: false, e2ee: false } })).json

function ligar(nome, token) {
  return new Promise(async (resolve, reject) => {
    const j = await api(`/api/rooms/${sala.code}/join`, { token, method: 'POST' })
    if (j.status !== 200) return reject(new Error(`${nome} join ${j.status}`))
    const ws = new WebSocket(`${WS}/ws?token=${j.json.room_token}&room=${sala.code}`)
    const c = { nome, ws, msgs: [], send: (m) => ws.send(JSON.stringify(m)) }
    c.de = (type, pred = () => true) => c.msgs.filter((m) => m.type === type && pred(m))
    c.ultimo = (type, pred) => c.de(type, pred).at(-1)
    ws.onmessage = (e) => c.msgs.push(JSON.parse(e.data))
    ws.onopen = () => resolve(c)
    ws.onerror = () => reject(new Error(`${nome} ws`))
  })
}
const A = await ligar('A', tA)
await esperar(800)
const joinedA = A.ultimo('joined')
ok(!!joinedA && joinedA.started_at > 0, '`joined` traz `started_at` (epoch ms)', `started_at=${joinedA?.started_at}`)
const B = await ligar('B', tB)
const C = await ligar('C', tC)
await esperar(800)
ok(B.de('waiting').length === 1 && C.de('waiting').length === 1, 'B e C (membros sem convite) esperam')
A.send({ type: 'admit-all' })
await esperar(1000)
const joinedB = B.ultimo('joined')
ok(!!joinedB && !!C.ultimo('joined'), '`admit-all` admite os dois')
const idA = joinedA.peer_id
const idB = joinedB.peer_id
const idC = C.ultimo('joined').peer_id
const peerA = joinedB.peers.find((p) => p.peer_id === idA)
ok(peerA?.role === 'host' && ['sso', 'password'].includes(peerA?.origin), '`PeerInfo` traz `role` e `origin`', JSON.stringify(peerA))

// ---- set-role → peer-role (sempre com role e can_admit) ----
A.send({ type: 'set-role', to: idB, role: 'cohost' })
await esperar(600)
const prC = C.ultimo('peer-role', (m) => m.peer_id === idB)
ok(prC?.role === 'cohost' && prC?.can_admit === true, '`set-role cohost` → `peer-role {role, can_admit}` a toda a sala', JSON.stringify(prC))
A.send({ type: 'set-role', to: idB, role: 'attendee' })
await esperar(600)
const prC2 = C.ultimo('peer-role', (m) => m.peer_id === idB)
ok(prC2?.role === 'attendee' && prC2?.can_admit === false, '… e volta a `attendee` sem admissão', JSON.stringify(prC2))
C.send({ type: 'set-role', to: idB, role: 'cohost' })
await esperar(600)
ok(C.ultimo('peer-role', (m) => m.peer_id === idB)?.role === 'attendee', 'um participante não muda papéis (fica como estava)')

// ---- spotlight ----
A.send({ type: 'spotlight', peer: idC })
await esperar(500)
ok(B.ultimo('spotlight')?.peer === idC && C.ultimo('spotlight')?.peer === idC, '`spotlight` fixa para toda a gente', JSON.stringify(B.ultimo('spotlight')))
A.send({ type: 'spotlight', peer: null })
await esperar(500)
ok(B.ultimo('spotlight')?.peer === null, '`spotlight null` limpa')

// ---- Q&A moderado ----
B.send({ type: 'qa-ask', text: 'pergunta a esconder' })
await esperar(500)
const q = A.ultimo('qa')?.questions?.find((x) => x.text === 'pergunta a esconder')
ok(!!q, 'A vê a pergunta de B')
A.send({ type: 'qa-spotlight', id: q?.id })
await esperar(500)
ok(C.ultimo('qa')?.questions?.find((x) => x.id === q?.id)?.spotlight === true, '`qa-spotlight` destaca para a sala')
A.send({ type: 'qa-hide', id: q?.id, hidden: true })
await esperar(500)
const qaA = A.ultimo('qa')?.questions?.find((x) => x.id === q?.id)
const qaC = C.ultimo('qa')?.questions?.find((x) => x.id === q?.id)
ok(qaA?.hidden === true && qaA?.spotlight === false, '`qa-hide` esconde (e tira do destaque) — o anfitrião continua a vê-la', JSON.stringify(qaA))
ok(!qaC, 'C deixa de receber a pergunta escondida', JSON.stringify(qaC))

// ---- sala de espera em runtime ----
A.send({ type: 'waiting-room', on: true })
await esperar(500)
ok(C.ultimo('room-settings')?.waiting_room === true, '`waiting-room on` → `room-settings.waiting_room`', JSON.stringify(C.ultimo('room-settings')))
A.send({ type: 'waiting-room', on: false })
await esperar(500)
ok(C.ultimo('room-settings')?.waiting_room === false, '`waiting-room off` volta a abrir')

// ---- difundir às salas ----
A.send({ type: 'breakouts-broadcast', text: 'cinco minutos' })
await esperar(500)
const an = C.ultimo('announcement')
ok(an?.text === 'cinco minutos' && an?.at > 0 && typeof an?.from === 'string', '`breakouts-broadcast` → `announcement {from, text, at}`', JSON.stringify(an))
B.send({ type: 'breakouts-broadcast', text: 'não sou anfitrião' })
await esperar(500)
ok(!C.de('announcement').some((m) => m.text === 'não sou anfitrião'), 'quem não é anfitrião não difunde')

// ---- quadro: objectos, páginas, cursores, permissões ----
A.send({ type: 'wb-open' })
await esperar(300)
const nota = crypto.randomUUID()
B.send({ type: 'wb-stroke', stroke: { id: nota, kind: 'note', text: 'nota', pts: [[0.2, 0.2]], c: '#ffcc00', w: 2, page: 0 } })
await esperar(500)
const recebida = C.ultimo('wb-stroke')?.stroke
ok(recebida?.id === nota && recebida?.kind === 'note' && recebida?.by, '`wb-stroke` de nota chega com id, kind e autor carimbado', JSON.stringify(recebida))
B.send({ type: 'wb-stroke', stroke: { id: crypto.randomUUID(), kind: 'shape', shape: 'rect', pts: [[0.1, 0.1], [0.3, 0.3]], c: '#000', w: 2, p: [0.5, 0.6] } })
B.send({ type: 'wb-stroke', stroke: { pts: [[0.1, 0.1], [0.2, 0.2]], c: '#000', w: 3, p: [0.4, 0.9] } })
await esperar(500)
const traco = C.ultimo('wb-stroke')?.stroke
ok(Array.isArray(traco?.p) && traco.p.length === 2, 'a pressão (`p`) de um traço chega aos outros', JSON.stringify(traco))
B.send({ type: 'wb-update', id: nota, text: 'nota editada' })
B.send({ type: 'wb-transform', id: nota, dx: 0.1, dy: 0 })
await esperar(500)
ok(C.ultimo('wb-update')?.text === 'nota editada', '`wb-update` do autor chega')
ok(C.ultimo('wb-transform')?.id === nota, '`wb-transform` do autor chega')
C.send({ type: 'wb-erase', id: nota })
await esperar(400)
ok(!B.de('wb-erase').some((m) => m.id === nota), 'quem não é autor nem anfitrião não apaga')
A.send({ type: 'wb-erase', id: nota })
await esperar(400)
ok(C.ultimo('wb-erase')?.id === nota, 'o anfitrião apaga → `wb-erase`')
B.send({ type: 'wb-cursor', x: 0.5, y: 0.4, laser: true, input: 'pen' })
await esperar(400)
const cur = C.ultimo('wb-cursor')
ok(cur?.from === idB && cur?.laser === true && cur?.input === 'pen', '`wb-cursor` chega com `from`, laser e input', JSON.stringify(cur))
B.send({ type: 'wb-add-page' })
await esperar(400)
ok(C.ultimo('wb-pages')?.count === 2, '`wb-add-page` → `wb-pages.count`', JSON.stringify(C.ultimo('wb-pages')))
A.send({ type: 'wb-page', page: 1 })
await esperar(400)
ok(B.ultimo('wb-pages')?.current === 1, '`wb-page` do anfitrião muda a página de todos')
A.send({ type: 'wb-lock', on: true })
await esperar(400)
ok(C.ultimo('wb-writers')?.restricted === true, '`wb-lock` → `wb-writers.restricted`')
A.send({ type: 'wb-grant', to: idC, allowed: true })
await esperar(400)
const wr = B.ultimo('wb-writers')
ok(wr?.writers?.includes(idC), '`wb-grant` → `wb-writers.writers`', JSON.stringify(wr))
const antes = A.de('wb-stroke').length
B.send({ type: 'wb-stroke', stroke: { pts: [[0.1, 0.1], [0.2, 0.2]], c: '#000', w: 3, page: 1 } })
C.send({ type: 'wb-stroke', stroke: { pts: [[0.3, 0.3], [0.4, 0.4]], c: '#f00', w: 3, page: 1 } })
await esperar(500)
const novos = A.de('wb-stroke').slice(antes)
ok(novos.length === 1 && novos[0].stroke.c === '#f00', 'com o quadro trancado só escreve quem tem `grant`', `${novos.length} traço(s)`)

for (const s of [A, B, C]) s.ws.close()
console.log(falhas ? `\n=== ${falhas} FALHARAM ===` : '\n=== TUDO VERDE ===')
process.exit(falhas ? 1 : 0)
