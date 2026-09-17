// O DIRECTO pelo `/api/rooms/{room_code}/live` do backend novo, contra um mediamtx local.
//
// O URL NÃO é montado aqui: é o `urlDoDirecto` da própria UI
// (`src/studio/directo.ts`), importado no browser a partir do vite. Assim o
// que se prova é o contrato da UI — caminho, `destinos`, `destination_ids`,
// `org_id`, `codec` — e não uma cópia dele.
//
// A media é um ffmpeg local em Matroska ao vivo (H.264 + Opus), que é o que o
// MediaRecorder produz: o Chromium do Playwright não traz codificador H.264,
// por isso o browser não é o que emite.
//
//   docker run -d --name mtx-ui-api-nova -p 127.0.0.1:19460:1935 bluenviron/mediamtx
//   API=http://127.0.0.1:8460 APP=http://127.0.0.1:5460 RTMP=rtmp://127.0.0.1:19460 node e2e/backend-novo-directo.mjs
import { chromium } from '@playwright/test'
import { spawn, spawnSync } from 'node:child_process'
import WebSocket from 'ws'
import { criarConta, PASSWORD } from './sessao.mjs'

const API = process.env.API ?? 'http://127.0.0.1:8460'
const APP = process.env.APP ?? 'http://127.0.0.1:5460'
const RTMP = process.env.RTMP ?? 'rtmp://127.0.0.1:19460'
let falhas = 0
const ok = (c, n, d) => {
  console.log(`  ${c ? '✓' : '✗'} ${n}${d ? `  — ${d}` : ''}`)
  if (!c) falhas++
}
const dormir = (ms) => new Promise((r) => setTimeout(r, ms))
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

const conta = await criarConta(API, 'dir')
const token = (await api('/api/auth/login', { method: 'POST', body: { email: conta.email, password: PASSWORD } })).json.access_token
const orgId = (await api('/api/orgs', { token })).json[0].id
const marca = Date.now().toString(36)
const guardado = await api(`/api/orgs/${orgId}/stream-destinations`, {
  token, method: 'POST', body: { kind: 'rtmp', label: 'mediamtx guardado', url: `${RTMP}/guardado`, stream_key: `g${marca}` },
})
ok(guardado.status === 201, 'destino guardado criado (201)', `${guardado.status} ${guardado.txt.slice(0, 120)}`)

const browser = await chromium.launch()
const page = await (await browser.newContext()).newPage()
await page.goto(`${APP}/`, { waitUntil: 'domcontentloaded' })
const urlDaUI = (code, roomToken, destinos, org) =>
  page.evaluate(
    async ({ api, code, roomToken, destinos, org }) => {
      const m = await import('/src/studio/directo.ts')
      const u = new URL(api)
      return m.urlDoDirecto({ protocol: u.protocol, host: u.host }, code, roomToken, destinos, org)
    },
    { api: API, code, roomToken, destinos, org },
  )

/** Abre uma emissão com o URL da UI; devolve a recusa do servidor ou `null` se aceitou. */
async function emitir(nome, destinos, org, segundos = 12) {
  const sala = (await api('/api/rooms', { token, method: 'POST', body: { name: nome, topology: 'sfu' } })).json
  const roomToken = (await api(`/api/rooms/${sala.code}/join`, { token, method: 'POST' })).json.room_token
  const url = await urlDaUI(sala.code, roomToken, destinos, org)
  const ws = new WebSocket(url)
  let recusa = null
  let fechou = false
  ws.on('message', (d, bin) => {
    if (bin) return
    try { recusa = JSON.parse(d.toString()).erro ?? recusa } catch { /* */ }
  })
  ws.on('close', () => (fechou = true))
  await new Promise((r, j) => { ws.once('open', r); ws.once('error', j) })
  await dormir(400)
  if (recusa || fechou) return { url, recusa: recusa ?? 'fechou sem razão' }
  const gerador = spawn('ffmpeg', [
    '-hide_banner', '-loglevel', 'error', '-re',
    '-f', 'lavfi', '-i', 'testsrc2=size=640x360:rate=30',
    '-f', 'lavfi', '-i', 'sine=frequency=440:sample_rate=48000',
    '-c:v', 'libx264', '-preset', 'ultrafast', '-tune', 'zerolatency', '-g', '30', '-pix_fmt', 'yuv420p',
    '-b:v', '1200k', '-c:a', 'libopus', '-b:a', '96k',
    '-f', 'matroska', '-live', '1', '-cluster_time_limit', '1000', '-t', String(segundos + 6), 'pipe:1',
  ])
  gerador.stdout.on('data', (c) => ws.readyState === WebSocket.OPEN && ws.send(c))
  return { url, sala, ws, gerador, recusa: null }
}
function sonda(caminho) {
  const r = spawnSync('ffprobe', ['-v', 'error', '-rw_timeout', '8000000', '-show_entries', 'stream=codec_name', '-of', 'csv=p=0', `${RTMP}/${caminho}`], { encoding: 'utf8', timeout: 20000 })
  return (r.stdout || '').trim().split('\n').filter(Boolean)
}
const parar = async (e) => {
  e.gerador?.kill('SIGKILL')
  e.ws?.close()
  await dormir(1500)
}

// 1. um destino por sessão (url + chave no JSON de `destinos`)
const chave1 = `s${marca}`
const e1 = await emitir('directo-sessao', [{ url: `${RTMP}/sessao`, chave: chave1, rotulo: 'sessão' }])
ok(new URL(e1.url).pathname.endsWith('/live'), 'a UI abre o WS em /api/rooms/{room_code}/live', new URL(e1.url).pathname)
ok(!e1.recusa, 'o servidor aceita a emissão', e1.recusa ?? 'aceite')
await dormir(8000)
const c1 = sonda(`sessao/${chave1}`)
ok(c1.includes('h264') && c1.some((c) => c === 'aac'), 'o mediamtx recebe H.264 + AAC do destino por sessão', c1.join(','))
await parar(e1)

// 2. um destino GUARDADO: só o id, em destination_ids + org_id
const e2 = await emitir('directo-guardado', [{ id: guardado.json?.id, url: '', chave: '', rotulo: 'guardado' }], orgId)
const q2 = new URL(e2.url).searchParams
ok(q2.get('destination_ids') === guardado.json?.id && q2.get('org_id') === orgId && q2.get('destinos') === '[]', 'o guardado vai só por id (destination_ids + org_id)', `${q2.get('destination_ids')} ${q2.get('org_id')} destinos=${q2.get('destinos')}`)
ok(!e2.recusa, 'o servidor aceita o destino guardado', e2.recusa ?? 'aceite')
await dormir(8000)
const c2 = sonda(`guardado/g${marca}`)
ok(c2.includes('h264'), 'o mediamtx recebe a emissão do destino guardado (chave decifrada no servidor)', c2.join(','))
await parar(e2)

// 3. guardado sem organização: a razão do servidor chega por inteiro
const e3 = await emitir('directo-sem-org', [{ id: guardado.json?.id, url: '', chave: '' }], null)
ok(e3.recusa === 'destination_ids exige org_id', 'sem org_id o servidor recusa e a razão chega ao cliente', e3.recusa)
await parar(e3)

// 4. dois destinos na mesma emissão (R230: só corrigido no PR backend/directo-multidestino)
const chave4 = `m${marca}`
const e4 = await emitir('directo-dois', [{ url: `${RTMP}/multi1`, chave: chave4 }, { url: `${RTMP}/multi2`, chave: chave4 }])
await dormir(8000)
const c4a = e4.recusa ? [] : sonda(`multi1/${chave4}`)
const c4b = e4.recusa ? [] : sonda(`multi2/${chave4}`)
console.log(`  · dois destinos: 1.º=${c4a.join(',') || '—'} 2.º=${c4b.join(',') || '—'} (recusa: ${e4.recusa ?? 'nenhuma'})`)
await parar(e4)

await browser.close()
console.log(falhas ? `\n=== ${falhas} FALHARAM ===` : '\n=== TUDO VERDE ===')
process.exit(falhas ? 1 : 0)
