// UMA GRAVAÇÃO ARRANCADA À MÃO A MEIO DA CHAMADA tem vídeo que se vê? — medido
// numa gravação a sério do servidor, com o VP8 do Chromium (R299).
//
// Quem carrega em «gravar» com a chamada já a decorrer liga um writer a
// publicações que estão a meio do fluxo: o codificador do browser só manda um
// keyframe quando lho pedem, e a pista IVF só serve para alguma coisa se o
// primeiro quadro for um. Sem isso a pista não descodifica do princípio ao fim
// (o cabeçalho IVF fica com as dimensões nominais, 1280×720, porque nunca viu
// um keyframe), a grelha de dois publicadores falha no ffmpeg e a gravação
// fica `failed`; com um publicador o vídeo vai em cópia e o ficheiro fica
// «pronto» com imagem que nenhum leitor mostra.
//
// O `auto_record` não passa por aqui — a gravação arranca antes de as pistas
// serem publicadas e cada uma abre pelo seu primeiro keyframe —, por isso este
// teste grava à mão, dois segundos depois de a media estar a circular.
// Dois cenários, um por caminho do `finalize_inner`, e um controlo:
//   1. dois publicadores → grelha;
//   2. um publicador     → o vídeo em cópia;
//   3. `auto_record`, um publicador sozinho: a pista continua a abrir pelo
//      keyframe de entrada, e a gravação NÃO pede keyframe nenhum (o SFU conta
//      os que pede em `delonix_sfu_keyframes_requested_total`).
//
// NÃO corre no CI (que não tem ffmpeg). Precisa do servidor com ffmpeg, do vite
// a servir o arnês e de acesso ao `RECORDINGS_DIR` do servidor, de onde copia
// as pistas cruas antes de o `finalize` as apagar. Uso:
//   API=http://127.0.0.1:8180 APP=http://localhost:5174 RECORDINGS_DIR=… \
//     [FFMPEG=ffmpeg] [FFPROBE=ffprobe] [SAIDA=<pasta>] [CENARIOS=1,2,3] [SEGUNDOS=20] [VERBOSE=1] \
//     node e2e/gravacao-a-meio.mjs
import { chromium } from '@playwright/test'
import { spawnSync } from 'node:child_process'
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

const API = process.env.API ?? 'http://127.0.0.1:8180'
const APP = process.env.APP ?? 'http://localhost:5174'
const REC_DIR = process.env.RECORDINGS_DIR
const FFMPEG = process.env.FFMPEG ?? 'ffmpeg'
const FFPROBE = process.env.FFPROBE ?? 'ffprobe'
const SAIDA = process.env.SAIDA ?? mkdtempSync(join(tmpdir(), 'dlx-a-meio-'))
const SEGUNDOS = Number(process.env.SEGUNDOS ?? 20)
// As esperas esticam com a máquina (R118); o que se mede não.
const FATOR = Number(process.env.E2E_TIMEOUT_FACTOR) || 1
const PW = 'UmaPasswordForte123!'
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
let falhas = 0
const chk = (c, n) => { console.log(`  ${c ? '✓' : '✗'} ${n}`); if (!c) falhas++ }
const nota = (n) => console.log(`    · ${n}`)

if (!REC_DIR || !existsSync(REC_DIR)) {
  console.error('RECORDINGS_DIR tem de apontar para a pasta de gravações do servidor')
  process.exit(2)
}
mkdirSync(SAIDA, { recursive: true })

const j = (path, o = {}) =>
  fetch(`${API}${path}`, {
    method: o.method ?? 'GET',
    headers: { ...(o.token ? { Authorization: `Bearer ${o.token}` } : {}), ...(o.body ? { 'Content-Type': 'application/json' } : {}) },
    ...(o.body ? { body: JSON.stringify(o.body) } : {}),
  }).then(async (r) => ({ s: r.status, j: await r.json().catch(() => null) }))

const m = Math.random().toString(36).slice(2, 7)
const email = `gm${m}@gm${m}.local`
await j('/api/auth/register', { method: 'POST', body: { org_name: `GM ${m}`, email, username: `gm${m}`, password: PW } })
const tok = (await j('/api/auth/login', { method: 'POST', body: { email, password: PW } })).j.access_token

/** Uma entrada na sala. `ligou` diz se o PC chegou a `connected`: o arnês
 *  cria a chamada logo a seguir ao `Signaling`, e com a máquina carregada a
 *  oferta inicial sai antes de o WebSocket abrir e perde-se. Não é o que aqui
 *  se mede — quem chama tenta outra vez. */
async function entra(code, quem) {
  const jr = await j(`/api/rooms/${code}/join`, { token: tok, method: 'POST' })
  const b = await chromium.launch({ args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'] })
  const p = await (await b.newContext({ ignoreHTTPSErrors: true })).newPage()
  if (process.env.VERBOSE) p.on('console', (msg) => console.log(`      [${quem}] ${msg.text().slice(0, 160)}`))
  await p.goto(`${APP}/e2e/harness.html?token=${encodeURIComponent(jr.j.room_token)}&code=${code}&access=${encodeURIComponent(tok)}`, { waitUntil: 'domcontentloaded', timeout: 120000 })
  // `ready` é só «o arnês criou a chamada»; a media só existe com o PC ligado.
  let ligou = false
  for (let k = 0; k < 40 * FATOR && !ligou; k++) { await sleep(500); ligou = await p.evaluate(() => window.__dlx.state === 'connected') }
  return { b, p, ligou }
}

async function entraComTentativas(code, quem) {
  for (let t = 1; ; t++) {
    const e = await entra(code, quem)
    if (e.ligou || t === 4) return e
    nota(`o PC de ${quem} não ligou a tempo (tentativa ${t}) — nova entrada`)
    await e.b.close()
    await sleep(3000)
  }
}

const sessoes = () => readdirSync(REC_DIR).filter((d) => d.startsWith('tmp-'))

/** Quantos keyframes o SFU já pediu aos publicadores (PLI enviados). Este
 *  servidor só serve este teste, por isso a diferença é do cenário. */
async function pedidosDeKeyframe() {
  const t = await fetch(`${API}/metrics`).then((r) => r.text()).catch(() => '')
  const v = /^delonix_sfu_keyframes_requested_total (\d+)/m.exec(t)
  return v ? Number(v[1]) : null
}

/** Copia as pistas cruas da sessão enquanto o servidor compõe. */
async function guardaPistas(dir, destino) {
  let copiou = false
  for (let k = 0; k < 600; k++) {
    if (existsSync(join(REC_DIR, dir))) {
      try { cpSync(join(REC_DIR, dir), destino, { recursive: true }); copiou = true } catch { /* apagada a meio */ }
      if (copiou && existsSync(join(REC_DIR, dir, 'out.webm'))) break
    } else if (copiou) break
    await sleep(50)
  }
  return copiou
}

/** A gravação da sala, quando deixar de estar a compor. */
async function espera(code) {
  const ate = Date.now() + 240_000 * FATOR
  let ultimo = null
  while (Date.now() < ate) {
    const itens = ((await j('/api/recordings', { token: tok })).j ?? []).filter((r) => r.room_code === code)
    ultimo = itens.find((r) => r.status === 'ready') ?? itens[0] ?? null
    if (ultimo?.status === 'ready') return ultimo
    // Enquanto o ffmpeg compõe, a API já responde `failed` (sem causa): a
    // regra de domínio dá «sem ficheiro» como falha. Falha a sério traz causa.
    if (itens.length > 0 && itens.every((r) => r.status === 'failed' && r.failure_reason)) return ultimo
    await sleep(250)
  }
  return ultimo
}

// --- medição -------------------------------------------------------------

/** O que a pista IVF diz de si: dimensões do cabeçalho, quadros, e o primeiro. */
function ivf(ficheiro) {
  const b = readFileSync(ficheiro)
  const o = { largura: b.readUInt16LE(12), altura: b.readUInt16LE(14), quadros: 0, primeiroEKeyframe: false, primeiro: null }
  for (let i = 32; i + 12 <= b.length; ) {
    const n = b.readUInt32LE(i)
    if (o.quadros === 0 && n >= 10) {
      const q = b.subarray(i + 12, i + 12 + 10)
      // Keyframe VP8: bit P a zero e o código de início 9d 01 2a (RFC 6386 §9.1).
      const chave = (q[0] & 1) === 0 && q[3] === 0x9d && q[4] === 0x01 && q[5] === 0x2a
      o.primeiroEKeyframe = chave
      if (chave) o.primeiro = { largura: q.readUInt16LE(6) & 0x3fff, altura: q.readUInt16LE(8) & 0x3fff }
    }
    o.quadros++
    i += 12 + n
  }
  return o
}

/** Descodifica o vídeo de ponta a ponta: erros do descodificador e quadros que saíram.
 *
 *  Cada quadro sai no seu instante, em milissegundos (`-fps_mode passthrough`,
 *  `-enc_time_base 1:1000`). Sem isso o destino nulo conta o tempo em quadros à
 *  cadência que adivinhou, e dois quadros mais juntos do que um passo — a
 *  câmara falsa atrasa-se com a máquina carregada — dão um aviso de «dts não
 *  crescente» que não é erro nenhum da pista. */
function descodifica(ficheiro) {
  const d = spawnSync(FFMPEG, ['-v', 'error', '-i', ficheiro, '-map', '0:v:0', '-fps_mode', 'passthrough', '-enc_time_base', '1:1000', '-f', 'null', '-'], { encoding: 'utf8', maxBuffer: 1 << 26 })
  const p = spawnSync(FFPROBE, ['-v', 'error', '-select_streams', 'v:0', '-show_entries', 'frame=pts_time', '-of', 'csv=p=0', ficheiro], { encoding: 'utf8', maxBuffer: 1 << 28 })
  const pts = (p.stdout ?? '').split('\n').map((l) => parseFloat(l)).filter((n) => Number.isFinite(n))
  const erros = (d.stderr ?? '').split('\n').filter((l) => l.trim() !== '')
  return { saida: d.status, erros, quadros: pts.length, primeiroPts: pts[0] ?? null }
}

async function cenario(nome, { comB = false, auto = false } = {}) {
  console.log(`\n--- ${nome} ---`)
  const antes = new Set(sessoes())
  const reuniao = async () => {
    const r = await j('/api/meetings', {
      token: tok, method: 'POST',
      // SEM `auto_record` a gravação é a que o anfitrião arranca, a meio.
      body: { title: `a meio ${nome}`, kind: 'video', starts_at: new Date(Date.now() + 60e3).toISOString(), format: 'meeting', ...(auto ? { auto_record: true } : {}) },
    })
    return (await j(`/api/meetings/${r.j.id}/start`, { token: tok, method: 'POST' })).j.code
  }
  const pedidosAoEntrar = await pedidosDeKeyframe()
  let code, a
  if (auto) {
    // A sala grava desde a entrada do anfitrião e não recomeça: se A não
    // ligar, a tentativa seguinte é numa sala nova.
    for (let t = 1; ; t++) {
      code = await reuniao()
      a = await entra(code, 'A')
      if (a.ligou || t === 4) break
      nota(`o PC de A não ligou a tempo (tentativa ${t}) — sala nova`)
      await a.b.close()
    }
  } else {
    code = await reuniao()
    a = await entraComTentativas(code, 'A')
  }
  const b = comB ? await entraComTentativas(code, 'B') : null
  chk(a.ligou, 'A ligou com media')
  if (b) {
    chk(b.ligou, 'B ligou com media')
    const ve = async (p) => {
      for (let k = 0; k < 40 * FATOR && (await p.evaluate(() => window.__dlx.publicadores().length)) < 1; k++) await sleep(500)
      return (await p.evaluate(() => window.__dlx.publicadores().length)) === 1
    }
    chk(await ve(a.p), 'A recebe a media de B')
    chk(await ve(b.p), 'B recebe a media de A')
  }
  // A chamada decorre: os keyframes de entrada já passaram todos.
  await sleep(2000)
  const novas = () => sessoes().filter((d) => !antes.has(d))
  let viva = null
  let pedidosComAPistaAberta = null
  if (auto) {
    viva = novas().sort((x, y) => statSync(join(REC_DIR, y)).mtimeMs - statSync(join(REC_DIR, x)).mtimeMs)[0] ?? null
    chk(!!viva, 'a gravação arrancou sozinha, com a entrada do anfitrião')
  } else {
    chk(novas().length === 0, 'ninguém grava antes de o anfitrião carregar')
    const pedidos = await pedidosDeKeyframe()
    await a.p.evaluate(() => window.__dlx.gravar(true))
    for (let k = 0; k < 40 * FATOR && !viva; k++) { await sleep(250); viva = novas()[0] ?? null }
    chk(!!viva, 'a gravação arrancou à mão, com a chamada a decorrer')
    await sleep(1500)
    const pediu = (await pedidosDeKeyframe()) - pedidos
    chk(pedidos !== null && pediu >= (comB ? 2 : 1), `ao arrancar, o SFU pediu um keyframe a cada câmara (${pediu} pedidos)`)
    pedidosComAPistaAberta = await pedidosDeKeyframe()
  }
  if (!viva) { await a.b.close(); if (b) await b.b.close(); return }
  await sleep(SEGUNDOS * 1000)
  if (auto) {
    const pediu = (await pedidosDeKeyframe()) - pedidosAoEntrar
    chk(pedidosAoEntrar !== null && pediu === 0, `a gravação automática de um publicador sozinho não pediu keyframe nenhum (${pediu} pedidos)`)
  } else {
    // Aberta a pista, a gravação deixa de pedir. Uma bandeira que nunca
    // apagasse dava um pedido por segundo e por câmara (R14): ~20 por câmara
    // nesta janela. Fica folga para um PLI que um browser mande por conta própria.
    const pediu = (await pedidosDeKeyframe()) - pedidosComAPistaAberta
    chk(pedidosComAPistaAberta !== null && pediu <= 2, `com as pistas abertas, a gravação deixou de pedir keyframes (${pediu} pedidos em ${SEGUNDOS} s)`)
  }
  await a.p.evaluate(() => window.__dlx.gravar(false))
  const pasta = join(SAIDA, nome)
  const copiou = await guardaPistas(viva, join(pasta, 'pistas'))
  const item = await espera(code)
  await a.b.close()
  if (b) await b.b.close()

  // As pistas cruas: é aqui que o defeito nasce, antes de qualquer composição.
  chk(copiou, 'as pistas cruas foram copiadas antes de o servidor as apagar')
  const pistas = copiou ? readdirSync(join(pasta, 'pistas')).filter((f) => f.endsWith('.ivf')).sort() : []
  chk(pistas.length === (comB ? 2 : 1), `uma pista de vídeo por publicador (${pistas.length})`)
  for (const f of pistas) {
    const caminho = join(pasta, 'pistas', f)
    const v = ivf(caminho)
    const d = descodifica(caminho)
    nota(`${f}: cabeçalho ${v.largura}×${v.altura}, ${v.quadros} quadros; o primeiro ${v.primeiroEKeyframe ? `é keyframe de ${v.primeiro.largura}×${v.primeiro.altura}` : 'NÃO é keyframe'}; o ffmpeg descodifica ${d.quadros}, ${d.erros.length} linhas de erro (saída ${d.saida})${d.erros[0] ? ` — «${d.erros[0].slice(0, 90)}»` : ''}`)
    chk(v.quadros >= SEGUNDOS * 5, `${f}: a pista tem os quadros da gravação (${v.quadros} em ${SEGUNDOS} s)`)
    chk(v.primeiroEKeyframe, `${f}: o primeiro quadro da pista é um keyframe`)
    chk(v.primeiroEKeyframe && v.largura === v.primeiro.largura && v.altura === v.primeiro.altura, `${f}: o cabeçalho IVF leva as dimensões do keyframe, não as nominais`)
    chk(d.saida === 0 && d.erros.length === 0, `${f}: o ffmpeg descodifica a pista sem um erro`)
    chk(v.quadros > 0 && d.quadros === v.quadros, `${f}: e descodifica-a desde o primeiro quadro (${d.quadros} de ${v.quadros})`)
  }

  chk(item?.status === 'ready', `a gravação terminou em ready → ${item?.status} ${item?.failure_reason ?? ''}`)
  if (item?.status !== 'ready') return
  const dl = await fetch(`${API}/api/recordings/${item.id}/content?dl=1`, { headers: { Authorization: `Bearer ${tok}` } })
  const webm = join(pasta, 'gravacao.webm')
  mkdirSync(pasta, { recursive: true })
  writeFileSync(webm, Buffer.from(await dl.arrayBuffer()))
  const g = descodifica(webm)
  nota(`gravação: ${g.quadros} quadros de vídeo descodificados, o primeiro aos ${g.primeiroPts?.toFixed(3)} s; ${g.erros.length} linhas de erro (saída ${g.saida})`)
  chk(g.saida === 0 && g.erros.length === 0, 'o vídeo da gravação descodifica sem um erro')
  chk(g.quadros >= SEGUNDOS * 5, `a gravação tem imagem do princípio ao fim (${g.quadros} quadros em ${SEGUNDOS} s)`)
  chk(g.primeiroPts !== null && g.primeiroPts < 1, `a imagem começa com a gravação (primeiro quadro aos ${g.primeiroPts?.toFixed(3)} s)`)
}

const quais = (process.env.CENARIOS ?? '1,2,3').split(',')
if (quais.includes('1')) await cenario('1-grelha-dois-publicadores', { comB: true })
if (quais.includes('2')) await cenario('2-copia-um-publicador')
if (quais.includes('3')) await cenario('3-controlo-gravacao-automatica', { auto: true })

console.log(`\nficheiros em ${SAIDA}`)
console.log(falhas === 0 ? '\n=== TODOS PASSARAM ===' : `\n=== ${falhas} FALHARAM ===`)
process.exit(falhas === 0 ? 0 : 1)
