// A FALA DEPOIS DE UM SILÊNCIO fica onde foi dita? — medido numa gravação a
// sério do servidor, com o Opus do Chromium e `usedtx=1`, e ouvida no Chromium
// (R295).
//
// Quem se cala deixa de enviar: o DTX manda um pacote a cada 400 ms e o
// timestamp RTP continua a andar. O `OggWriter` avança o grânulo pelo
// timestamp, por isso a pista gravada fica com BURACOS de PTS. Se a composição
// não os encher, o ficheiro final leva-os no contentor, o Chromium toca as
// amostras seguidas, e a fala depois de cada silêncio recua.
//
// Dois microfones falsos lidos de ficheiro (`--use-file-for-fake-audio-capture`,
// que o Chromium repete em ciclo de 8 s):
//   A — 2 s de tom a 440 Hz e 6 s de silêncio digital: o participante que se cala;
//   B — um toque de 150 ms a 2500 Hz em cada segundo: a régua.
// Na gravação os tons de A têm de começar de 8 em 8 s e os de B de segundo a
// segundo. Três cenários, um por caminho do `finalize_inner`:
//   1. dois publicadores com vídeo  → grelha + `amix`;
//   2. «só áudio», um publicador    → uma cadeia de áudio e mais nada;
//   3. um publicador com vídeo      → o vídeo em cópia.
//
// NÃO corre no CI (que não tem ffmpeg). Precisa do servidor com ffmpeg, do vite
// a servir o arnês e de acesso ao `RECORDINGS_DIR` do servidor, de onde copia
// as pistas cruas antes de o `finalize` as apagar. Uso:
//   API=http://127.0.0.1:8180 APP=http://localhost:5174 RECORDINGS_DIR=… \
//     [FFMPEG=ffmpeg] [FFPROBE=ffprobe] [SAIDA=<pasta>] [CENARIOS=1,2,3] [VERBOSE=1] \
//     node e2e/gravacao-buraco-audio.mjs
import { chromium } from '@playwright/test'
import { execFileSync } from 'node:child_process'
import { cpSync, createReadStream, existsSync, mkdirSync, mkdtempSync, readdirSync, statSync, writeFileSync } from 'node:fs'
import { createServer } from 'node:http'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

const API = process.env.API ?? 'http://127.0.0.1:8180'
const APP = process.env.APP ?? 'http://localhost:5174'
const REC_DIR = process.env.RECORDINGS_DIR
const FFMPEG = process.env.FFMPEG ?? 'ffmpeg'
const FFPROBE = process.env.FFPROBE ?? 'ffprobe'
const SAIDA = process.env.SAIDA ?? mkdtempSync(join(tmpdir(), 'dlx-buraco-'))
const SEGUNDOS = Number(process.env.SEGUNDOS ?? 36)
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

// --- os dois microfones --------------------------------------------------
const lavfi = (expr, out) =>
  execFileSync(FFMPEG, ['-y', '-loglevel', 'error', '-f', 'lavfi', '-i', expr, '-t', '8', '-ac', '1', '-ar', '48000', '-c:a', 'pcm_s16le', out])
const wavA = join(SAIDA, 'mic-a.wav')
const wavB = join(SAIDA, 'mic-b.wav')
lavfi("aevalsrc='0.5*sin(2*PI*440*t)*lt(t,2)':s=48000:d=8", wavA)
lavfi("aevalsrc='0.5*sin(2*PI*2500*t)*lt(mod(t,1),0.15)':s=48000:d=8", wavB)

const j = (path, o = {}) =>
  fetch(`${API}${path}`, {
    method: o.method ?? 'GET',
    headers: { ...(o.token ? { Authorization: `Bearer ${o.token}` } : {}), ...(o.body ? { 'Content-Type': 'application/json' } : {}) },
    ...(o.body ? { body: JSON.stringify(o.body) } : {}),
  }).then(async (r) => ({ s: r.status, j: await r.json().catch(() => null) }))

const m = Math.random().toString(36).slice(2, 7)
const email = `ba${m}@ba${m}.local`
await j('/api/auth/register', { method: 'POST', body: { org_name: `BA ${m}`, email, username: `ba${m}`, password: PW } })
const tok = (await j('/api/auth/login', { method: 'POST', body: { email, password: PW } })).j.access_token

async function reuniao(opcoes) {
  const r = await j('/api/meetings', {
    token: tok, method: 'POST',
    // `auto_record`: a gravação arranca com a entrada do anfitrião e cada
    // pista abre ao ser publicada, pelo seu primeiro keyframe. Arrancada à
    // mão a meio da chamada, a pista de vídeo começa sem keyframe e a grelha
    // não a descodifica — outro defeito, que taparia este.
    body: { title: `buraco ${JSON.stringify(opcoes)}`, kind: 'video', starts_at: new Date(Date.now() + 60e3).toISOString(), format: 'meeting', auto_record: true, ...opcoes },
  })
  const st = await j(`/api/meetings/${r.j.id}/start`, { token: tok, method: 'POST' })
  return st.j.code
}

/** Uma entrada na sala. `ligou` diz se o PC chegou a `connected`: o arnês
 *  cria a chamada logo a seguir ao `Signaling`, e com a máquina carregada a
 *  oferta inicial sai antes de o WebSocket abrir e perde-se. Não é o que aqui
 *  se mede — quem chama tenta outra vez. */
async function entra(code, wav) {
  const jr = await j(`/api/rooms/${code}/join`, { token: tok, method: 'POST' })
  const b = await chromium.launch({
    args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream', `--use-file-for-fake-audio-capture=${wav}`],
  })
  const p = await (await b.newContext({ ignoreHTTPSErrors: true })).newPage()
  if (process.env.VERBOSE) p.on('console', (msg) => console.log(`      [${wav.slice(-9, -4)}] ${msg.text().slice(0, 160)}`))
  await p.goto(`${APP}/e2e/harness.html?token=${encodeURIComponent(jr.j.room_token)}&code=${code}&access=${encodeURIComponent(tok)}`, { waitUntil: 'domcontentloaded', timeout: 120000 })
  // `ready` é só «o arnês criou a chamada»; a media só existe com o PC ligado.
  let ligou = false
  for (let k = 0; k < 40 && !ligou; k++) { await sleep(500); ligou = await p.evaluate(() => window.__dlx.state === 'connected') }
  return { b, p, ligou }
}

/** Copia as pistas cruas da sessão enquanto o servidor compõe. */
async function guardaPistas(dir, destino) {
  let copiou = false
  for (let k = 0; k < 600; k++) {
    if (dir && existsSync(join(REC_DIR, dir))) {
      try { cpSync(join(REC_DIR, dir), destino, { recursive: true }); copiou = true } catch { /* apagada a meio */ }
      if (copiou && existsSync(join(REC_DIR, dir, 'out.webm'))) break
    } else if (copiou) break
    await sleep(50)
  }
  return copiou
}

/** A gravação da sala, quando deixar de estar a compor. */
async function espera(code) {
  const ate = Date.now() + 240_000
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
const passos = (v) => v.slice(1).map((t, i) => t - v[i])

/** Os buracos de PTS de uma pista de áudio (pacotes de 20 ms). */
function buracos(ficheiro) {
  const pts = execFileSync(FFPROBE, ['-v', 'error', '-select_streams', 'a:0', '-show_entries', 'packet=pts_time', '-of', 'csv=p=0', ficheiro], { maxBuffer: 1 << 28 })
    .toString().split('\n').filter((l) => l.trim() !== '').map((l) => parseFloat(l)).filter((n) => Number.isFinite(n))
  const saltos = passos(pts).filter((d) => d > 0.03)
  return { pacotes: pts.length, fim: pts.at(-1) ?? 0, n: saltos.length, total: saltos.reduce((a, b) => a + b - 0.02, 0), maior: Math.max(0, ...saltos) }
}

/** O áudio em PCM mono a 48 kHz, amostra atrás de amostra (sem olhar aos PTS). */
function pcm(ficheiro) {
  const raw = execFileSync(FFMPEG, ['-loglevel', 'error', '-i', ficheiro, '-map', '0:a:0', '-ac', '1', '-ar', '48000', '-f', 's16le', '-'], { maxBuffer: 1 << 30 })
  return new Int16Array(raw.buffer, raw.byteOffset, raw.byteLength >> 1)
}

/** Instantes (ms) em que o tom a `freq` começa: janelas de 20 ms, limiar a
 *  25 % do pico. Só conta depois de `calaMs` de silêncio e se durar `duraMs`
 *  — o tom apanhado a meio, no início da gravação, não é um início. */
function inicios(x, freq, calaMs, duraMs) {
  const N = 960
  const w = (2 * Math.PI * freq) / 48000
  const nivel = []
  for (let a = 0; a + N <= x.length; a += N) {
    let re = 0, im = 0
    for (let i = 0; i < N; i++) { re += x[a + i] * Math.cos(w * i); im += x[a + i] * Math.sin(w * i) }
    nivel.push((2 * Math.hypot(re, im)) / N / 32768)
  }
  const pico = Math.max(...nivel, 1e-9)
  const tons = []
  let calado = 0
  nivel.forEach((v, k) => {
    if (v > pico * 0.25) {
      if (calado * 20 >= calaMs) tons.push({ t: k * 20, dura: 0 })
      if (tons.length > 0 && calado * 20 < calaMs) tons.at(-1).dura = k * 20 - tons.at(-1).t + 20
      calado = 0
    } else calado++
  })
  return tons.filter((o) => o.dura >= duraMs).map((o) => o.t)
}

/** Toca o ficheiro no Chromium e devolve o instante DO MEDIA (ms) em que cada
 *  tom a 440 Hz se ouve — o que uma pessoa vê e ouve na biblioteca. */
async function ouveNoChromium(ficheiro) {
  const srv = createServer((req, res) => {
    if (req.url === '/') { res.setHeader('Content-Type', 'text/html'); return res.end('<video id=v src="/f.webm" preload="auto"></video>') }
    const size = statSync(ficheiro).size
    const r = /bytes=(\d+)-(\d*)/.exec(req.headers.range ?? '')
    const a = r ? Number(r[1]) : 0
    const b = r && r[2] ? Number(r[2]) : size - 1
    res.writeHead(r ? 206 : 200, { 'Content-Type': 'video/webm', 'Accept-Ranges': 'bytes', 'Content-Length': b - a + 1, ...(r ? { 'Content-Range': `bytes ${a}-${b}/${size}` } : {}) })
    createReadStream(ficheiro, { start: a, end: b }).pipe(res)
  })
  await new Promise((r) => srv.listen(0, '127.0.0.1', r))
  const b = await chromium.launch({ args: ['--autoplay-policy=no-user-gesture-required'] })
  try {
    const p = await b.newPage()
    await p.goto(`http://127.0.0.1:${srv.address().port}/`)
    return await p.evaluate(async () => {
      const v = document.getElementById('v')
      const ctx = new AudioContext()
      const an = ctx.createAnalyser()
      an.fftSize = 2048
      an.smoothingTimeConstant = 0
      ctx.createMediaElementSource(v).connect(an)
      an.connect(ctx.destination)
      const bin = Math.round(440 / (ctx.sampleRate / an.fftSize))
      const buf = new Float32Array(an.frequencyBinCount)
      const tons = []
      let calado = 0
      await v.play()
      const t0 = performance.now()
      await new Promise((fim) => {
        const id = setInterval(() => {
          an.getFloatFrequencyData(buf)
          if (Math.max(buf[bin - 1], buf[bin], buf[bin + 1]) > -45) {
            if (calado >= 50) tons.push({ t: Math.round(v.currentTime * 1000), ticks: 0 })
            if (tons.length > 0) tons.at(-1).ticks++
            calado = 0
          } else calado++
          if (v.ended || performance.now() - t0 > 120000) { clearInterval(id); fim() }
        }, 10)
      })
      // Só os tons inteiros (2 s): o apanhado a meio no início não é um início.
      return { inicios: tons.filter((o) => o.ticks >= 150).map((o) => o.t), relogio: (performance.now() - t0) / 1000, duracao: v.duration }
    })
  } finally {
    await b.close()
    srv.close()
  }
}

async function cenario(nome, opcoes, comB) {
  console.log(`\n--- ${nome} ---`)
  // A sala grava desde a entrada do anfitrião e não recomeça: se A não ligar,
  // a tentativa seguinte é numa sala nova. B pode repetir na mesma.
  let code, a, b = null
  for (let t = 1; ; t++) {
    code = await reuniao(opcoes)
    a = await entra(code, wavA)
    if (a.ligou || t === 4) break
    nota(`o PC de A não ligou em 20 s (tentativa ${t}) — sala nova`)
    await a.b.close()
  }
  for (let t = 1; comB; t++) {
    b = await entra(code, wavB)
    if (b.ligou || t === 4) break
    nota(`o PC de B não ligou em 20 s (tentativa ${t}) — nova entrada`)
    await b.b.close()
    await sleep(3000)
  }
  chk(a.ligou, 'A ligou com media')
  if (b) {
    chk(b.ligou, 'B ligou com media')
    for (let k = 0; k < 40 && (await a.p.evaluate(() => window.__dlx.publicadores().length)) < 1; k++) await sleep(500)
    chk((await a.p.evaluate(() => window.__dlx.publicadores().length)) === 1, 'A recebe a media de B')
  }
  await sleep(SEGUNDOS * 1000)
  // A sessão viva é a pasta `tmp-*` mais recente: este servidor só serve este teste.
  const viva = readdirSync(REC_DIR).filter((d) => d.startsWith('tmp-'))
    .sort((x, y) => statSync(join(REC_DIR, y)).mtimeMs - statSync(join(REC_DIR, x)).mtimeMs)[0]
  await a.p.evaluate(() => window.__dlx.gravar(false))
  const pasta = join(SAIDA, nome)
  const copiou = await guardaPistas(viva, join(pasta, 'pistas'))
  const item = await espera(code)
  await a.b.close()
  if (b) await b.b.close()
  chk(item?.status === 'ready', `a gravação terminou em ready → ${item?.status} ${item?.failure_reason ?? ''}`)
  if (item?.status !== 'ready') return

  // Controlo: sem buracos nas pistas cruas, o resto não prova nada.
  let silencio = 0
  if (copiou) {
    for (const f of readdirSync(join(pasta, 'pistas')).filter((f) => f.endsWith('.ogg'))) {
      const g = buracos(join(pasta, 'pistas', f))
      silencio = Math.max(silencio, g.total)
      nota(`pista crua ${f}: ${g.pacotes} pacotes até ${g.fim.toFixed(2)} s; ${g.n} buracos de PTS, ${g.total.toFixed(2)} s no total, o maior de ${g.maior.toFixed(2)} s`)
    }
  }
  chk(silencio > 10, `o DTX deixou buracos na pista crua de quem se cala (${silencio.toFixed(2)} s)`)

  const dl = await fetch(`${API}/api/recordings/${item.id}/content?dl=1`, { headers: { Authorization: `Bearer ${tok}` } })
  const webm = join(pasta, 'gravacao.webm')
  mkdirSync(pasta, { recursive: true })
  writeFileSync(webm, Buffer.from(await dl.arrayBuffer()))

  const g = buracos(webm)
  const x = pcm(webm)
  const amostras = x.length / 48000
  nota(`gravação: último PTS de áudio aos ${g.fim.toFixed(2)} s; ${amostras.toFixed(2)} s de amostras; ${g.n} buracos de PTS (${g.total.toFixed(2)} s)`)
  chk(g.n === 0, 'o áudio da gravação não tem buracos de PTS')
  chk(Math.abs(amostras - g.fim) < 0.5, 'as amostras de áudio cobrem a gravação inteira')

  const pa = passos(inicios(x, 440, 500, 1500))
  nota(`A (440 Hz), amostras seguidas: passos de ${pa.join(', ')} ms (esperado 8000)`)
  chk(pa.length >= 2 && pa.every((p) => Math.abs(p - 8000) <= 200), 'a fala de A depois de cada silêncio está onde foi dita (8000 ± 200 ms)')
  if (comB) {
    const tb = inicios(x, 2500, 300, 0)
    const pb = passos(tb)
    const fora = pb.filter((p) => Math.abs(p - 1000) > 100)
    nota(`B (2500 Hz): ${tb.length} toques; passo mínimo ${Math.min(...pb)} ms, máximo ${Math.max(...pb)} ms (esperado 1000)`)
    chk(tb.length >= SEGUNDOS - 4, `a régua de B tem os toques todos (${tb.length} em ${SEGUNDOS} s)`)
    chk(pb.length > 0 && fora.length === 0, `os toques de B estão de segundo a segundo (${fora.length} fora de 1000 ± 100 ms)`)
  }

  const o = await ouveNoChromium(webm)
  const po = passos(o.inicios)
  nota(`no Chromium: A ouve-se aos ${o.inicios.map((t) => (t / 1000).toFixed(2)).join(', ')} s do media; tocou em ${o.relogio.toFixed(1)} s (duration ${o.duracao.toFixed(1)} s)`)
  chk(po.length >= 2 && po.every((p) => Math.abs(p - 8000) <= 300), 'no Chromium, a fala de A ouve-se de 8 em 8 s (± 300 ms)')
  chk(Math.abs(o.relogio - o.duracao) < 3, 'e o ficheiro toca no tempo que diz ter')
}

const quais = (process.env.CENARIOS ?? '1,2,3').split(',')
if (quais.includes('1')) await cenario('1-grelha-dois-publicadores', {}, true)
if (quais.includes('2')) await cenario('2-so-audio-um-publicador', { record_quality: 'audio' }, false)
if (quais.includes('3')) await cenario('3-remux-um-publicador', {}, false)

console.log(`\nficheiros em ${SAIDA}`)
console.log(falhas === 0 ? '\n=== TODOS PASSARAM ===' : `\n=== ${falhas} FALHARAM ===`)
process.exit(falhas === 0 ? 0 : 1)
