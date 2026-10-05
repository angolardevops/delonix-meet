#!/usr/bin/env node
// Um browser na sala, a ouvir um TELEFONE que entra pela ponte (ADR-0010) — e a
// ser ouvido por ele.
//
// O browser é um Chromium com a pilha REAL do cliente (`e2e/harness.html`:
// `SfuCall` e `Signaling`). O microfone falso toca um tom de 440 Hz; o telefone
// — lançado por quem chama este guião, com `scripts/softphone-prova.sh` — toca
// 1000 Hz. Aqui mede-se o sentido telefone → browser:
//
//   1. o telefone aparece na sala como publicador;
//   2. o browser RECEBE o áudio dele (pacotes do `inbound-rtp`; antes de ele
//      entrar não há nenhum);
//   3. o que descodifica É o tom do telefone: o pico do espectro das amostras
//      (Web Audio sobre a faixa remota) está nos 1000 Hz, e não nos 440 Hz do
//      próprio browser.
//
// O outro sentido (browser → telefone) mede-o o softphone, que grava o que ouve
// e procura os 440 Hz.
//
// Uso (a partir de web/, com um Vite a servir o arnês e a fazer de proxy à API):
//   API=http://172.30.50.12:8180 APP=http://localhost:5199 \
//   EMAIL=… PASSWORD=… SALA=<código> node e2e/telefone-na-sala.mjs
// Não corre no CI: precisa do bordo, do FreeSWITCH e de um telefone
// (`scripts/pbx-tronco-prova.sh browser`).
import { chromium } from '@playwright/test'
import { mkdtempSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

const API = process.env.API ?? 'http://127.0.0.1:8180'
const APP = process.env.APP ?? 'http://localhost:5173'
const { EMAIL, PASSWORD, SALA } = process.env
// Um runner lento declara-o (R118): as ESPERAS esticam com o factor; a janela
// de medição não, que essa é do que se mede.
const FATOR = Number(process.env.E2E_TIMEOUT_FACTOR) || 1
const ESPERA = Number(process.env.ESPERA ?? 90) * FATOR // quanto se espera pelo telefone
const SEGUNDOS = Number(process.env.SEGUNDOS ?? 12) // quanto se mede depois de ele entrar
const TOM_DO_TELEFONE = Number(process.env.TOM_DO_TELEFONE ?? 1000)
const TOM_DO_BROWSER = 440
if (!EMAIL || !PASSWORD || !SALA) {
  console.error('faltam EMAIL, PASSWORD e SALA no ambiente')
  process.exit(2)
}

let falhas = 0
const chk = (c, n, d) => {
  console.log(`  ${c ? '✓' : '✗'} ${n}${d ? `  — ${d}` : ''}`)
  if (!c) falhas++
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const j = (u, o = {}) =>
  fetch(u, {
    ...o,
    headers: {
      ...(o.token ? { Authorization: `Bearer ${o.token}` } : {}),
      ...(o.body ? { 'Content-Type': 'application/json' } : {}),
    },
  }).then(async (r) => ({ s: r.status, j: await r.json().catch(() => null) }))

// ---------- o microfone falso: 440 Hz, PCM de 16 bits a 48 kHz ----------
function wavDeTom(hz, segundos, amplitude = 0.5, sr = 48000) {
  const n = sr * segundos
  const b = Buffer.alloc(44 + n * 2)
  b.write('RIFF', 0); b.writeUInt32LE(36 + n * 2, 4); b.write('WAVEfmt ', 8)
  b.writeUInt32LE(16, 16); b.writeUInt16LE(1, 20); b.writeUInt16LE(1, 22)
  b.writeUInt32LE(sr, 24); b.writeUInt32LE(sr * 2, 28); b.writeUInt16LE(2, 32); b.writeUInt16LE(16, 34)
  b.write('data', 36); b.writeUInt32LE(n * 2, 40)
  for (let i = 0; i < n; i++) b.writeInt16LE(Math.round(amplitude * 32767 * Math.sin((2 * Math.PI * hz * i) / sr)), 44 + i * 2)
  return b
}
const wav = join(mkdtempSync(join(tmpdir(), 'telefone-na-sala-')), 'tom.wav')
writeFileSync(wav, wavDeTom(TOM_DO_BROWSER, 10))

// ---------- a sala ----------
const login = await j(`${API}/api/auth/login`, { method: 'POST', body: JSON.stringify({ email: EMAIL, password: PASSWORD }) })
if (login.s !== 200) {
  console.error(`login devolveu ${login.s}`)
  process.exit(2)
}
const tok = login.j.access_token
const jr = (await j(`${API}/api/rooms/${SALA}/join`, { token: tok, method: 'POST' })).j
if (!jr?.room_token) {
  console.error('não consegui entrar na sala (sem room_token)')
  process.exit(2)
}

const browser = await chromium.launch({
  args: [
    '--use-fake-ui-for-media-stream',
    '--use-fake-device-for-media-stream',
    `--use-file-for-fake-audio-capture=${wav}`,
    '--autoplay-policy=no-user-gesture-required',
  ],
})
const ctx = await browser.newContext({ permissions: ['camera', 'microphone'], locale: 'pt-PT' })
// Guarda cada RTCPeerConnection: é por elas que se chega às faixas remotas.
await ctx.addInitScript(() => {
  const Original = window.RTCPeerConnection
  window.__pcs = []
  window.RTCPeerConnection = function (...a) {
    const pc = new Original(...a)
    window.__pcs.push(pc)
    return pc
  }
  window.RTCPeerConnection.prototype = Original.prototype
})
const p = await ctx.newPage()
const url = `${APP}/e2e/harness.html?token=${encodeURIComponent(jr.room_token)}&code=${SALA}&access=${encodeURIComponent(tok)}&som=cru`
await p.goto(url, { waitUntil: 'domcontentloaded', timeout: 180000 * FATOR })
let pronto = false
for (let k = 0; k < 120 * FATOR && !pronto; k++) {
  await sleep(1000)
  pronto = await p.evaluate(() => window.__dlx?.ready === true && window.__dlx.state === 'connected').catch(() => false)
}
const erro = await p.evaluate(() => window.__dlx?.error ?? null).catch(() => 'página sem arnês')
chk(pronto, 'o browser entrou na sala com a pilha real do cliente', erro ? `erro do arnês: ${erro}` : await p.evaluate(() => window.__dlx.state))
if (!pronto) {
  await browser.close()
  process.exit(1)
}
console.log('BROWSER-NA-SALA') // quem chama espera por esta linha para ligar o telefone

// A energia de áudio RECEBIDA até agora, somada por todos os `inbound-rtp`.
const energia = () =>
  p.evaluate(async () => {
    let e = 0, d = 0, pacotes = 0
    for (const s of await window.__dlx.raw()) {
      if (s.type === 'inbound-rtp' && s.kind === 'audio') {
        e += s.totalAudioEnergy ?? 0
        d += s.totalSamplesDuration ?? 0
        pacotes += s.packetsReceived ?? 0
      }
    }
    return { e, d, pacotes }
  })
// O detalhe de cada faixa de áudio recebida: separa «não chegam pacotes» de
// «chegam e descodificam em silêncio».
const detalhe = () =>
  p.evaluate(async () => {
    const stats = await window.__dlx.raw()
    const codecs = Object.fromEntries(stats.filter((s) => s.type === 'codec').map((s) => [s.id, `${s.mimeType}/${s.clockRate}${s.channels ? `/${s.channels}` : ''} pt=${s.payloadType} ${s.sdpFmtpLine ?? ''}`]))
    const faixas = window.__pcs.flatMap((pc) => pc.getReceivers().map((r) => r.track)).filter((t) => t?.kind === 'audio')
    return {
      faixas: faixas.map((t) => `${t.id.slice(0, 8)} ${t.readyState}${t.muted ? ' MUDA' : ''}`),
      entradas: stats
        .filter((s) => s.type === 'inbound-rtp' && s.kind === 'audio')
        .map((s) => ({
          mid: s.mid, ssrc: s.ssrc, codec: codecs[s.codecId] ?? '(sem codec)',
          pacotes: s.packetsReceived, bytes: s.bytesReceived, perdidos: s.packetsLost, descartados: s.packetsDiscarded,
          amostras: s.totalSamplesReceived, ocultadas: s.concealedSamples, silencioOcultado: s.silentConcealedSamples,
          nivel: s.audioLevel, energia: s.totalAudioEnergy, emitidas: s.jitterBufferEmittedCount,
        })),
    }
  })
// O espectro do que o browser está a receber. O grafo de Web Audio monta-se UMA
// vez, depois de o telefone entrar, e lê-se várias: montá-lo a cada leitura
// dava leituras vazias com a máquina carregada.
const montarEspectro = () =>
  p.evaluate(async () => {
    const faixas = window.__pcs.flatMap((pc) => pc.getReceivers().map((r) => r.track)).filter((t) => t?.kind === 'audio' && t.readyState === 'live')
    if (!faixas.length) return false
    const ac = (window.__ac = new window.AudioContext({ sampleRate: 48000 }))
    await ac.resume()
    const fluxo = new MediaStream(faixas)
    // No Chromium, uma faixa remota só alimenta o Web Audio se também estiver
    // ligada a um elemento de media.
    const el = (window.__el = new Audio())
    el.muted = true
    el.srcObject = fluxo
    await el.play().catch(() => {})
    const an = (window.__an = ac.createAnalyser())
    an.fftSize = 8192
    an.smoothingTimeConstant = 0
    // Uma fonte POR FAIXA, todas somadas no analisador. Uma fonte feita do
    // fluxo inteiro só lê UMA das faixas de áudio — e o cliente tem sempre uma
    // faixa remota muda (um lugar vago): quando calhava ser essa, lia-se
    // silêncio com os pacotes a chegar (medido: uma corrida em cada três).
    window.__fontes = faixas.map((t) => {
      const fonte = ac.createMediaStreamSource(new MediaStream([t]))
      fonte.connect(an)
      return fonte
    })
    return ac.state === 'running'
  })
// O pico entre 200 e 3400 Hz, e o nível nos 1000 Hz e nos 440 Hz.
const lerEspectro = () =>
  p.evaluate(() => {
    const an = window.__an
    if (!an) return null
    const db = new Float32Array(an.frequencyBinCount)
    an.getFloatFrequencyData(db)
    const hz = window.__ac.sampleRate / an.fftSize
    let melhor = -Infinity, onde = 0
    for (let i = Math.ceil(200 / hz); i < Math.floor(3400 / hz); i++) if (db[i] > melhor) { melhor = db[i]; onde = i }
    const em = (f) => Math.max(...Array.from({ length: 5 }, (_, k) => db[Math.round(f / hz) - 2 + k]))
    const r = (x) => (Number.isFinite(x) ? Math.round(x) : -200)
    return { hz: Math.round(onde * hz), db: r(melhor), db1000: r(em(1000)), db440: r(em(440)) }
  })

// ---------- antes do telefone: ninguém a publicar, nenhum áudio a chegar ----------
const antes = await energia()
const sozinho = await p.evaluate(() => window.__dlx.publicadores().length)
chk(sozinho === 0 && antes.e === 0, 'antes do telefone: o browser está sozinho e não recebe áudio nenhum', `${sozinho} publicador(es), energia ${antes.e}`)

// ---------- o telefone entra ----------
let entrou = false
for (let k = 0; k < ESPERA && !entrou; k++) {
  await sleep(1000)
  entrou = (await p.evaluate(() => window.__dlx.publicadores().length)) > 0
}
chk(entrou, 'o telefone apareceu na sala como publicador', entrou ? undefined : `ninguém entrou em ${ESPERA} s`)
if (!entrou) {
  await browser.close()
  process.exit(1)
}

// ---------- o que o browser ouve dele ----------
// Mede-se nas AMOSTRAS descodificadas (o espectro), não no `audioLevel` nem no
// `totalAudioEnergy` do `inbound-rtp`: neste Chromium, sem saída de áudio real,
// esses dois vêm a zero com o tom lá — medido: energia 0 com 1002 Hz a −26 dB.
await sleep(1500) // a renegociação da subscrição acaba de chegar
const grafo = await montarEspectro().catch(() => false)
const antesDeOuvir = await energia()
const leituras = []
for (let k = 0; k < SEGUNDOS; k++) {
  await sleep(1000)
  const m = await lerEspectro().catch(() => null)
  if (m) leituras.push(m)
}
const depois = await energia()
const chegaram = depois.pacotes - antesDeOuvir.pacotes
chk(chegaram > 20 * (SEGUNDOS - 2), 'o browser recebe o áudio do telefone', `${chegaram} pacotes em ${SEGUNDOS} s`)
const comTom = leituras.filter((m) => Math.abs(m.hz - TOM_DO_TELEFONE) <= 30 && m.db >= -60)
const ouviu = grafo && comTom.length >= Math.ceil(SEGUNDOS / 2)
const resumo = (v) => (v.length ? `${Math.min(...v)} a ${Math.max(...v)}` : 'sem leituras')
chk(
  ouviu,
  `o que ele descodifica É o tom do telefone: pico do espectro nos ${TOM_DO_TELEFONE} Hz`,
  `${comTom.length} de ${leituras.length} leituras; pico em ${resumo(comTom.map((m) => m.hz))} Hz, a ${resumo(comTom.map((m) => m.db))} dB`,
)
const margem = comTom.map((m) => m.db1000 - m.db440)
chk(
  comTom.length > 0 && Math.min(...margem) >= 20,
  `e não o seu próprio tom de ${TOM_DO_BROWSER} Hz`,
  comTom.length ? `os 1000 Hz ficam ${Math.min(...margem)} dB ou mais acima dos 440 Hz` : 'sem medição',
)
if (!ouviu) {
  const d = await detalhe().catch((e) => ({ erro: String(e) }))
  console.log(`    grafo de Web Audio a correr: ${grafo}; leituras: ${JSON.stringify(leituras.slice(0, 4))}`)
  console.log(`    faixas de áudio remotas: ${JSON.stringify(d.faixas ?? d)}`)
  for (const e of d.entradas ?? []) console.log(`    inbound-rtp: ${JSON.stringify(e)}`)
}

// O browser fica na sala até o telefone desligar: é ele que mede o outro sentido.
for (let k = 0; k < 120 * FATOR; k++) {
  if ((await p.evaluate(() => window.__dlx.publicadores().length)) === 0) break
  await sleep(1000)
}
await browser.close()
console.log(falhas === 0 ? '✓ telefone → browser: tudo medido' : `✗ telefone → browser: ${falhas} falha(s)`)
process.exit(falhas === 0 ? 0 : 1)
