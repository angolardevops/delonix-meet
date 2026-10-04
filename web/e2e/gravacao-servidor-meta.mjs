// O GRAVADOR DO SERVIDOR de ponta a ponta, com media a sério (Chromium com
// câmara e microfone falsos): reunião agendada com opções → sala → gravação
// → `processing` com progresso → `ready` → metadados medidos → `recording.ready`.
//
// Três cenários — os dois primeiros provam uma opção da reunião que antes não
// existia, o terceiro o `recording.ready` que o upload não enviava:
//   1. `auto_record` + formato `hybrid`: ninguém carrega em «gravar»; o
//      servidor arranca quando o anfitrião entra, e a gravação sai com
//      `kind = hybrid`, duração/resolução medidas e miniatura.
//   2. `record_quality = audio`: o anfitrião grava à mão e o ficheiro sai SEM
//      vídeo (`video_codec = null`, `audio_codec = opus`).
//   3. upload de um webm 720p com `kind=broadcast`: metadados na resposta e
//      `recording.ready` com `source = upload`.
//
// Precisa do servidor com ffmpeg/ffprobe e `WEBHOOK_ALLOW_HOSTS=127.0.0.1`
// (o webhook entrega-se a um receptor local deste teste) e do vite a servir
// o arnês. Uso:
//   API=http://127.0.0.1:8180 APP=http://localhost:5174 node e2e/gravacao-servidor-meta.mjs
import { chromium } from '@playwright/test'
import { execFileSync } from 'node:child_process'
import { mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { createServer } from 'node:http'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

const API = process.env.API ?? 'http://127.0.0.1:8180'
const APP = process.env.APP ?? 'http://localhost:5174'
const PW = 'UmaPasswordForte123!'
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
let falhas = 0
const chk = (c, n) => { console.log(`  ${c ? '✓' : '✗'} ${n}`); if (!c) falhas++ }

const j = (path, o = {}) =>
  fetch(`${API}${path}`, {
    method: o.method ?? 'GET',
    headers: { ...(o.token ? { Authorization: `Bearer ${o.token}` } : {}), ...(o.body ? { 'Content-Type': 'application/json' } : {}) },
    ...(o.body ? { body: JSON.stringify(o.body) } : {}),
  }).then(async (r) => ({ s: r.status, j: await r.json().catch(() => null) }))

// Receptor do webhook `generic`.
const recebidos = []
const receptor = createServer((req, res) => {
  let b = ''
  req.on('data', (c) => { b += c })
  req.on('end', () => { try { recebidos.push(JSON.parse(b)) } catch { /* ignora */ } res.end('ok') })
})
await new Promise((r) => receptor.listen(0, '127.0.0.1', r))
const portaReceptor = receptor.address().port

const m = Math.random().toString(36).slice(2, 7)
const email = `gs${m}@gs${m}.local`
await j('/api/auth/register', { method: 'POST', body: { org_name: `GS ${m}`, email, username: `gs${m}`, password: PW } })
const login = await j('/api/auth/login', { method: 'POST', body: { email, password: PW } })
const tok = login.j.access_token
const orgId = (await j('/api/orgs', { token: tok })).j[0].id
const hook = await j(`/api/orgs/${orgId}/webhooks`, {
  token: tok, method: 'POST',
  body: { kind: 'generic', url: `http://127.0.0.1:${portaReceptor}/hook`, secret: 's', events: 'recording.ready' },
})
chk(hook.s === 200, `webhook generic registado → ${hook.s} ${hook.j?.error ?? ''}`)

async function reuniao(opcoes) {
  const r = await j('/api/meetings', {
    token: tok, method: 'POST',
    body: { title: `gravação ${JSON.stringify(opcoes)}`, kind: 'video', starts_at: new Date(Date.now() + 60e3).toISOString(), ...opcoes },
  })
  const st = await j(`/api/meetings/${r.j.id}/start`, { token: tok, method: 'POST' })
  const jr = await j(`/api/rooms/${st.j.code}/join`, { token: tok, method: 'POST' })
  return { code: st.j.code, roomToken: jr.j.room_token }
}

async function sessao(code, roomToken, { gravarAMao, segundos }) {
  const b = await chromium.launch({ args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'] })
  const p = await (await b.newContext({ ignoreHTTPSErrors: true })).newPage()
  await p.goto(`${APP}/e2e/harness.html?token=${encodeURIComponent(roomToken)}&code=${code}&access=${encodeURIComponent(tok)}`, { waitUntil: 'domcontentloaded', timeout: 120000 })
  for (let k = 0; k < 40; k++) { await sleep(500); if (await p.evaluate(() => window.__dlx.ready)) break }
  chk(await p.evaluate(() => window.__dlx.ready), `a sala ${code} ligou com media`)
  if (gravarAMao) await p.evaluate(() => window.__dlx.gravar(true))
  await sleep(segundos * 1000)
  if (gravarAMao) await p.evaluate(() => window.__dlx.gravar(false))
  await b.close()
}

/** Espera pela gravação da sala; regista os estados por que passou. */
async function acompanhar(code) {
  const vistos = new Set()
  let item = null
  const ate = Date.now() + 90_000
  while (Date.now() < ate) {
    const lib = (await j('/api/recordings', { token: tok })).j ?? []
    item = lib.find((r) => r.room_code === code)
    if (item) {
      vistos.add(item.status)
      if (item.status === 'processing' && item.progress_pct !== null) vistos.add('progresso')
      if (item.status !== 'processing') break
    }
    await sleep(80)
  }
  return { item, vistos }
}

console.log('\n--- 1. gravação automática, formato híbrido ---')
const r1 = await reuniao({ format: 'hybrid', auto_record: true, record_quality: '1080p' })
await sessao(r1.code, r1.roomToken, { gravarAMao: false, segundos: 8 })
const a1 = await acompanhar(r1.code)
chk(!!a1.item, 'a sala gravou sem ninguém carregar em «gravar»')
if (a1.item) {
  chk(a1.vistos.has('processing') || a1.item.status === 'ready', `estados vistos: ${[...a1.vistos].join(' → ')}`)
  if (!a1.vistos.has('processing')) console.log('    · a composição acabou antes da primeira sondagem — `processing` não foi observado desta vez')
  chk(a1.item.status === 'ready', `terminou em ready → ${a1.item.status} ${a1.item.failure_reason ?? ''}`)
  chk(a1.item.kind === 'hybrid', `kind vem do formato da reunião → ${a1.item.kind}`)
  // A medição corre depois do `ready`: espera-se por ela.
  let d = a1.item
  for (let k = 0; k < 50 && d.duration_ms === null; k++) { await sleep(200); d = (await j(`/api/recordings/${a1.item.id}/details`, { token: tok })).j }
  chk(d.duration_ms > 3000 && d.duration_ms < 20000, `duração medida ${d.duration_ms} ms`)
  chk(d.width > 0 && d.height > 0 && d.height <= 1080, `resolução medida ${d.width}×${d.height}`)
  chk(d.video_codec && d.audio_codec, `codecs ${d.video_codec}/${d.audio_codec}`)
  chk(d.has_thumbnail, 'miniatura gerada')
  const dl = await fetch(`${API}/api/recordings/${a1.item.id}/content?dl=1`, { headers: { Authorization: `Bearer ${tok}` } })
  chk(dl.ok && Number(dl.headers.get('content-length') ?? (await dl.arrayBuffer()).byteLength) > 1000, 'o ficheiro descarrega')
  for (let k = 0; k < 25 && !recebidos.some((w) => w.data?.recording_id === a1.item.id); k++) await sleep(200)
  const w = recebidos.find((x) => x.data?.recording_id === a1.item.id)
  chk(w?.event === 'recording.ready' && w.data.source === 'server' && w.data.kind === 'hybrid', `recording.ready do gravador → source=${w?.data?.source}`)
  chk(w?.data?.duration_ms > 0 && w.data.height > 0, 'e traz os metadados medidos')
}

console.log('\n--- 2. qualidade «só áudio», gravação manual ---')
const r2 = await reuniao({ format: 'meeting', record_quality: 'audio' })
await sessao(r2.code, r2.roomToken, { gravarAMao: true, segundos: 6 })
const a2 = await acompanhar(r2.code)
chk(a2.item?.status === 'ready', `terminou em ready → ${a2.item?.status} ${a2.item?.failure_reason ?? ''}`)
if (a2.item) {
  let d = a2.item
  for (let k = 0; k < 50 && d.duration_ms === null; k++) { await sleep(200); d = (await j(`/api/recordings/${a2.item.id}/details`, { token: tok })).j }
  chk(d.video_codec === null && d.audio_codec === 'opus', `sem vídeo: ${d.video_codec}/${d.audio_codec}`)
  chk(d.has_thumbnail === false && d.width === null, 'sem miniatura nem resolução — não se inventam')
  chk(d.kind === 'meeting', `kind=${d.kind}`)
}

console.log('\n--- 3. recording.ready também no UPLOAD ---')
const dir = mkdtempSync(join(tmpdir(), 'dlx-gs-'))
execFileSync('ffmpeg', ['-y', '-loglevel', 'error', '-f', 'lavfi', '-i', 'testsrc=size=1280x720:rate=30',
  '-f', 'lavfi', '-i', 'sine', '-t', '2', '-c:v', 'libvpx', '-c:a', 'libopus', '-live', '1', '-f', 'webm', join(dir, 'e.webm')], { stdio: 'ignore' })
const up = await fetch(`${API}/api/rooms/${r2.code}/recordings?name=estudio.webm&kind=broadcast`, {
  method: 'POST', headers: { Authorization: `Bearer ${tok}` }, body: readFileSync(join(dir, 'e.webm')),
})
rmSync(dir, { recursive: true, force: true })
const upj = await up.json()
chk(up.ok && upj.height === 720 && upj.kind === 'broadcast', `upload medido na resposta → ${upj.width}×${upj.height} ${upj.kind}`)
for (let k = 0; k < 25 && !recebidos.some((w) => w.data?.recording_id === upj.id); k++) await sleep(200)
const wu = recebidos.find((x) => x.data?.recording_id === upj.id)
chk(wu?.event === 'recording.ready' && wu.data.source === 'upload' && wu.data.height === 720, `recording.ready do upload → source=${wu?.data?.source} height=${wu?.data?.height}`)

receptor.close()
console.log(falhas ? `\n${falhas} FALHARAM` : '\ntudo verde')
process.exit(falhas ? 1 : 0)
