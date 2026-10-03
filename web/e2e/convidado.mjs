#!/usr/bin/env node
// Um CONVIDADO SEM CONTA entra numa reunião — contra o servidor a sério, pela
// API e pelo WebSocket de sinalização, sem interface.
//
// Sem interface DE PROPÓSITO: a consola da `main` vai ser substituída (branch
// `frontend/ui-template-rebuild`), e este teste é o contrato que o ecrã novo tem
// de cumprir. Quando o ecrã existir, o e2e de interface assenta nisto; até lá,
// isto é o que prova que o caminho funciona.
//
// O caminho:
//   anfitrião cria a sala e entra (directo)
//   → anónimo pede `guest-join` com um nome → token de sala `origin: guest`
//   → abre o /ws → cai na SALA DE ESPERA
//   → o anfitrião vê-o MARCADO como convidado e admite-o
//   → o convidado recebe `joined` com o anfitrião no roster
// E a metade negativa, já dentro da sala:
//   → o anfitrião tenta passar-lhe o papel → recusado (ninguém muda de papel)
//   → o convidado tenta admitir outro, gravar, expulsar e criar grupos → nada
//   → um segundo convidado recusado recebe `denied`
//   → o convidado cai e VOLTA com o segredo de reclamação → mesmo lugar, sem
//     sala de espera, e continua marcado como convidado
//   → expulso, recebe `kicked`
//
// O QUE NÃO PROVA, e é preciso dizê-lo: não há media. Não abre RTCPeerConnection
// nem mede que o vídeo do convidado chega ao anfitrião — isso é o `reuniao.mjs`
// (membros) e fica para o e2e de interface da UI nova. A sinalização é a mesma
// para membros e convidados depois do `joined`; a diferença está toda ANTES.
//
// Uso:  API=http://127.0.0.1:8180 node web/e2e/convidado.mjs
import WebSocket from 'ws'
import { criarConta } from './sessao.mjs'

const API = process.env.API ?? 'http://127.0.0.1:8180'
const WS = API.replace(/^http/, 'ws')
let falhas = 0
const ok = (c, n, d) => {
  console.log(`  ${c ? '✓' : '✗'} ${n}${d ? `  — ${d}` : ''}`)
  if (!c) falhas++
}
const dormir = (ms) => new Promise((r) => setTimeout(r, ms))

async function api(path, { token, method = 'GET', body } = {}) {
  const r = await fetch(`${API}${path}`, {
    method,
    headers: {
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
      ...(body ? { 'Content-Type': 'application/json' } : {}),
    },
    ...(body ? { body: JSON.stringify(body) } : {}),
  })
  let json = null
  try { json = await r.json() } catch { /* sem corpo */ }
  return { status: r.status, json }
}

/** Liga ao /ws e acumula as mensagens; `esperar(pred)` devolve a primeira que bate. */
async function ligar(roomToken, reconnect) {
  const q = `token=${encodeURIComponent(roomToken)}${reconnect ? `&reconnect=${encodeURIComponent(reconnect)}` : ''}`
  const ws = new WebSocket(`${WS}/ws?${q}`)
  const msgs = []
  ws.on('message', (d) => { try { msgs.push(JSON.parse(d.toString())) } catch { /* ignora */ } })
  await new Promise((res, rej) => { ws.on('open', res); ws.on('error', rej) })
  const esperar = (pred, ms = 10_000) => new Promise((resolve) => {
    const t0 = Date.now()
    const tick = () => {
      const m = msgs.find(pred)
      if (m) return resolve(m)
      if (Date.now() - t0 > ms) return resolve(null)
      setTimeout(tick, 50)
    }
    tick()
  })
  const enviar = (m) => ws.send(JSON.stringify(m))
  return { ws, msgs, esperar, enviar }
}

console.log('\n=== Convidado sem conta (API + /ws, servidor real) ===\n')

// ---- anfitrião ----
const conta = await criarConta(API, 'conv')
const login = await api('/api/auth/login', { method: 'POST', body: conta })
const tokenDono = login.json?.access_token
if (!tokenDono) { console.log('  ✗ login do anfitrião falhou', login.status); process.exit(1) }
const sala = (await api('/api/rooms', { token: tokenDono, method: 'POST', body: { name: 'Reunião com externos', topology: 'sfu' } })).json
ok(sala?.allow_guests === true, 'a sala nasce a aceitar convidados', sala?.code)
const joinDono = await api(`/api/rooms/${sala.code}/join`, { token: tokenDono, method: 'POST' })
const dono = await ligar(joinDono.json.room_token)
const entrouDono = await dono.esperar((m) => m.type === 'joined')
ok(Boolean(entrouDono), 'o anfitrião entra directo')

// ---- convidado ----
const gj = await api(`/api/rooms/${sala.code}/guest-join`, { method: 'POST', body: { display_name: '  Maria   Externa ' } })
ok(gj.status === 200 && Boolean(gj.json?.room_token), 'guest-join sem conta → 200 com token', `${gj.status}`)
ok(gj.json?.guest?.display_name === 'Maria Externa', 'o nome volta normalizado', JSON.stringify(gj.json?.guest))
ok(Array.isArray(gj.json?.ice_servers?.iceServers), 'traz a configuração ICE (o convidado não tem sessão para o /api/ice)')

const conv = await ligar(gj.json.room_token)
const espera = await conv.esperar((m) => ['waiting', 'joined'].includes(m.type))
ok(espera?.type === 'waiting', 'o convidado cai na SALA DE ESPERA', espera?.type)

const bateu = await dono.esperar((m) => m.type === 'waiting-join' && m.peer?.username === 'Maria Externa')
ok(bateu?.peer?.is_guest === true, 'o anfitrião vê-o à porta MARCADO como convidado', JSON.stringify(bateu?.peer))

// Sozinho à porta, não entra — nem a tentar admitir-se a si próprio.
conv.enviar({ type: 'admit', to: bateu?.peer?.peer_id })
await dormir(1500)
ok(!conv.msgs.some((m) => m.type === 'joined'), 'o convidado não se admite a si próprio')

dono.enviar({ type: 'admit', to: bateu.peer.peer_id })
const dentro = await conv.esperar((m) => m.type === 'joined')
ok(Boolean(dentro), 'admitido pelo anfitrião, o convidado recebe `joined`')
ok(Boolean(dentro?.peers?.some((p) => p.host)), 'e vê o anfitrião no roster', JSON.stringify(dentro?.peers?.map((p) => p.username)))
const guestPeer = dentro?.peer_id
const anunciado = await dono.esperar((m) => m.type === 'peer-joined' && m.peer?.peer_id === guestPeer)
ok(anunciado?.peer?.is_guest === true && anunciado.peer.host === false, 'na sala, continua marcado como convidado para o anfitrião')

// ---- metade negativa, já dentro ----
dono.enviar({ type: 'transfer-host', to: guestPeer })
await dormir(1200)
ok(!dono.msgs.some((m) => m.type === 'host-changed') && !conv.msgs.some((m) => m.type === 'host-changed'),
  'o anfitrião NÃO consegue passar o papel a um convidado')

// Um segundo convidado à porta: o primeiro não o admite.
const gj2 = await api(`/api/rooms/${sala.code}/guest-join`, { method: 'POST', body: { display_name: 'Intruso' } })
const conv2 = await ligar(gj2.json.room_token)
const bateu2 = await dono.esperar((m) => m.type === 'waiting-join' && m.peer?.username === 'Intruso')
ok(!conv.msgs.some((m) => m.type === 'waiting-join'), 'o convidado não recebe a fila de espera (só quem admite)')
conv.enviar({ type: 'admit', to: bateu2?.peer?.peer_id })
conv.enviar({ type: 'kick', to: entrouDono?.peer_id })
conv.enviar({ type: 'server-record', active: true })
conv.enviar({ type: 'breakouts-create', count: 2, minutes: 5 })
conv.enviar({ type: 'room-lock', locked: true })
await dormir(1500)
ok(!conv2.msgs.some((m) => m.type === 'joined'), 'o convidado não admite outro convidado')
ok(!dono.msgs.some((m) => m.type === 'kicked'), 'o convidado não expulsa o anfitrião')
ok(!dono.msgs.some((m) => m.type === 'server-recording'), 'o convidado não liga a gravação')
ok(!dono.msgs.some((m) => m.type === 'breakouts-created'), 'o convidado não cria salas de grupo')
ok(!dono.msgs.some((m) => m.type === 'room-settings' && m.locked), 'o convidado não tranca a sala')

dono.enviar({ type: 'deny', to: bateu2.peer.peer_id })
const recusado = await conv2.esperar((m) => m.type === 'denied')
ok(Boolean(recusado), 'o anfitrião recusa o segundo convidado → `denied`')
conv2.ws.close()

// ---- quebra de rede: volta ao MESMO lugar, sem sala de espera, ainda convidado ----
const segredo = dentro?.reconnect
ok(typeof segredo === 'string' && segredo.length > 0, 'o convidado admitido recebe segredo de reclamação')
conv.ws.terminate()
await dono.esperar((m) => m.type === 'peer-reconnecting' && m.peer_id === guestPeer)
// Um token NOVO (o de há pouco pode já ter expirado numa quebra longa).
const gj3 = await api(`/api/rooms/${sala.code}/guest-join`, { method: 'POST', body: { display_name: 'Maria Externa' } })
const volta = await ligar(gj3.json.room_token, segredo)
const reentrada = await volta.esperar((m) => ['waiting', 'joined'].includes(m.type))
ok(reentrada?.type === 'joined' && reentrada.peer_id === guestPeer, 'volta ao MESMO lugar sem passar pela espera', JSON.stringify({ t: reentrada?.type, mesmo: reentrada?.peer_id === guestPeer }))
dono.msgs.length = 0
await dormir(300)

// ---- expulsão ----
dono.enviar({ type: 'kick', to: guestPeer })
const expulso = await volta.esperar((m) => m.type === 'kicked')
ok(Boolean(expulso), 'expulso pelo anfitrião → `kicked`')

// ---- a sala fecha a porta a meio ----
const fechar = await api(`/api/rooms/${sala.code}`, { token: tokenDono, method: 'PATCH', body: { allow_guests: false } })
const depois = await api(`/api/rooms/${sala.code}/guest-join`, { method: 'POST', body: { display_name: 'Tarde' } })
ok(fechar.status === 200 && depois.status === 403, 'o anfitrião desliga os convidados → o próximo leva 403', `${fechar.status}/${depois.status}`)

volta.ws.close()
dono.ws.close()
console.log(`\n=== ${falhas === 0 ? 'TODAS PASSARAM' : `${falhas} FALHARAM`} ===`)
process.exit(falhas ? 1 : 0)
