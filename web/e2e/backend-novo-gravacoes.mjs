// Gravações pela interface contra o backend novo (#93): biblioteca, leitor, Estúdio e link público.
//
// Uma gravação REAL (webm com vídeo e áudio) é carregada na sala pela API, como
// o `seed-v2.mjs`; a transcrição entra por SQL (em produção é o ai-worker que a
// escreve). O resto é conduzido na interface, e cada passo confere o pedido e a
// resposta da API que a interface fez.
//
// Uso: API=http://127.0.0.1:8460 APP=http://127.0.0.1:5460 PG_CONTENTOR=uitpl-postgres PG_BASE=ui_api_nova \
//        node e2e/backend-novo-gravacoes.mjs
import { chromium } from '@playwright/test'
import { execFileSync, spawnSync } from 'node:child_process'
import { mkdirSync, readFileSync, statSync } from 'node:fs'
import { join } from 'node:path'
import { criarConta, entrar, PASSWORD } from './sessao.mjs'

const API = process.env.API ?? 'http://127.0.0.1:8460'
const APP = process.env.APP ?? 'http://127.0.0.1:5460'
const PG = process.env.PG_CONTENTOR ?? 'uitpl-postgres'
const BASE = process.env.PG_BASE ?? 'ui_api_nova'
const PASTA = join(process.cwd(), 'node_modules', '.cache', 'gravacoes-e2e')
mkdirSync(PASTA, { recursive: true })
let falhas = 0
const ok = (c, n, d) => {
  console.log(`  ${c ? '✓' : '✗'} ${n}${d !== undefined ? `  — ${d}` : ''}`)
  if (!c) falhas++
}
async function api(path, { token, method = 'GET', body, raw, type } = {}) {
  const r = await fetch(`${API}${path}`, {
    method,
    headers: { 'Content-Type': type ?? 'application/json', ...(token ? { Authorization: `Bearer ${token}` } : {}) },
    body: raw ?? (body ? JSON.stringify(body) : undefined),
  })
  const txt = await r.text()
  let json = null
  try { json = JSON.parse(txt) } catch { /* sem corpo */ }
  return { status: r.status, json, txt, type: r.headers.get('content-type') }
}

// ---------- montagem ----------
const dono = await criarConta(API, 'grv')
const sA = (await api('/api/auth/login', { method: 'POST', body: { email: dono.email, password: PASSWORD } })).json
const tA = sA.access_token
const orgId = (await api('/api/orgs', { token: tA })).json[0].id
const emailM = `membro${Date.now().toString(36)}@${dono.email.split('@')[1]}`
await api(`/api/orgs/${orgId}/members`, { token: tA, method: 'POST', body: { email: emailM, username: emailM.split('@')[0], password: PASSWORD } })
const tM = (await api('/api/auth/login', { method: 'POST', body: { email: emailM, password: PASSWORD } })).json.access_token
const sala = (await api('/api/rooms', { token: tA, method: 'POST', body: { name: 'aula gravada', topology: 'sfu' } })).json
await api(`/api/rooms/${sala.code}/join`, { token: tA, method: 'POST' })
const webm = join(PASTA, 'aula.webm')
spawnSync('ffmpeg', ['-v', 'error', '-y', '-f', 'lavfi', '-i', 'testsrc2=size=640x360:rate=25', '-f', 'lavfi', '-i', 'sine=frequency=440:sample_rate=48000',
  '-t', '12', '-c:v', 'libvpx', '-b:v', '600k', '-c:a', 'libopus', webm])
const up = await api(`/api/rooms/${sala.code}/recordings?name=${encodeURIComponent('Aula de redes.webm')}&kind=training`, {
  token: tA, method: 'POST', raw: readFileSync(webm), type: 'video/webm',
})
const rec = up.json
ok(up.status === 200 && rec?.id, 'upload real na sala', `${up.status} duration_ms=${rec?.duration_ms} ${rec?.width}x${rec?.height} has_thumbnail=${rec?.has_thumbnail}`)
const segs = [
  { start_ms: 0, end_ms: 4000, text: 'Bem-vindos à aula de redes.', confidence: 0.9 },
  { start_ms: 4000, end_ms: 9000, text: 'Hoje falamos de encaminhamento.', confidence: 0.88 },
]
execFileSync('docker', ['exec', PG, 'psql', '-U', 'delonix', '-d', BASE, '-c',
  `update recordings set transcript = 'Bem-vindos à aula de redes. Hoje falamos de encaminhamento.', transcript_segments = '${JSON.stringify(segs)}'::jsonb, transcript_language = 'pt', transcribed_at = now() where id = '${rec.id}'`], { stdio: 'pipe' })

const browser = await chromium.launch()
const erros = []
async function pagina(nome, conta) {
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, locale: 'pt-PT', acceptDownloads: true })
  const page = await ctx.newPage()
  const pedidos = []
  page.on('response', (r) => {
    const u = new URL(r.url())
    if (!u.pathname.startsWith('/api/')) return
    const linha = { m: r.request().method(), p: u.pathname, q: u.search, s: r.status(), t: r.headers()['content-type'] ?? '' }
    pedidos.push(linha)
    if (r.status() >= 400) erros.push(`${nome} ${linha.m} ${u.pathname.replace(/[0-9a-f]{8}-[0-9a-f-]{27}/g, '{id}')} → ${r.status()}`)
  })
  if (conta) await entrar(page, APP, conta)
  return { page, pedidos }
}
const viu = (p, m, re, s) => p.pedidos.find((x) => x.m === m && re.test(x.p) && (s === undefined || x.s === s))
const esperarPedido = (p, m, re) =>
  p.page.waitForResponse((r) => r.request().method() === m && re.test(new URL(r.url()).pathname), { timeout: 20000 }).catch(() => null)
const RID = rec.id

// ---------- biblioteca ----------
console.log('· biblioteca')
const A = await pagina('A', dono)
await A.page.goto(`${APP}/#/recordings`, { waitUntil: 'domcontentloaded' })
await A.page.getByRole('button', { name: /abrir aula de redes/i }).first().waitFor({ timeout: 30000 })
await A.page.waitForTimeout(2000)
const miniatura = viu(A, 'GET', new RegExp(`/api/recordings/${RID}/thumbnail$`))
if (rec.has_thumbnail) ok(miniatura?.s === 200 && miniatura.t.includes('image/jpeg'), 'a miniatura do servidor aparece na biblioteca (jpeg)', miniatura && `${miniatura.s} ${miniatura.t}`)
else ok(!miniatura, 'sem miniatura no servidor, a biblioteca não a pede (fica o tom por nome)')
const linha = await A.page.locator('.rec-row').filter({ hasText: 'Aula de redes' }).first().innerText()
ok(/00:1[12]/.test(linha) && linha.includes('360p'), 'duração e resolução medidas pelo servidor na lista', linha.replace(/\s+/g, ' ').slice(0, 120))

// ---------- leitor: toca, descarrega, visualização ----------
console.log('· leitor')
const content = esperarPedido(A, 'GET', new RegExp(`/api/recordings/${RID}/content$`))
await A.page.goto(`${APP}/#/recordings/${RID}`, { waitUntil: 'domcontentloaded' })
const c = await content
ok(c?.status() === 200 && (c.headers()['content-type'] ?? '').startsWith('video/webm'), 'o leitor pede …/content e recebe video/webm, não JSON', c && `${c.status()} ${c.headers()['content-type']}`)
const tocou = await A.page.waitForFunction(() => { const v = document.querySelector('video.pl-video__el'); return v && v.readyState >= 2 && v.currentTime > 0.5 }, null, { timeout: 30000 }).then(() => true, () => false)
const t0 = await A.page.evaluate(() => document.querySelector('video.pl-video__el')?.currentTime ?? 0)
ok(tocou, 'o vídeo toca (readyState ≥ 2, currentTime a avançar)', `currentTime=${t0.toFixed(2)}`)
await A.page.waitForTimeout(1500)
ok(viu(A, 'POST', new RegExp(`/api/recordings/${RID}/views$`), 204), 'tocar conta uma visualização (POST …/views → 204)')
const [dl] = await Promise.all([
  A.page.waitForEvent('download', { timeout: 20000 }).catch(() => null),
  A.page.getByRole('button', { name: /^descarregar$/i }).click(),
])
const dlPath = dl ? await dl.path() : null
const magic = dlPath ? readFileSync(dlPath).subarray(0, 4).toString('hex') : ''
ok(!!dl && magic === '1a45dfa3' && statSync(dlPath).size === rec.size_bytes, 'descarregar entrega o ficheiro webm inteiro', dl && `${dl.suggestedFilename()} ${statSync(dlPath).size} B, EBML=${magic}`)
ok(A.pedidos.some((x) => x.p === `/api/recordings/${RID}/content` && x.q === '?dl=1' && x.s === 200 && x.t.startsWith('video/webm')), 'o download vai por …/content?dl=1 (200 video/webm)')

// ---------- transcrição e participantes ----------
await A.page.getByRole('tab', { name: /^transcrição/i }).click()
ok(await A.page.getByText('Hoje falamos de encaminhamento.').first().waitFor({ timeout: 15000 }).then(() => true, () => false), 'a transcrição do servidor (inserida por SQL) aparece no leitor com tempos')
ok(viu(A, 'GET', new RegExp(`/api/recordings/${RID}/transcript$`), 200), 'GET …/transcript 200')
await A.page.getByRole('tab', { name: /^participantes/i }).click()
const nomeA = sA.user.username
ok(await A.page.locator('.pl-people').getByText(nomeA).first().waitFor({ timeout: 15000 }).then(() => true, () => false), 'participantes: quem entrou na sala aparece', nomeA)

// ---------- editar ----------
console.log('· editar')
await A.page.getByRole('button', { name: /^editar$/i }).click()
await A.page.fill('#pl-edit-name', 'Redes — aula 1')
await A.page.fill('#pl-edit-desc', 'Encaminhamento estático e dinâmico.')
await A.page.fill('#pl-edit-tags', '#Redes, ospf')
const patch = esperarPedido(A, 'PATCH', new RegExp(`/api/recordings/${RID}$`))
await A.page.locator('.dx-dialog button[type=submit]').click()
ok((await patch)?.status() === 200, 'guardar → PATCH …/{id} 200')
await A.page.getByRole('heading', { name: 'Redes — aula 1' }).waitFor({ timeout: 15000 }).catch(() => {})
const item = (await api(`/api/recordings/${RID}`, { token: tA })).json
ok(item.filename === 'Redes — aula 1' && item.description.startsWith('Encaminhamento') && JSON.stringify(item.tags) === '["redes","ospf"]', 'nome, descrição e etiquetas gravados', `${item.filename} | ${JSON.stringify(item.tags)}`)

// ---------- publicar ----------
console.log('· publicar')
const antes = (await api('/api/recordings?scope=published', { token: tM })).json
ok(!antes.some((r) => r.id === RID), 'antes: o membro não a vê em scope=published')
let resp = esperarPedido(A, 'PUT', new RegExp(`/api/recordings/${RID}/publication$`))
await A.page.getByRole('button', { name: /publicar na organização/i }).click()
ok((await resp)?.status() === 200, 'publicar → PUT …/publication 200')
const depois = (await api('/api/recordings?scope=published', { token: tM })).json
ok(depois.some((r) => r.id === RID && r.state === 'published'), 'o membro vê-a em scope=published', `${depois.length} publicada(s)`)
const M = await pagina('M', { email: emailM, password: PASSWORD })
await M.page.goto(`${APP}/#/recordings?scope=published`, { waitUntil: 'domcontentloaded' })
ok(await M.page.getByRole('button', { name: /abrir redes — aula 1/i }).first().waitFor({ timeout: 20000 }).then(() => true, () => false), 'na biblioteca do membro, «Publicadas» mostra-a')
await M.page.goto(`${APP}/#/recordings/${RID}`, { waitUntil: 'domcontentloaded' })
ok(await M.page.getByRole('heading', { name: 'Redes — aula 1' }).waitFor({ timeout: 20000 }).then(() => true, () => false), 'o membro abre o leitor de uma publicada onde não participou')
ok((await M.page.getByRole('button', { name: /^editar$/i }).count()) === 0, 'o membro não vê «Editar» (can_manage=false)')
await A.page.waitForTimeout(800)
resp = esperarPedido(A, 'DELETE', new RegExp(`/api/recordings/${RID}/publication$`))
await A.page.getByRole('button', { name: /^despublicar$/i }).click()
ok((await resp)?.status() === 204, 'despublicar → DELETE …/publication 204')
ok(!(await api('/api/recordings?scope=published', { token: tM })).json.some((r) => r.id === RID), 'o membro deixa de a ver em scope=published')

// ---------- capítulos ----------
console.log('· capítulos')
const ed = A.page.locator('.pl-chapter-edit')
await ed.getByLabel('Instante (mm:ss)').last().fill('00:02')
await ed.getByLabel('Título do capítulo').last().fill('Introdução')
resp = esperarPedido(A, 'POST', new RegExp(`/api/recordings/${RID}/chapters$`))
await ed.getByRole('button', { name: 'Adicionar' }).click()
ok((await resp)?.status() === 201, 'criar capítulo → POST 201')
await ed.getByText('Introdução').first().waitFor({ timeout: 10000 })
await ed.getByLabel('Instante (mm:ss)').last().fill('00:02')
await ed.getByLabel('Título do capítulo').last().fill('Repetido')
resp = esperarPedido(A, 'POST', new RegExp(`/api/recordings/${RID}/chapters$`))
await ed.getByRole('button', { name: 'Adicionar' }).click()
ok((await resp)?.status() === 409, 'mesmo instante → 409')
ok(await ed.getByText('Já há um capítulo nesse instante.').waitFor({ timeout: 5000 }).then(() => true, () => false), 'o 409 recording.chapter_timestamp_taken aparece por extenso')
await ed.getByRole('button', { name: 'Editar «Introdução»' }).click()
await ed.getByLabel('Título do capítulo').first().fill('Introdução às redes')
await ed.getByLabel('Instante (mm:ss)').first().fill('00:03')
resp = esperarPedido(A, 'PATCH', new RegExp(`/api/recordings/${RID}/chapters/[^/]+$`))
await ed.getByRole('button', { name: 'Guardar' }).click()
ok((await resp)?.status() === 200, 'editar capítulo → PATCH 200')
const caps = (await api(`/api/recordings/${RID}/chapters`, { token: tA })).json
ok(caps.length === 1 && caps[0].title === 'Introdução às redes' && caps[0].t_ms === 3000, 'título e instante novos no servidor', JSON.stringify(caps.map((x) => [x.t_ms, x.title])))
await ed.getByRole('button', { name: 'Apagar «Introdução às redes»' }).waitFor({ timeout: 10000 })
resp = esperarPedido(A, 'DELETE', new RegExp(`/api/recordings/${RID}/chapters/[^/]+$`))
await ed.getByRole('button', { name: 'Apagar «Introdução às redes»' }).click()
ok((await resp)?.status() === 204, 'apagar capítulo → DELETE 204')

// ---------- comentários ----------
console.log('· comentários')
await A.page.evaluate(() => { const v = document.querySelector('video.pl-video__el'); if (v) { v.pause(); v.currentTime = 5 } })
await A.page.waitForTimeout(500)
await A.page.getByRole('tab', { name: /^comentários/i }).click()
await A.page.getByLabel('Novo comentário').fill('Bom exemplo de OSPF aqui.')
resp = esperarPedido(A, 'POST', new RegExp(`/api/recordings/${RID}/comments$`))
await A.page.getByRole('button', { name: 'Comentar' }).click()
const rc = await resp
const corpo = rc ? await rc.json().catch(() => null) : null
ok(rc?.status() === 201 && corpo?.t_ms >= 4500 && corpo?.t_ms <= 6000 && corpo?.can_delete === true, 'comentário no instante → POST 201 com t_ms', corpo && `t_ms=${corpo.t_ms}`)
await A.page.locator('.pl-comment').getByText('Bom exemplo de OSPF aqui.').waitFor({ timeout: 10000 })
resp = esperarPedido(A, 'DELETE', new RegExp(`/api/recordings/${RID}/comments/[^/]+$`))
await A.page.getByRole('button', { name: 'Apagar comentário' }).first().click()
ok((await resp)?.status() === 204, 'apagar o próprio comentário → DELETE 204')

// ---------- legendas ----------
console.log('· legendas')
const vtt = 'WEBVTT\n\n00:00:00.000 --> 00:00:04.000\nBem-vindos à aula de redes.\n\n00:00:04.000 --> 00:00:09.000\nHoje falamos de encaminhamento.\n'
await A.page.getByRole('tab', { name: /^legendas/i }).click()
await A.page.getByLabel(/língua/i).fill('pt')
await A.page.getByLabel('Ficheiro WebVTT').setInputFiles({ name: 'pt.vtt', mimeType: 'text/vtt', buffer: Buffer.from(vtt) })
resp = esperarPedido(A, 'PUT', new RegExp(`/api/recordings/${RID}/captions/pt$`))
// Publicada, a língua entra em caption_languages e o leitor vai buscar o VTT para a faixa.
const vttResp = esperarPedido(A, 'GET', new RegExp(`/api/recordings/${RID}/captions/pt/vtt$`))
await A.page.getByRole('button', { name: 'Enviar legenda' }).click()
ok((await resp)?.status() === 201, 'enviar VTT → PUT …/captions/pt 201 (criou)')
await A.page.locator('.pl-captions__list').getByText('Publicada').waitFor({ timeout: 10000 })
const vr = await vttResp
ok(vr?.status() === 200, 'o leitor vai buscar o VTT publicado (GET …/vtt 200) para a faixa', vr && `${vr.status()} ${vr.headers()['content-type']}`)
const botaoCc = A.page.getByRole('button', { name: 'Ligar legendas (pt)' })
ok(await botaoCc.waitFor({ timeout: 15000 }).then(() => true, () => false), 'aparece o botão de legendas no leitor')
await botaoCc.click()
const cues = await A.page
  .waitForFunction(() => { const tr = document.querySelector('video.pl-video__el')?.textTracks?.[0]; return tr && tr.mode === 'showing' && tr.cues && tr.cues.length > 0 ? tr.cues.length : false }, null, { timeout: 15000 })
  .then((h) => h.jsonValue(), () => 0)
ok(cues === 2, 'ligar legendas mostra a faixa pt com as 2 deixas do VTT', `cues=${cues}`)
await A.page.getByRole('tab', { name: /^legendas/i }).click()
resp = esperarPedido(A, 'DELETE', new RegExp(`/api/recordings/${RID}/captions/pt$`))
await A.page.getByRole('button', { name: 'Apagar a legenda pt' }).click()
ok((await resp)?.status() === 204, 'apagar legenda → DELETE 204')
ok((await api(`/api/recordings/${RID}/captions`, { token: tA })).json.length === 0, 'a lista de legendas fica vazia no servidor')

// ---------- estúdio ----------
console.log('· estúdio')
await A.page.goto(`${APP}/#/recordings/${RID}`, { waitUntil: 'domcontentloaded' })
await A.page.getByRole('button', { name: /editar no studio/i }).click()
const noEditor = await A.page.waitForFunction(() => location.hash.startsWith('#/studio'), null, { timeout: 10000 }).then(() => true, () => false)
const abriu = esperarPedido(A, 'GET', new RegExp(`/api/recordings/${RID}/content$`))
ok(noEditor && (await abriu)?.status() === 200, '«Editar no Studio» abre o editor e descarrega o ficheiro por …/content')
await A.page.locator('[data-studio-vista="legendas"]').first().click({ timeout: 60000 })
ok(await A.page.locator('[data-studio="usar-transcricao-servidor"]').waitFor({ timeout: 30000 }).then(() => true, () => false), 'nas Legendas do Estúdio aparece a transcrição do servidor')
ok(viu(A, 'GET', new RegExp(`/api/recordings/${RID}/transcript$`), 200), 'o Estúdio lê GET …/transcript 200')

// ---------- link público ----------
console.log('· link público')
await A.page.goto(`${APP}/#/recordings/${RID}`, { waitUntil: 'domcontentloaded' })
await A.page.getByRole('button', { name: /^partilhar/i }).click()
resp = esperarPedido(A, 'PUT', new RegExp(`/api/recordings/${RID}/public-link$`))
await A.page.getByRole('button', { name: 'Gerar link' }).click()
ok((await resp)?.status() === 200, 'gerar link → PUT …/public-link 200')
const link = await A.page.getByRole('textbox', { name: 'Link público' }).inputValue()
const P = await pagina('público', null)
const pubContent = esperarPedido(P, 'GET', /\/api\/public\/recordings\/[^/]+\/content$/)
await P.page.goto(link, { waitUntil: 'domcontentloaded' })
const pc = await pubContent
ok(pc && [200, 206].includes(pc.status()) && (pc.headers()['content-type'] ?? '').startsWith('video/webm'), 'sem sessão, a página pública pede …/content e recebe video/webm', pc && `${pc.status()} ${pc.headers()['content-type']}`)
const tocouPub = await P.page.waitForFunction(() => { const v = document.querySelector('video.pub-partilha__video'); return v && v.readyState >= 1 && v.duration > 0 }, null, { timeout: 20000 }).then(() => true, () => false)
await P.page.evaluate(() => document.querySelector('video.pub-partilha__video')?.play().catch(() => {}))
await P.page.waitForTimeout(2500)
const tPub = await P.page.evaluate(() => document.querySelector('video.pub-partilha__video')?.currentTime ?? 0)
ok(tocouPub && tPub > 0.5, 'o vídeo público toca sem sessão', `currentTime=${tPub.toFixed(2)}`)
ok(!P.pedidos.some((x) => x.s === 401), 'nenhum 401 na página pública')

console.log('\n· respostas ≥ 400 da API (biblioteca, leitor, estúdio, página pública):')
for (const e of [...new Set(erros)]) console.log(`    ${e}`)
await browser.close()
console.log(falhas ? `\n=== ${falhas} FALHARAM ===` : '\n=== TUDO VERDE ===')
process.exit(falhas ? 1 : 0)
