// O SOM E A IMAGEM DE UMA GRAVAÇÃO ficam juntos? — medido numa gravação a
// sério do servidor, com uma régua de som e imagem: um clarão e um toque no
// mesmo instante, de dois em dois segundos.
//
// Cada pista da gravação tem um instante em que entra na linha do tempo
// (`RecTrackMeta::offset_ms`). Se esse instante for o de quando o writer foi
// LIGADO e não o do primeiro quadro (ou pacote) que a pista tem, a pista entra
// adiantada pelo tempo que esperou: o vídeo, pelo keyframe que o SFU teve de
// pedir (até 1 s se o pedido cair no intervalo mínimo entre PLI); o áudio, pelo
// primeiro pacote (até 400 ms com o microfone em silêncio e DTX).
//
// A régua (`fonte=regua` no arnês) sai do relógio do `AudioContext` do browser,
// som e imagem do mesmo sítio. A câmara e o microfone falsos do Chromium não
// servem: arrancam cada um por si, a uma distância que muda a cada entrada.
//
// Cenários (`CENARIOS=…`), um por caminho do `finalize_inner` e por causa:
//   calibra    a régua gravada pelo `MediaRecorder` do próprio browser — quanto
//              vale o zero da medição;
//   copia      um publicador, gravação arrancada à mão a meio da chamada;
//   copia-pli  o mesmo, com o pedido de keyframe do arranque TRAVADO (outro
//              participante entrou e saiu no segundo anterior);
//   copia-dtx  o mesmo, com o microfone em silêncio entre os toques (DTX);
//   grelha     dois publicadores, arrancada logo a seguir à entrada do segundo;
//   tarde      a gravação já corre e o segundo publicador entra `TARDE` segundos
//              depois: a pista dele tem um offset grande.
//
// NÃO corre no CI (que não tem ffmpeg). Precisa do servidor com ffmpeg, do vite
// a servir o arnês e de acesso ao `RECORDINGS_DIR` do servidor. Uso:
//   API=http://127.0.0.1:8180 APP=http://localhost:5174 RECORDINGS_DIR=… \
//     [SERVER_LOG=<log do servidor com RUST_LOG=info,delonix_server::recorder=debug>] \
//     [FFMPEG=ffmpeg] [SAIDA=<pasta>] [CENARIOS=calibra,copia,…] [SEGUNDOS=24] [TARDE=30] \
//     [TOLERANCIA_MS=60] [VERBOSE=1] node e2e/gravacao-sincronismo.mjs
import { chromium } from '@playwright/test'
import { execFileSync, spawnSync } from 'node:child_process'
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

const API = process.env.API ?? 'http://127.0.0.1:8180'
const APP = process.env.APP ?? 'http://localhost:5174'
const REC_DIR = process.env.RECORDINGS_DIR
const SERVER_LOG = process.env.SERVER_LOG
const FFMPEG = process.env.FFMPEG ?? 'ffmpeg'
const SAIDA = process.env.SAIDA ?? mkdtempSync(join(tmpdir(), 'dlx-sincronismo-'))
const SEGUNDOS = Number(process.env.SEGUNDOS ?? 24)
const TARDE = Number(process.env.TARDE ?? 30)
// Um quadro de vídeo a 30 fps são 33 ms, e a régua tem o seu próprio erro
// (ver `calibra`). Abaixo dos 125 ms a que um atraso do som se nota e perto
// dos 45 ms a que se nota um avanço (ITU-R BT.1359).
const TOLERANCIA = Number(process.env.TOLERANCIA_MS ?? 60)
// As esperas esticam com a máquina (R118); o que se mede não.
const FATOR = Number(process.env.E2E_TIMEOUT_FACTOR) || 1
const PERIODO = 2
const PW = 'UmaPasswordForte123!'
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
let falhas = 0
const chk = (c, n) => { console.log(`  ${c ? '✓' : '✗'} ${n}`); if (!c) falhas++ }
const nota = (n) => console.log(`    · ${n}`)
const resumo = []

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
const email = `sy${m}@sy${m}.local`
await j('/api/auth/register', { method: 'POST', body: { org_name: `SY ${m}`, email, username: `sy${m}`, password: PW } })
const tok = (await j('/api/auth/login', { method: 'POST', body: { email, password: PW } })).j.access_token

async function reuniao(nome) {
  const r = await j('/api/meetings', {
    token: tok, method: 'POST',
    body: { title: `sincronismo ${nome}`, kind: 'video', starts_at: new Date(Date.now() + 60e3).toISOString(), format: 'meeting' },
  })
  return (await j(`/api/meetings/${r.j.id}/start`, { token: tok, method: 'POST' })).j.code
}

/** Uma entrada na sala com a régua. `tom`: a frequência do toque (cada
 *  publicador tem a sua, para se separarem na mistura); `faixa`: uma faixa
 *  clara fixa no topo da imagem, para se saber de quem é cada célula da grelha.
 *  `ligou` diz se o PC chegou a `connected`: o arnês cria a chamada logo a
 *  seguir ao `Signaling`, e com a máquina carregada a oferta inicial sai antes
 *  de o WebSocket abrir e perde-se — quem chama tenta outra vez. */
async function entra(code, quem, { fundo = true, tom = 2500, faixa = false, assiste = false } = {}) {
  const jr = await j(`/api/rooms/${code}/join`, { token: tok, method: 'POST' })
  const b = await chromium.launch({ args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream', '--autoplay-policy=no-user-gesture-required'] })
  const p = await (await b.newContext({ ignoreHTTPSErrors: true })).newPage()
  if (process.env.VERBOSE) p.on('console', (msg) => console.log(`      [${quem}] ${msg.text().slice(0, 160)}`))
  const q = assiste ? 'fonte=nada' : `fonte=regua&fundo=${fundo ? 1 : 0}&tom=${tom}&faixa=${faixa ? 1 : 0}`
  await p.goto(`${APP}/e2e/harness.html?token=${encodeURIComponent(jr.j.room_token)}&code=${code}&access=${encodeURIComponent(tok)}&${q}`, { waitUntil: 'domcontentloaded', timeout: 120000 })
  let ligou = false
  for (let k = 0; k < 40 * FATOR && !ligou; k++) { await sleep(500); ligou = await p.evaluate(() => window.__dlx.state === 'connected') }
  return { b, p, ligou }
}

async function entraComTentativas(code, quem, o) {
  for (let t = 1; ; t++) {
    const e = await entra(code, quem, o)
    if (e.ligou || t === 4) return e
    nota(`o PC de ${quem} não ligou a tempo (tentativa ${t}) — nova entrada`)
    await e.b.close()
    await sleep(3000)
  }
}

/** O browser `p` já descodifica vídeo de alguém? (a pista de áudio chega
 *  primeiro, e com a máquina carregada o vídeo pode demorar segundos.) */
async function recebeVideo(p) {
  for (let k = 0; k < 80 * FATOR; k++) {
    const n = await p.evaluate(async () => (await window.__dlx.raw()).filter((s) => s.type === 'inbound-rtp' && s.kind === 'video' && s.framesDecoded > 0).length)
    if (n > 0) return true
    await sleep(250)
  }
  return false
}

const sessoes = () => readdirSync(REC_DIR).filter((d) => d.startsWith('tmp-'))

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
    if (itens.length > 0 && itens.every((r) => r.status === 'failed' && r.failure_reason)) return ultimo
    await sleep(250)
  }
  return ultimo
}

/** O que o servidor escreveu no log desde `desde` (bytes) sobre a espera das pistas. */
function esperasNoLog(desde) {
  if (!SERVER_LOG || !existsSync(SERVER_LOG)) return null
  const novo = readFileSync(SERVER_LOG).subarray(desde).toString().replace(/\x1b\[[0-9;]*m/g, '')
  const o = []
  for (const l of novo.split('\n')) {
    const e = /gravação: a pista .*/.exec(l)
    if (!e) continue
    const pista = /track=(\S+)/.exec(l)?.[1] ?? '?'
    const ms = /(?:espera_ms|primeiro_ms|offset_ms)=(\d+)/.exec(l)?.[1]
    if (ms !== undefined) o.push(`${pista} ${ms} ms`)
  }
  return o
}
const tamanhoDoLog = () => (SERVER_LOG && existsSync(SERVER_LOG) ? readFileSync(SERVER_LOG).length : 0)

// --- medição -------------------------------------------------------------

/** Os quadros de uma zona da imagem: instante (PTS do ficheiro) e luz média. */
function luz(ficheiro, recorte) {
  const vf = `crop=${recorte},scale=8:8,format=gray,showinfo`
  const r = spawnSync(FFMPEG, ['-nostdin', '-loglevel', 'info', '-i', ficheiro, '-map', '0:v:0', '-vf', vf, '-fps_mode', 'passthrough', '-f', 'null', '-'], { encoding: 'utf8', maxBuffer: 1 << 28 })
  const q = []
  for (const l of (r.stderr ?? '').split('\n')) {
    const t = /pts_time:([0-9.]+)/.exec(l)
    const v = /mean:\[(\d+)/.exec(l)
    if (t && v) q.push({ t: parseFloat(t[1]), luz: Number(v[1]) })
  }
  return q
}

/** Instantes em que a zona passa de escura a clara. */
function claroes(quadros) {
  const o = []
  for (let i = 1; i < quadros.length; i++) if (quadros[i].luz > 150 && quadros[i - 1].luz <= 150) o.push(quadros[i].t)
  return o
}

/** Instantes em que começa um toque a `hz`. O áudio é descodificado a honrar
 *  os PTS do contentor (o silêncio que ele declara conta), que é o que um
 *  leitor faz quando acerta o som pela imagem. Energia em janelas de 5 ms. */
function toques(ficheiro, hz) {
  const raw = execFileSync(FFMPEG, ['-nostdin', '-loglevel', 'error', '-i', ficheiro, '-map', '0:a:0', '-af', 'aresample=async=1:first_pts=0', '-ac', '1', '-ar', '48000', '-f', 's16le', '-'], { maxBuffer: 1 << 30 })
  const s = new Int16Array(raw.buffer.slice(raw.byteOffset, raw.byteOffset + (raw.byteLength & ~1)))
  const J = 240
  const c = 2 * Math.cos((2 * Math.PI * hz) / 48000)
  const e = []
  for (let i = 0; i + J <= s.length; i += J) {
    let a = 0, b = 0
    for (let k = 0; k < J; k++) { const v = s[i + k] / 32768 + c * a - b; b = a; a = v }
    e.push(Math.sqrt(Math.max(0, a * a + b * b - c * a * b)) / (J / 2))
  }
  const limiar = e.reduce((x, v) => Math.max(x, v), 0) * 0.3
  const o = []
  let calado = 0
  for (let i = 0; i < e.length; i++) {
    if (e[i] > limiar) { if (calado >= 20) o.push((i * J) / 48000); calado = 0 } else calado++
  }
  return o
}

/** Para cada clarão, o toque mais próximo: `imagem − som`, em ms. Negativo: a
 *  imagem adianta-se ao som. Lê-se sem ambiguidade até ±1 s (meio período). */
function desvio(cl, tq) {
  const d = []
  for (const c of cl) {
    let melhor = null
    for (const t of tq) if (melhor === null || Math.abs(c - t) < Math.abs(c - melhor)) melhor = t
    if (melhor !== null && Math.abs(c - melhor) <= PERIODO / 2) d.push((c - melhor) * 1000)
  }
  const o = [...d].sort((a, b) => a - b)
  return { n: d.length, mediana: o.length ? o[Math.floor(o.length / 2)] : null, min: o[0] ?? null, max: o.at(-1) ?? null }
}
const ms = (v) => (v === null ? '—' : `${v >= 0 ? '+' : ''}${v.toFixed(0)} ms`)

/** A zona do quadro que tem imagem (a grelha pode vir com barras pretas),
 *  medida nos últimos segundos, quando todas as células já estão ocupadas. */
function zona(ficheiro) {
  const r = spawnSync(FFMPEG, ['-nostdin', '-sseof', '-4', '-i', ficheiro, '-map', '0:v:0', '-vf', 'cropdetect=limit=24:round=2:reset=0', '-f', 'null', '-'], { encoding: 'utf8', maxBuffer: 1 << 26 })
  const c = [...(r.stderr ?? '').matchAll(/crop=(\d+):(\d+):(\d+):(\d+)/g)].at(-1)
  return c ? { w: Number(c[1]), h: Number(c[2]), x: Number(c[3]), y: Number(c[4]) } : null
}

/** Mede um ficheiro: por célula (uma, ou as da grelha), de quem é e o desvio. */
function mede(ficheiro, celulas, publicadores) {
  const z = zona(ficheiro)
  if (!z) return []
  const cw = z.w / celulas
  const o = []
  for (let k = 0; k < celulas; k++) {
    // O meio da célula (uma imagem 4:3 numa célula 16:9 leva barras aos lados):
    // a faixa de cima diz de quem é, a parte de baixo leva o clarão.
    const x = Math.round(z.x + k * cw + cw * 0.4)
    const w = Math.round(cw * 0.2)
    const cima = luz(ficheiro, `${w}:${Math.round(z.h * 0.1)}:${x}:${Math.round(z.y + z.h * 0.08)}`)
    const baixo = luz(ficheiro, `${w}:${Math.round(z.h * 0.2)}:${x}:${Math.round(z.y + z.h * 0.55)}`)
    // Fora dos clarões a faixa de cima é clara (>90) só em quem a pinta.
    const escuros = cima.filter((_, i) => (baixo[i]?.luz ?? 0) <= 150 && (baixo[i]?.luz ?? 0) > 20).map((q) => q.luz).sort((a, b) => a - b)
    const temFaixa = (escuros[Math.floor(escuros.length / 2)] ?? 0) > 90
    const quem = publicadores.find((p) => p.faixa === temFaixa) ?? publicadores[0]
    const cl = claroes(baixo)
    const tq = toques(ficheiro, quem.tom)
    // O primeiro quadro com imagem a sério (antes dele a célula é preta).
    const comeca = baixo.find((q) => q.luz > 20)?.t ?? null
    o.push({ quem: quem.nome, celula: k, comeca, claroes: cl.length, toques: tq.length, ...desvio(cl, tq) })
  }
  return o
}

// --- cenários ------------------------------------------------------------

async function calibra() {
  console.log('\n--- calibra ---')
  const code = await reuniao('calibra')
  const a = await entraComTentativas(code, 'A')
  chk(a.ligou, 'A ligou com media')
  for (const fundo of [true]) {
    const b64 = await a.p.evaluate(() => window.__dlx.gravarLocal(12000))
    const f = join(SAIDA, `calibra-${fundo ? 'fundo' : 'silencio'}.webm`)
    writeFileSync(f, Buffer.from(b64, 'base64'))
    const r = mede(f, 1, [{ nome: 'A', tom: 2500, faixa: false }])[0]
    nota(`a régua gravada pelo MediaRecorder do browser: imagem − som = ${ms(r?.mediana ?? null)} (${r?.n} clarões, de ${ms(r?.min ?? null)} a ${ms(r?.max ?? null)})`)
    chk(!!r && r.n >= 4 && Math.abs(r.mediana) <= TOLERANCIA, `a régua sai do browser com som e imagem a menos de ${TOLERANCIA} ms`)
    resumo.push({ cenario: 'calibra', ...r })
  }
  await a.b.close()
}

/** `travado`: alguém entra só para assistir mesmo antes de se gravar. O SFU
 *  pede keyframes a A por causa dele, e o pedido do arranque da gravação cai no
 *  intervalo mínimo entre PLI (1 s): a pista de vídeo espera. Quanto esperou
 *  lê-se no log do servidor, e o cenário repete-se até a espera passar de
 *  `ESPERA_MIN` — sem isso não se tinha medido o que o cenário diz medir. */
const ESPERA_MIN = 200
async function copia(nome, { fundo = true, travado = false } = {}) {
  console.log(`\n--- ${nome} ---`)
  for (let tentativa = 1; ; tentativa++) {
    const antes = new Set(sessoes())
    const code = await reuniao(nome)
    const a = await entraComTentativas(code, 'A', { fundo })
    if (!a.ligou) { chk(false, 'A ligou com media'); await a.b.close(); return }
    await sleep(3000)
    const log0 = tamanhoDoLog()
    let b = null
    if (travado) {
      b = await entraComTentativas(code, 'B', { assiste: true })
      await sleep(Math.random() * 600)
    }
    await a.p.evaluate(() => window.__dlx.gravar(true))
    let viva = null
    for (let k = 0; k < 40 * FATOR && !viva; k++) { await sleep(250); viva = sessoes().find((d) => !antes.has(d)) ?? null }
    if (!viva) { chk(false, 'a gravação arrancou à mão, com a chamada a decorrer'); await a.b.close(); if (b) await b.b.close(); return }
    if (travado) {
      await sleep(2500)
      const esperou = Math.max(0, ...(esperasNoLog(log0) ?? []).map((l) => Number(/ (\d+) ms/.exec(l)?.[1] ?? 0)))
      if (SERVER_LOG && esperou < ESPERA_MIN && tentativa < 6) {
        nota(`tentativa ${tentativa}: a pista de vídeo só esperou ${esperou} ms pelo keyframe — o pedido não foi travado; outra vez`)
        await a.p.evaluate(() => window.__dlx.gravar(false))
        await espera(code)
        await a.b.close()
        await b.b.close()
        continue
      }
      chk(!SERVER_LOG || esperou >= ESPERA_MIN, `a pista de vídeo esperou pelo keyframe (${SERVER_LOG ? `${esperou} ms` : 'sem SERVER_LOG não se sabe quanto'})`)
    }
    chk(true, 'a gravação arrancou à mão, com a chamada a decorrer')
    await sleep(SEGUNDOS * 1000)
    await a.p.evaluate(() => window.__dlx.gravar(false))
    await fecha(nome, code, viva, [a, b], 1, [{ nome: 'A', tom: 2500, faixa: false }], log0)
    return
  }
}

/** `tarde`: a gravação arranca só com A, e B entra `TARDE` segundos depois. */
async function grelha(nome, { tarde = false } = {}) {
  console.log(`\n--- ${nome} ---`)
  const antes = new Set(sessoes())
  const code = await reuniao(nome)
  const a = await entraComTentativas(code, 'A')
  chk(a.ligou, 'A ligou com media')
  await sleep(3000)
  const log0 = tamanhoDoLog()
  let b = null
  const entraB = async () => {
    b = await entraComTentativas(code, 'B', { tom: 1500, faixa: true })
    chk(b.ligou, 'B ligou com media')
    // Os dois vêem-se: cada um acabou de receber um pedido de keyframe.
    for (let k = 0; k < 400 * FATOR && ((await a.p.evaluate(() => window.__dlx.publicadores().length)) < 1 || (await b.p.evaluate(() => window.__dlx.publicadores().length)) < 1); k++) await sleep(25)
  }
  if (!tarde) await entraB()
  await a.p.evaluate(() => window.__dlx.gravar(true))
  let viva = null
  for (let k = 0; k < 40 * FATOR && !viva; k++) { await sleep(250); viva = sessoes().find((d) => !antes.has(d)) ?? null }
  chk(!!viva, 'a gravação arrancou à mão, com a chamada a decorrer')
  if (!viva) { await a.b.close(); if (b) await b.b.close(); return }
  if (tarde) {
    await sleep(TARDE * 1000)
    // A pista de vídeo de B tem de chegar a existir: sem ela não há célula.
    for (let t = 1; ; t++) {
      await entraB()
      if ((await recebeVideo(a.p)) || t === 4) break
      nota(`o vídeo de B não chegou a A (tentativa ${t}) — nova entrada`)
      await b.b.close()
    }
  }
  await sleep(SEGUNDOS * 1000)
  await a.p.evaluate(() => window.__dlx.gravar(false))
  await fecha(nome, code, viva, [a, b], 2, [{ nome: 'A', tom: 2500, faixa: false }, { nome: 'B', tom: 1500, faixa: true }], log0)
}

async function fecha(nome, code, viva, browsers, celulas, publicadores, log0) {
  const pasta = join(SAIDA, nome)
  const copiou = await guardaPistas(viva, join(pasta, 'pistas'))
  const item = await espera(code)
  for (const x of browsers) if (x) await x.b.close()
  const pistas = copiou ? readdirSync(join(pasta, 'pistas')).filter((f) => /\.(ivf|ogg)$/.test(f)).sort() : []
  nota(`pistas cruas: ${pistas.join(', ') || '(nenhuma)'}`)
  const esperas = esperasNoLog(log0)
  if (esperas) nota(`no log do servidor: ${esperas.join('; ') || '(nada)'}`)
  chk(pistas.filter((f) => f.endsWith('.ivf')).length === celulas, `uma pista de vídeo por publicador (${pistas.filter((f) => f.endsWith('.ivf')).length} para ${celulas})`)
  chk(item?.status === 'ready', `a gravação terminou em ready → ${item?.status} ${item?.failure_reason ?? ''}`)
  if (item?.status !== 'ready') return
  const dl = await fetch(`${API}/api/recordings/${item.id}/content?dl=1`, { headers: { Authorization: `Bearer ${tok}` } })
  mkdirSync(pasta, { recursive: true })
  const webm = join(pasta, 'gravacao.webm')
  writeFileSync(webm, Buffer.from(await dl.arrayBuffer()))
  const medidas = mede(webm, celulas, publicadores)
  chk(new Set(medidas.map((x) => x.quem)).size === celulas, `cada célula é de um publicador diferente (${medidas.map((x) => x.quem).join(', ')})`)
  for (const r of medidas) {
    nota(`${r.quem}: a imagem começa aos ${r.comeca?.toFixed(3)} s; ${r.claroes} clarões, ${r.toques} toques; imagem − som = ${ms(r.mediana)} (de ${ms(r.min)} a ${ms(r.max)})`)
    chk(r.n >= 4, `${r.quem}: a gravação tem clarões e toques que cheguem para medir (${r.n} pares)`)
    chk(r.mediana !== null && Math.abs(r.mediana) <= TOLERANCIA, `${r.quem}: o som e a imagem estão a menos de ${TOLERANCIA} ms um do outro (${ms(r.mediana)})`)
    resumo.push({ cenario: nome, esperas, ...r })
  }
}

const quais = (process.env.CENARIOS ?? 'calibra,copia,copia-pli,copia-dtx,grelha,tarde').split(',')
if (quais.includes('calibra')) await calibra()
if (quais.includes('copia')) await copia('copia')
if (quais.includes('copia-pli')) await copia('copia-pli', { travado: true })
if (quais.includes('copia-dtx')) await copia('copia-dtx', { fundo: false })
if (quais.includes('grelha')) await grelha('grelha')
if (quais.includes('tarde')) await grelha('tarde', { tarde: true })

writeFileSync(join(SAIDA, 'resumo.json'), JSON.stringify(resumo, null, 2))
console.log('\ncenário · quem · imagem − som (mediana, mínimo, máximo) · pares')
for (const r of resumo) console.log(`  ${r.cenario} · ${r.quem ?? 'A'} · ${ms(r.mediana ?? null)} (${ms(r.min ?? null)} … ${ms(r.max ?? null)}) · ${r.n ?? 0}`)
console.log(`\nficheiros em ${SAIDA}`)
console.log(falhas === 0 ? '\n=== TODOS PASSARAM ===' : `\n=== ${falhas} FALHARAM ===`)
process.exit(falhas === 0 ? 0 : 1)
