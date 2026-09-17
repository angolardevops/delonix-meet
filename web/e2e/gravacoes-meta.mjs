#!/usr/bin/env node
// Contrato do leitor de gravações e das opções de reunião, contra um servidor
// A SÉRIO (migrações 0040–0046).
//
// Cobre, com controlo positivo antes de cada recusa:
//   - upload de um webm real → duração, resolução, fps, codecs e miniatura
//     medidos pelo servidor (ffprobe), tipo de sessão, `recording.ready`;
//   - biblioteca com metadados, estados, contagens, organização do autor e
//     pesquisa `?q=` na transcrição;
//   - transcrição com segmentos, legendas por língua (VTT), capítulos
//     (automáticos e manuais), comentários com marca temporal, visualizações,
//     participantes, descrição/etiquetas e publicação para a organização;
//   - o isolamento: outra organização recebe 404 em TODAS as rotas novas, e um
//     colega da mesma organização só vê o que foi publicado;
//   - reuniões com formato, sala de espera, gravação automática, qualidade,
//     contagem de convidados, origem externa e resposta «tentativa».
//
// Duas coisas que este teste NÃO faz, e diz quando não as fez:
//   - Sem `ffmpeg` no sítio onde corre, o ficheiro enviado não é um webm real.
//     Sem `ffprobe` no SERVIDOR, os metadados vêm `null` e os asserts de
//     medição passam a «não verificado» (EXPECT_PROBE=1 torna-os obrigatórios).
//   - O ai-worker (GPU) não corre aqui: a transcrição é escrita na base com a
//     MESMA instrução SQL que `ai-worker/transcribe_worker.py::_mark_done` usa.
//     Isso prova a leitura do servidor, não o faster-whisper.
//   - Capítulos automáticos e tradução precisam do LLM local (`OLLAMA_URL`).
//     Sem ele, espera-se 503 e diz-se que o caminho do LLM não foi exercitado.
//
// Uso:  API=http://127.0.0.1:8180 PG=<contentor> PGDB=delonix_meet node e2e/gravacoes-meta.mjs
import { execFileSync } from 'node:child_process'
import { mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { sql } from './pg.mjs'

const API = process.env.API ?? 'http://127.0.0.1:8180'
const PGDB = process.env.PGDB ?? 'delonix_meet'
const EXPECT_PROBE = process.env.EXPECT_PROBE === '1'
const EXPECT_LLM = process.env.EXPECT_LLM === '1'
const PW = 'UmaPasswordForte123!'

let passou = 0
let falhou = 0
const naoVerificado = []
const ok = (n) => { passou++; console.log(`  ✓ ${n}`) }
const nok = (n, d) => { falhou++; console.log(`  ✗ ${n}\n      ${d}`) }
const chk = (c, n, d = '') => (c ? ok(n) : nok(n, d))
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

async function req(path, { token, method = 'GET', body, raw } = {}) {
  const r = await fetch(`${API}${path}`, {
    method,
    headers: {
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
      ...(body !== undefined && !raw ? { 'Content-Type': 'application/json' } : {}),
    },
    ...(body !== undefined ? { body: raw ? body : JSON.stringify(body) } : {}),
  })
  const ct = r.headers.get('content-type') ?? ''
  let json = null
  let text = null
  if (ct.includes('json')) json = await r.json().catch(() => null)
  else text = await r.text().catch(() => null)
  return { status: r.status, json, text, ct, location: r.headers.get('location') }
}

async function novaOrg(sufixo) {
  const email = `admin@${sufixo}.local`
  await req('/api/auth/register', {
    method: 'POST',
    body: { org_name: `Org ${sufixo}`, email, username: `admin-${sufixo}`, password: PW },
  })
  const l = await req('/api/auth/login', { method: 'POST', body: { email, password: PW } })
  const token = l.json?.access_token
  if (!token) throw new Error(`login falhou para ${email}: ${JSON.stringify(l.json)}`)
  const orgs = await req('/api/orgs', { token })
  return { email, token, orgId: orgs.json?.[0]?.id, userId: l.json.user.id, orgName: `Org ${sufixo}`, dominio: `${sufixo}.local` }
}

/** Tem de ser recusado SEM confirmar que existe: 404. */
async function escondido(nome, path, opts) {
  const r = await req(path, opts)
  chk(r.status === 404, `${nome} → ${r.status}`, `esperava 404, veio ${r.status}: ${JSON.stringify(r.json ?? r.text).slice(0, 140)}`)
}

const marca = Math.random().toString(36).slice(2, 8)
console.log(`\n=== Leitor de gravações e opções de reunião (marca ${marca}) ===\n`)

const A = await novaOrg(`gma${marca}`)
const B = await novaOrg(`gmb${marca}`)
// C: colega da org A que NÃO esteve na sala.
const emailC = `carla-${marca}@${A.dominio}`
const empC = await req(`/api/orgs/${A.orgId}/employees`, {
  token: A.token, method: 'POST',
  body: { email: emailC, username: `carla-${marca}`, password: PW, role: 'member', title: 'Formadora' },
})
const C = {
  userId: empC.json?.user_id,
  token: (await req('/api/auth/login', { method: 'POST', body: { email: emailC, password: PW } })).json?.access_token,
}
if (!C.token) throw new Error(`não criei a colega C: ${JSON.stringify(empC.json)}`)

// ---------------------------------------------------------------------------
console.log('--- upload de um webm e metadados medidos pelo servidor ---')
const sala = (await req('/api/rooms', { token: A.token, method: 'POST', body: { name: 'aula gravada', topology: 'sfu', format: 'training' } })).json
chk(sala?.format === 'training', `a sala nasce em formato training → ${sala?.format}`)
await req(`/api/rooms/${sala.code}/join`, { token: A.token, method: 'POST' })

let ficheiro
let real = false
const dir = mkdtempSync(join(tmpdir(), 'dlx-gm-'))
try {
  execFileSync('ffmpeg', ['-y', '-loglevel', 'error', '-f', 'lavfi', '-i', 'testsrc=size=640x360:rate=25',
    '-f', 'lavfi', '-i', 'sine=frequency=440', '-t', '3', '-c:v', 'libvpx', '-c:a', 'libopus',
    '-live', '1', '-f', 'webm', join(dir, 'aula.webm')], { stdio: 'ignore' })
  ficheiro = readFileSync(join(dir, 'aula.webm'))
  real = true
} catch {
  ficheiro = Buffer.from([0x1a, 0x45, 0xdf, 0xa3])
  naoVerificado.push('sem ffmpeg AQUI: o upload não é um webm real, nada de media foi medido')
} finally {
  rmSync(dir, { recursive: true, force: true })
}

const up = await req(`/api/rooms/${sala.code}/recordings?name=${encodeURIComponent('Aula de redes.webm')}&kind=training`, {
  token: A.token, method: 'POST', body: ficheiro, raw: true,
})
chk(up.status === 200 && up.json?.id, `upload → ${up.status}`, JSON.stringify(up.json))
const rec = up.json.id
chk(up.json.kind === 'training' && up.json.status === 'ready', `resposta traz kind=${up.json.kind} status=${up.json.status}`)
const medido = real && up.json.duration_ms !== null
if (medido) {
  chk(up.json.width === 640 && up.json.height === 360, `resolução medida ${up.json.width}×${up.json.height}`)
  chk(up.json.fps === 25, `fps medido ${up.json.fps}`)
  chk(up.json.duration_ms >= 2900 && up.json.duration_ms <= 3100, `duração medida ${up.json.duration_ms} ms (webm do browser, sem duração no cabeçalho)`)
  chk(up.json.video_codec === 'vp8' && up.json.audio_codec === 'opus', `codecs ${up.json.video_codec}/${up.json.audio_codec}`)
  chk(up.json.has_thumbnail === true, 'miniatura gerada')
} else if (EXPECT_PROBE) {
  nok('metadados medidos (EXPECT_PROBE=1)', `vieram null: ${JSON.stringify(up.json)}`)
} else {
  naoVerificado.push('metadados de media: o servidor não tem ffprobe/ffmpeg (ou o ficheiro não é real) — vieram null, como devem')
  chk(up.json.duration_ms === null && up.json.width === null, 'sem medição, os campos vêm null e não inventados')
}

const bad = await req(`/api/rooms/${sala.code}/recordings?kind=podcast`, { token: A.token, method: 'POST', body: ficheiro, raw: true })
chk(bad.status === 400, `kind desconhecido é recusado → ${bad.status}`)

const lib = await req('/api/recordings', { token: A.token })
const item = Array.isArray(lib.json) ? lib.json.find((r) => r.id === rec) : null
chk(!!item, 'a gravação está na biblioteca')
if (item) {
  chk(item.state === 'ready' && item.kind === 'training', `state=${item.state} kind=${item.kind}`)
  chk(item.transcript_status === 'none', `transcript_status=${item.transcript_status} antes do worker`)
  chk(item.participant_count === 1 && item.view_count === 0, `participant_count=${item.participant_count} view_count=${item.view_count}`)
  chk(item.uploader_org_id === A.orgId && item.uploader_org_name === A.orgName, `organização do autor ${item.uploader_org_name}`)
  chk(item.can_manage === true && item.visibility === 'private', `can_manage=${item.can_manage} visibility=${item.visibility}`)
  if (medido) chk(item.duration_ms === up.json.duration_ms && item.height === 360, 'a biblioteca devolve os metadados medidos')
}
if (medido) {
  const th = await req(`/api/recordings/${rec}/thumbnail`, { token: A.token })
  chk(th.status === 200 && th.ct === 'image/jpeg', `miniatura servida → ${th.status} ${th.ct}`)
}

// ---------------------------------------------------------------------------
console.log('\n--- transcrição (escrita como o ai-worker a escreve) e pesquisa ---')
const palavra = `ipam${marca}`
const segmentos = [
  { start_ms: 0, end_ms: 1200, text: 'Bom dia a todos', confidence: 0.93 },
  { start_ms: 1300, end_ms: 2500, text: `hoje falamos de ${palavra} e sub-redes`, confidence: 0.81 },
]
const t0 = await req(`/api/recordings/${rec}/transcript`, { token: A.token })
chk(t0.status === 200 && t0.json?.status === 'none' && t0.json.segments.length === 0, `antes: status=${t0.json?.status}`)
const lit = (s) => `'${String(s).replace(/'/g, "''")}'`
sql(`UPDATE recordings SET transcript = ${lit(segmentos.map((s) => s.text).join(' '))}, minutes = '',
       transcribed_at = now(), transcript_segments = ${lit(JSON.stringify(segmentos))}::jsonb,
       transcript_language = 'pt', transcript_confidence = 0.87, transcript_error = NULL,
       status = CASE WHEN status = 'transcribing' THEN 'ready' ELSE status END,
       progress_pct = NULL, progress_at = NULL
     WHERE id = '${rec}'`, { db: PGDB })
const t1 = await req(`/api/recordings/${rec}/transcript`, { token: A.token })
chk(t1.json?.status === 'ready' && t1.json.language === 'pt' && t1.json.segments.length === 2, `depois: status=${t1.json?.status} língua=${t1.json?.language} segmentos=${t1.json?.segments?.length}`)
chk(t1.json?.segments?.[1]?.start_ms === 1300 && t1.json.segments[1].confidence > 0.8, 'segmentos com tempos e confiança')
const achou = await req(`/api/recordings?q=${palavra}`, { token: A.token })
chk(Array.isArray(achou.json) && achou.json.some((r) => r.id === rec), 'pesquisa ?q= encontra pela TRANSCRIÇÃO')
const naoAchou = await req(`/api/recordings?q=zzz${marca}nada`, { token: A.token })
chk(Array.isArray(naoAchou.json) && !naoAchou.json.some((r) => r.id === rec), 'e não devolve o que não bate')
chk((await req('/api/recordings?scope=tudo', { token: A.token })).status === 400, 'scope desconhecido é recusado')

// ---------------------------------------------------------------------------
console.log('\n--- descrição, etiquetas e publicação ---')
const pa = await req(`/api/recordings/${rec}`, { token: A.token, method: 'PATCH', body: { description: 'Módulo 3', tags: ['#Redes', 'redes', `t${marca}`] } })
chk(pa.status === 200 && pa.json.description === 'Módulo 3' && pa.json.tags.join() === `redes,t${marca}`, `PATCH normaliza etiquetas → ${JSON.stringify(pa.json?.tags)}`)
chk((await req(`/api/recordings?q=t${marca}`, { token: A.token })).json?.some((r) => r.id === rec), 'pesquisa encontra pela etiqueta')

// C é da mesma organização mas não esteve na sala: ANTES de publicar não vê.
await escondido('C (colega, não participou) lê os detalhes ANTES de publicar', `/api/recordings/${rec}/details`, { token: C.token })
chk((await req(`/api/recordings/${rec}`, { token: C.token, method: 'PATCH', body: { description: 'x' } })).status === 404, 'C não edita o que não vê')
const pub = await req(`/api/recordings/${rec}/publish`, { token: A.token, method: 'POST', body: { visibility: 'org' } })
chk(pub.status === 200 && pub.json.state === 'published' && pub.json.published_at, `publicar → ${pub.status} state=${pub.json?.state}`)
const pubC = await req('/api/recordings?scope=published', { token: C.token })
chk(pubC.json?.some((r) => r.id === rec && r.can_manage === false), 'C vê a publicada em scope=published, sem gerir')
chk((await req(`/api/recordings/${rec}/details`, { token: C.token })).status === 200, 'C lê os detalhes DEPOIS de publicar')
chk((await req(`/api/recordings/${rec}`, { token: C.token, method: 'PATCH', body: { description: 'x' } })).status === 403, 'C vê mas não edita → 403')
chk((await req(`/api/recordings/${rec}/publish`, { token: C.token, method: 'POST', body: {} })).status === 403, 'C não publica → 403')

// ---------------------------------------------------------------------------
console.log('\n--- legendas ---')
const g1 = await req(`/api/recordings/${rec}/captions/generate`, { token: A.token, method: 'POST', body: {} })
chk(g1.status === 201 && g1.json?.lang === 'pt' && g1.json.status === 'draft' && g1.json.source === 'transcript', `gerar na língua da transcrição → ${g1.status} ${g1.json?.status}`)
chk(g1.location === `/api/recordings/${rec}/captions/pt`, `Location ${g1.location}`)
chk((await req(`/api/recordings/${rec}/captions/pt/vtt`, { token: C.token })).status === 404, 'C não lê um RASCUNHO')
const lcC = await req(`/api/recordings/${rec}/captions`, { token: C.token })
chk(Array.isArray(lcC.json) && lcC.json.length === 0, 'e a lista de C não o mostra')
const pp = await req(`/api/recordings/${rec}/captions/pt`, { token: A.token, method: 'PATCH', body: { status: 'published' } })
chk(pp.json?.status === 'published' && pp.json.published_at, 'A publica a legenda')
const vttC = await req(`/api/recordings/${rec}/captions/pt/vtt`, { token: C.token })
chk(vttC.status === 200 && vttC.ct.startsWith('text/vtt') && vttC.text.startsWith('WEBVTT') && vttC.text.includes('00:00:01.300 --> 00:00:02.500'), `C lê o VTT publicado → ${vttC.status} ${vttC.ct}`)
chk((await req(`/api/recordings/${rec}/captions/fr`, { token: A.token, method: 'PUT', body: { vtt: 'SRT\n1\n00:00:01,000 --> 00:00:02,000\nx' } })).status === 400, 'PUT de um VTT inválido → 400')
const put = await req(`/api/recordings/${rec}/captions/fr`, { token: A.token, method: 'PUT', body: { vtt: 'WEBVTT\n\n00:00.000 --> 00:02.000\nBonjour\n', publish: true } })
chk(put.status === 200 && put.json.source === 'upload' && put.json.status === 'published', `PUT de um VTT válido → ${put.status}`)
chk((await req(`/api/recordings/${rec}/captions/fr`, { token: C.token, method: 'DELETE' })).status === 403, 'C não apaga legendas → 403')
chk((await req(`/api/recordings/${rec}/captions/fr`, { token: A.token, method: 'DELETE' })).status === 204, 'A apaga → 204')
chk((await req(`/api/recordings/${rec}/captions/fr`, { token: A.token, method: 'DELETE' })).status === 404, 'apagar outra vez → 404')
chk((await req(`/api/recordings/${rec}/captions/PT_ao`, { token: A.token, method: 'PUT', body: { vtt: 'WEBVTT\n' } })).status === 400, 'língua mal formada → 400')
const detC = await req(`/api/recordings/${rec}/details`, { token: C.token })
chk(JSON.stringify(detC.json?.caption_languages) === '["pt"]', `caption_languages=${JSON.stringify(detC.json?.caption_languages)}`)

const g2 = await req(`/api/recordings/${rec}/captions/generate`, { token: A.token, method: 'POST', body: { lang: 'en' } })
if (g2.status === 202) {
  chk(g2.json.status === 'generating' && g2.json.source === 'translation', 'tradução aceite em segundo plano → 202 generating')
  let en = null
  for (let i = 0; i < 40; i++) {
    en = (await req(`/api/recordings/${rec}/captions/en`, { token: A.token })).json
    if (en?.status !== 'generating') break
    await sleep(250)
  }
  chk(en?.status === 'draft' && /WEBVTT/.test(en.vtt) && en.vtt.split('-->').length === 3, `tradução termina em rascunho com as 2 cues → ${en?.status}`)
} else if (g2.status === 503 && !EXPECT_LLM) {
  naoVerificado.push('tradução de legendas: sem LLM local (OLLAMA_URL) — só se provou o 503')
  ok('sem LLM, a tradução responde 503 (não finge)')
} else {
  nok('tradução de legendas', `${g2.status}: ${JSON.stringify(g2.json)}`)
}
chk((await req(`/api/recordings/${rec}/captions/generate`, { token: A.token, method: 'POST', body: { lang: 'umb' } })).status !== 202, 'umbundu não é anunciado como traduzível')

// ---------------------------------------------------------------------------
console.log('\n--- capítulos ---')
const gc = await req(`/api/recordings/${rec}/chapters/generate`, { token: A.token, method: 'POST' })
if (gc.status === 200) {
  chk(Array.isArray(gc.json) && gc.json.length >= 1 && gc.json.every((c) => c.source === 'auto'), `capítulos automáticos gerados: ${gc.json?.length}`)
  chk(gc.json?.[0]?.t_ms === 0, 'o primeiro começa em 0')
} else if (gc.status === 503 && !EXPECT_LLM) {
  naoVerificado.push('capítulos automáticos: sem LLM local (OLLAMA_URL) — só se provou o 503')
  ok('sem LLM, gerar capítulos responde 503')
} else {
  nok('gerar capítulos', `${gc.status}: ${JSON.stringify(gc.json)}`)
}
const dur = up.json.duration_ms ?? 3000
const c1 = await req(`/api/recordings/${rec}/chapters`, { token: A.token, method: 'POST', body: { t_ms: 1500, title: 'Sub-redes' } })
chk(c1.status === 201 && c1.json.source === 'manual' && c1.location?.endsWith(c1.json.id), `capítulo manual → ${c1.status}`)
chk((await req(`/api/recordings/${rec}/chapters`, { token: A.token, method: 'POST', body: { t_ms: 1500, title: 'repetido' } })).status === 409, 'mesmo instante → 409')
if (up.json.duration_ms !== null) {
  chk((await req(`/api/recordings/${rec}/chapters`, { token: A.token, method: 'POST', body: { t_ms: dur + 60_000, title: 'depois do fim' } })).status === 400, 'depois do fim da gravação → 400')
}
chk((await req(`/api/recordings/${rec}/chapters`, { token: C.token, method: 'POST', body: { t_ms: 10, title: 'x' } })).status === 403, 'C não cria capítulos → 403')
const c1p = await req(`/api/recordings/${rec}/chapters/${c1.json.id}`, { token: A.token, method: 'PATCH', body: { title: 'Sub-redes e VLAN' } })
chk(c1p.status === 200 && c1p.json.title === 'Sub-redes e VLAN', 'PATCH do capítulo')
chk((await req(`/api/recordings/${rec}/chapters/${c1.json.id}`, { token: C.token })).status === 200, 'C lê um capítulo')
if (gc.status === 200) {
  const regen = await req(`/api/recordings/${rec}/chapters/generate`, { token: A.token, method: 'POST' })
  chk(regen.json?.some((c) => c.id === c1.json.id), 'gerar outra vez NÃO apaga o capítulo manual')
}
chk((await req(`/api/recordings/${rec}/chapters/${c1.json.id}`, { token: A.token, method: 'DELETE' })).status === 204, 'DELETE → 204')

// ---------------------------------------------------------------------------
console.log('\n--- comentários, visualizações, participantes ---')
const k1 = await req(`/api/recordings/${rec}/comments`, { token: C.token, method: 'POST', body: { body: 'Pode repetir esta parte?', t_ms: 1400 } })
chk(k1.status === 201 && k1.json.t_ms === 1400 && k1.json.can_delete === true, `C comenta com marca temporal → ${k1.status}`)
const k2 = await req(`/api/recordings/${rec}/comments`, { token: A.token, method: 'POST', body: { body: 'Sim, no minuto seguinte.' } })
chk(k2.status === 201 && k2.json.t_ms === null, 'A comenta sem marca temporal')
chk((await req(`/api/recordings/${rec}/comments`, { token: A.token, method: 'POST', body: { body: '' } })).status === 400, 'comentário vazio → 400')
const pg1 = await req(`/api/recordings/${rec}/comments?page_size=1`, { token: C.token })
chk(pg1.json?.items?.length === 1 && pg1.json.next_page_token, 'página 1 de 1 com cursor')
const pg2 = await req(`/api/recordings/${rec}/comments?page_size=1&page_token=${pg1.json?.next_page_token}`, { token: C.token })
chk(pg2.json?.items?.[0]?.id === k2.json.id && pg2.json.next_page_token === null, 'página 2 traz o seguinte e acaba')
chk((await req(`/api/recordings/${rec}/comments?page_size=1000`, { token: C.token })).status === 400, 'page_size acima de 100 é recusado, não cortado')
chk((await req(`/api/recordings/${rec}/comments/${k2.json.id}`, { token: C.token, method: 'DELETE' })).status === 403, 'C não apaga o comentário de A → 403')
chk((await req(`/api/recordings/${rec}/comments/${k1.json.id}`, { token: A.token, method: 'DELETE' })).status === 204, 'A (gestor) apaga o de C → 204')

chk((await req(`/api/recordings/${rec}/views`, { token: C.token, method: 'POST' })).status === 204, 'C regista visualização → 204')
await req(`/api/recordings/${rec}/views`, { token: C.token, method: 'POST' })
await req(`/api/recordings/${rec}/views`, { token: A.token, method: 'POST' })
const det = (await req(`/api/recordings/${rec}/details`, { token: A.token })).json
chk(det?.view_count === 2 && det.comment_count === 1, `uma visualização por pessoa por dia → view_count=${det?.view_count} comment_count=${det?.comment_count}`)

const parts = await req(`/api/recordings/${rec}/participants`, { token: C.token })
chk(parts.json?.items?.length === 1 && parts.json.items[0].user_id === A.userId, 'participantes da gravação')
chk((await req(`/api/rooms/${sala.code}/participants`, { token: A.token })).json?.items?.length === 1, 'participantes da sala para quem esteve lá')
await escondido('C (não esteve na sala) lista os participantes da SALA', `/api/rooms/${sala.code}/participants`, { token: C.token })

// ---------------------------------------------------------------------------
console.log('\n--- isolamento: a org B contra a gravação da org A (publicada) ---')
const rotasB = [
  ['detalhes', `/api/recordings/${rec}/details`],
  ['miniatura', `/api/recordings/${rec}/thumbnail`],
  ['transcrição', `/api/recordings/${rec}/transcript`],
  ['lista de legendas', `/api/recordings/${rec}/captions`],
  ['legenda', `/api/recordings/${rec}/captions/pt`],
  ['VTT', `/api/recordings/${rec}/captions/pt/vtt`],
  ['capítulos', `/api/recordings/${rec}/chapters`],
  ['comentários', `/api/recordings/${rec}/comments`],
  ['comentário', `/api/recordings/${rec}/comments/${k2.json.id}`],
  ['participantes', `/api/recordings/${rec}/participants`],
]
for (const [n, p] of rotasB) await escondido(`B lê ${n}`, p, { token: B.token })
await escondido('B edita', `/api/recordings/${rec}`, { token: B.token, method: 'PATCH', body: { description: 'forjada' } })
await escondido('B publica', `/api/recordings/${rec}/publish`, { token: B.token, method: 'POST', body: { visibility: 'org' } })
await escondido('B despublica', `/api/recordings/${rec}/unpublish`, { token: B.token, method: 'POST' })
await escondido('B regista visualização', `/api/recordings/${rec}/views`, { token: B.token, method: 'POST' })
await escondido('B comenta', `/api/recordings/${rec}/comments`, { token: B.token, method: 'POST', body: { body: 'intruso' } })
await escondido('B cria capítulo', `/api/recordings/${rec}/chapters`, { token: B.token, method: 'POST', body: { t_ms: 1, title: 'x' } })
await escondido('B gera capítulos', `/api/recordings/${rec}/chapters/generate`, { token: B.token, method: 'POST' })
await escondido('B envia legenda', `/api/recordings/${rec}/captions/pt`, { token: B.token, method: 'PUT', body: { vtt: 'WEBVTT\n' } })
await escondido('B gera legenda', `/api/recordings/${rec}/captions/generate`, { token: B.token, method: 'POST', body: {} })
await escondido('B lista participantes da sala da A', `/api/rooms/${sala.code}/participants`, { token: B.token })
const libB = await req('/api/recordings?scope=published', { token: B.token })
chk(Array.isArray(libB.json) && !libB.json.some((r) => r.id === rec), 'a publicada da A NÃO aparece em scope=published da B')
chk(!(await req(`/api/recordings?q=${palavra}`, { token: B.token })).json?.some((r) => r.id === rec), 'nem na pesquisa da B pela transcrição')
const detA = (await req(`/api/recordings/${rec}/details`, { token: A.token })).json
chk(detA?.description === 'Módulo 3' && detA.comment_count === 1, 'e nada do que B tentou mudou o estado')

await req(`/api/recordings/${rec}/unpublish`, { token: A.token, method: 'POST' })
await escondido('C DEPOIS de despublicar', `/api/recordings/${rec}/details`, { token: C.token })

// ---------------------------------------------------------------------------
console.log('\n--- reuniões: formato, sala de espera, gravação automática, qualidade ---')
chk((await req('/api/meetings', { token: A.token, method: 'POST', body: { title: 'x', starts_at: new Date(Date.now() + 3600e3).toISOString(), format: 'webinar' } })).status === 400, 'formato desconhecido → 400')
chk((await req('/api/meetings', { token: A.token, method: 'POST', body: { title: 'x', starts_at: new Date(Date.now() + 3600e3).toISOString(), record_quality: '8k' } })).status === 400, 'qualidade desconhecida → 400')
const m = await req('/api/meetings', {
  token: A.token, method: 'POST',
  body: {
    title: 'Videoaula de redes', kind: 'video', starts_at: new Date(Date.now() + 3600e3).toISOString(), duration_min: 60,
    invitee_ids: [C.userId], format: 'training', waiting_room: true, auto_record: true, record_quality: '720p',
  },
})
chk(m.status === 200 && m.json.format === 'training' && m.json.record_quality === '720p' && m.json.auto_record === true, `criar devolve as opções → ${m.status}`)
const ml = (await req('/api/meetings', { token: A.token })).json?.find((x) => x.id === m.json.id)
chk(ml?.format === 'training' && ml.waiting_room === true && ml.auto_record === true && ml.record_quality === '720p', 'a lista devolve as opções')
chk(ml?.invitee_count === 1 && ml.external_source === null, `invitee_count=${ml?.invitee_count} external_source=${ml?.external_source}`)
const tent = await req(`/api/meetings/${m.json.id}/respond`, { token: C.token, method: 'POST', body: { status: 'tentative' } })
chk(tent.status === 200, `C responde «tentativa» → ${tent.status}`)
chk((await req('/api/meetings', { token: C.token })).json?.find((x) => x.id === m.json.id)?.my_status === 'tentative', 'e a lista de C mostra tentative')
const st = await req(`/api/meetings/${m.json.id}/start`, { token: A.token, method: 'POST' })
chk(st.status === 200 && st.json.format === 'training' && st.json.waiting_room === true, `arrancar devolve as opções → ${st.status}`)
const salaM = (await req(`/api/rooms/${st.json.code}`, { token: A.token })).json
chk(salaM?.waiting_room === true && salaM.format === 'training', `a sala nasce com sala de espera e formato training (antes: sempre normal, sem espera) → ${salaM?.format}/${salaM?.waiting_room}`)
chk(sql(`SELECT auto_record::text || '/' || record_quality FROM rooms WHERE code = '${st.json.code}'`, { db: PGDB }) === 'true/720p', 'a sala guarda gravação automática e qualidade para o gravador')

const chave = await req(`/api/orgs/${A.orgId}/api-keys`, { token: A.token, method: 'POST', body: { name: `gm-${marca}` } })
const v1 = await req('/api/v1/meetings', {
  token: chave.json?.key, method: 'POST',
  body: { title: 'Do calendário Odoo', starts_at: new Date(Date.now() + 7200e3).toISOString(), host_email: A.email, external_ref: `odoo:teste:calendar.event:${marca}` },
})
chk(v1.status >= 200 && v1.status < 300, `reunião criada pela integração → ${v1.status}`)
const mo = (await req('/api/meetings', { token: A.token })).json?.find((x) => x.title === 'Do calendário Odoo')
chk(mo?.external_source === 'odoo', `origem externa na lista → ${mo?.external_source}`)

// ---------------------------------------------------------------------------
console.log(`\n=== ${passou} passaram, ${falhou} falharam ===`)
if (naoVerificado.length) {
  console.log('\nNÃO VERIFICADO nesta corrida:')
  for (const n of naoVerificado) console.log(`  · ${n}`)
}
process.exit(falhou ? 1 : 0)
