// Leitor em página inteira com a API SIMULADA por interceptação de rede
// (o servidor de validação 8190 estava em baixo). Prova layout e comportamento
// do cliente com respostas no formato real de api.ts; NÃO prova o servidor.
import { chromium } from '@playwright/test'
import { readFileSync } from 'node:fs'
const DIR = new URL('./', import.meta.url).pathname
const APP = process.env.APP ?? 'http://127.0.0.1:5505'
const MOBILE = process.argv.includes('--mobile')
let falhas = 0
const ok = (n, c, d = '') => { console.log(`${c ? '  ok  ' : ' FALHA'}  ${n}${d ? '  — ' + d : ''}`); if (!c) falhas++ }
const ids = ['6f1c2a34-9b8d-4e7f-a1b2-c3d4e5f6a7b8', '7a2d3b45-0c9e-4f80-b2c3-d4e5f6a7b8c9', '8b3e4c56-1d0f-4091-83d4-e5f6a7b8c9d0', '9c4f5d67-2e1a-41a2-94e5-f6a7b8c9d0e1']
const rec = (i, name, room, who) => ({ id: ids[i], room_id: 'r', uploader_id: 'u', filename: `${name}.webm`, size_bytes: 5909414 * (i + 1), created_at: `2026-09-1${i}T10:00:00Z`, room_code: room, uploader_name: who, owned: i !== 2, share_count: i === 0 ? 2 : 0, can_download: true, status: 'ready', failure_reason: null })
const library = [
  rec(0, 'Formação — Arquitectura de Voz Delonix · sessão 3', 'voz-tres-sala', 'Ana Mbala'),
  rec(1, 'Arquitectura de Voz — sessão 2', 'voz-dois-sala', 'Ana Mbala'),
  rec(2, 'Onboarding RH — Setembro', 'rh-set-sala', 'Luísa Cardoso'),
  rec(3, 'Aula aberta — Numeração +244', 'num-aula-sala', 'Teresa Kiala'),
]
const meeting = (id, title, room, parent, freq, desc = '') => ({ id, owner_id: 'o', owner_name: 'Ana Mbala', title, description: desc, kind: 'video', starts_at: '2026-09-11T09:00:00Z', duration_min: 60, room_code: room, is_owner: true, recurrence_parent_id: parent, recurrence_freq: freq })
const meetings = [
  meeting('m1', 'Formação — Arquitectura de Voz', 'voz-um-sala', null, 'weekly'),
  meeting('m2', 'Formação — Arquitectura de Voz · sessão 2', 'voz-dois-sala', 'm1', null),
  meeting('m3', 'Formação — Arquitectura de Voz · sessão 3', 'voz-tres-sala', 'm1', null, 'Terceira sessão da formação interna sobre a camada de voz: encaminhamento SIP, failover entre SBC e gravação em 4K.'),
]
const boards = [{ id: 'b1b1b1b1-0000-4000-8000-000000000001', title: 'Plano de numeração', room_code: 'voz-tres-sala', is_public: false, share_token: '', created_at: '2026-09-11T10:20:00Z' }]
const chat = [{ id: 'c1', user_id: 'u', username: 'Joaquim Ferreira', message: 'Este trecho devia entrar no material de onboarding.', created_at: '2026-09-11T10:18:22Z' }, { id: 'c2', user_id: 'u', username: 'Teresa Kiala', message: 'Corrijo o número de porta no diapositivo 19.', created_at: '2026-09-11T10:25:04Z' }]
const notes = { title: 'Formação', minutes: '- [ ] Rever failover\n- [x] Publicar plano', transcript: '[10:02] Ana Mbala: Bom dia a todos.\n[10:18] Joaquim Ferreira: O failover foi corrigido na versão 2.4.' }
const webm = readFileSync(`${DIR}amostra.webm`)
const png = readFileSync(`${DIR}quadro.png`)

const b = await chromium.launch()
const ctx = await b.newContext({ viewport: MOBILE ? { width: 390, height: 844 } : { width: 1440, height: 900 } })
await ctx.addInitScript(() => {
  localStorage.setItem('dx_user', JSON.stringify({ id: '00000000-0000-0000-0000-000000000001', email: 'demo@delonix.co.ao', username: 'Ana Mbala' }))
  localStorage.setItem('dx_access', 'falso'); localStorage.setItem('dx_tour_v1', 'done')
})
const pedidos = []
await ctx.route('**/api/**', (route) => {
  const u = new URL(route.request().url()); const path = u.pathname
  pedidos.push(path)
  const json = (d) => route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(d) })
  if (path === '/api/recordings') return json(library)
  if (/^\/api\/recordings\/[0-9a-f-]{36}$/.test(path)) return route.fulfill({ status: 200, contentType: 'video/webm', body: webm })
  if (path === '/api/meetings') return json(meetings)
  if (path === '/api/whiteboards') return json(boards)
  if (/\/api\/whiteboards\/.+\/png$/.test(path)) return route.fulfill({ status: 200, contentType: 'image/png', body: png })
  if (/\/api\/rooms\/.+\/chat$/.test(path)) return json(path.includes('voz-tres') ? chat : [])
  if (/\/api\/rooms\/.+\/notes$/.test(path)) return path.includes('voz-tres') ? json(notes) : route.fulfill({ status: 404, body: '{}' })
  return route.fulfill({ status: 404, contentType: 'application/json', body: '{"error":"simulado"}' })
})
const p = await ctx.newPage()
p.on('pageerror', (e) => { console.log('pageerror', e.message); falhas++ })
await p.goto(`${APP}/#/recordings`)
await p.waitForTimeout(1500)
if (!MOBILE) {
  await p.getByRole('button', { name: 'Abrir em página inteira' }).first().click().catch(() => {})
  await p.waitForTimeout(500)
  ok('Gravações → «Abrir em página inteira» leva ao leitor', /#\/recordings\/[0-9a-f-]{36}$/.test(await p.evaluate(() => location.hash)), await p.evaluate(() => location.hash))
}
await p.goto(`${APP}/#/recordings/${ids[0]}`)
await p.waitForSelector('video', { timeout: 20000 })
await p.waitForFunction(() => { const v = document.querySelector('video'); return v && v.readyState >= 1 }, null, { timeout: 20000 })
ok('o vídeo carrega', true)
await p.waitForFunction(() => document.querySelectorAll('.pl-frame__img img').length >= 3, null, { timeout: 30000 }).catch(() => {})
const thumbs = await p.locator('.pl-frame__img img').count()
ok('miniaturas por tempo tiradas do ficheiro', thumbs >= 3, `${thumbs}`)
ok('Descrição vem da reunião da sala', await p.getByText('Terceira sessão da formação').isVisible())
ok('«A seguir» lista as outras gravações', (await p.locator('.pl-item').count()) === 3)
await p.getByRole('tab', { name: /Da mesma série/ }).click()
const serie = await p.locator('.pl-item').allTextContents()
ok('«Da mesma série» pela recorrência', serie.length === 1 && serie[0].includes('sessão 2'), JSON.stringify(serie))
await p.getByRole('tab', { name: /A seguir/ }).click()
await p.screenshot({ path: `${DIR}app/DelonixPlayer${MOBILE ? '-390' : ''}.png` })
await p.getByRole('tab', { name: 'Anexos' }).click()
await p.waitForSelector('.pl-board img', { timeout: 10000 }).catch(() => {})
ok('Anexos: quadro da sala e chat', (await p.locator('.pl-board').count()) === 1 && (await p.locator('.pl-chat li').count()) === 2)
await p.screenshot({ path: `${DIR}app/DelonixPlayer-anexos${MOBILE ? '-390' : ''}.png` })
await p.getByRole('tab', { name: 'Transcrição' }).click()
ok('Transcrição mostra as notas da sala', await p.getByText('O failover foi corrigido').first().waitFor({ timeout: 8000 }).then(() => true).catch(() => false))
// Saltar por miniatura
await p.locator('.pl-frame').nth(2).click()
await p.waitForTimeout(600)
const t = await p.evaluate(() => document.querySelector('video').currentTime)
ok('carregar numa miniatura salta o vídeo', t > 10, String(t))
if (MOBILE) { const sw = await p.evaluate(() => document.documentElement.scrollWidth); ok('sem scroll horizontal', sw <= 390, String(sw)) }
// Editar no Studio
await p.getByRole('button', { name: 'Editar no Studio' }).click()
ok('Editar no Studio navega pelo contrato', (await p.evaluate(() => location.hash)) === `#/studio?editar=${ids[0]}`)
// Seguinte
await p.goto(`${APP}/#/recordings/${ids[0]}`); await p.waitForSelector('video')
await p.getByRole('button', { name: 'Seguinte' }).click()
ok('Seguinte vai para a próxima da biblioteca', (await p.evaluate(() => location.hash)) === `#/recordings/${ids[1]}`)
await p.goto(`${APP}/#/recordings/00000000-0000-4000-8000-000000000000`); await p.waitForTimeout(1200)
ok('id desconhecido: «Gravação indisponível»', await p.getByText('Gravação indisponível').isVisible())
await b.close()
if (!MOBILE) {
  const b2 = await chromium.launch(); const cmp = await b2.newPage({ viewport: { width: 2896, height: 940 } })
  const img = (f) => 'data:image/png;base64,' + readFileSync(f).toString('base64')
  await cmp.setContent(`<body style="margin:0;background:#222;font:12px monospace;color:#ccc;display:flex;gap:16px"><div><div style="height:20px">TEMPLATE · DelonixPlayer</div><img src="${img(`${DIR}../../notas-ui-template/ref/DelonixPlayer.png`)}" width="1440" height="900"></div><div><div style="height:20px">APP (API simulada) · ${APP}</div><img src="${img(`${DIR}app/DelonixPlayer.png`)}" width="1440" height="900"></div></body>`)
  await cmp.screenshot({ path: `${DIR}lado-a-lado/DelonixPlayer.png` }); await b2.close()
}
console.log(falhas ? `\n${falhas} falha(s)` : '\ntudo ok')
process.exit(falhas ? 1 : 0)
