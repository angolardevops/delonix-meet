#!/usr/bin/env node
// Matriz de rede degradada — mede a chamada com a rede a ser DEGRADADA DE
// VERDADE, não simulada em código.
//
// Como funciona: o servidor corre num contentor com CAP_NET_ADMIN, e o `tc
// netem` é aplicado à interface dele. Isso molda o caminho REAL — sinalização
// e media, UDP incluído. Dois Chromium a sério (media falsa determinista, para
// duas execuções serem comparáveis) carregam a PILHA REAL do cliente através
// do arnês em `web/e2e/harness.html`: o mesmo `SfuCall` e o mesmo `Signaling`
// que a aplicação usa.
//
// Porque não `--use-fake-network` nem estrangulamento do DevTools: nenhum dos
// dois afecta UDP, e é UDP que transporta a media. Estrangular só o HTTP daria
// um número bonito e sem relação com a chamada.
//
// Uso:
//   node e2e/netem-matrix.mjs                 # matriz completa
//   node e2e/netem-matrix.mjs --only 5%-loss  # um cenário
//   SETTLE_MS=25000 node e2e/netem-matrix.mjs # janela de medição maior
import { chromium } from '@playwright/test'
import { execFileSync } from 'node:child_process'

const API = process.env.API ?? 'http://127.0.0.1:8180'
const APP = process.env.APP ?? 'http://localhost:5173'
const CONTAINER = process.env.CONTAINER ?? 'dlx-srv'
const IFACE = process.env.IFACE ?? 'eth0'
const SETTLE_MS = Number(process.env.SETTLE_MS ?? 14_000)
const EMAIL = process.env.EMAIL ?? 'a@teste.local'
const PASSWORD = process.env.PASSWORD ?? 'UmaPasswordForte123!'

/** Cenários do §4.5 do mandato. `netem` vazio = referência sem degradação. */
const SCENARIOS = [
  { name: 'referência', netem: [] },
  { name: 'latência-100ms', netem: ['delay', '50ms'] },
  { name: 'latência-250ms', netem: ['delay', '125ms'] },
  { name: 'perda-2%', netem: ['loss', '2%'] },
  { name: 'perda-5%', netem: ['loss', '5%'] },
  { name: 'perda-10%', netem: ['loss', '10%'] },
  { name: 'perda-20%', netem: ['loss', '20%'] },
  { name: 'jitter-variável', netem: ['delay', '60ms', '40ms', 'distribution', 'normal'] },
  { name: '3G-típico', netem: ['delay', '100ms', '30ms', 'loss', '1.5%', 'rate', '1mbit'] },
  { name: '4G-congestionado', netem: ['delay', '40ms', '15ms', 'loss', '0.5%', 'rate', '5mbit'] },
  { name: 'wifi-congestionado', netem: ['delay', '25ms', '25ms', 'loss', '3%'] },
  { name: 'banda-128kbps', netem: ['rate', '128kbit'] },
]

const docker = (...args) =>
  execFileSync('docker', args, { stdio: ['ignore', 'pipe', 'pipe'] }).toString().trim()

function clearNetem() {
  try {
    docker('exec', CONTAINER, 'tc', 'qdisc', 'del', 'dev', IFACE, 'root')
  } catch {
    /* não havia nenhum — é o estado normal antes do primeiro cenário */
  }
}

function applyNetem(spec) {
  clearNetem()
  if (spec.length === 0) return
  docker('exec', CONTAINER, 'tc', 'qdisc', 'add', 'dev', IFACE, 'root', 'netem', ...spec)
}

const j = (url, opts) => fetch(url, opts).then((r) => r.json())
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

async function novaSala(access, nome) {
  const room = await j(`${API}/api/rooms`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${access}` },
    body: JSON.stringify({ name: nome, topology: 'sfu' }),
  })
  const jr = await j(`${API}/api/rooms/${room.code}/join`, {
    method: 'POST',
    headers: { Authorization: `Bearer ${access}` },
  })
  return { code: room.code, token: jr.room_token }
}

async function corre(cenario, access, browser) {
  applyNetem(cenario.netem)
  const { code, token } = await novaSala(access, `netem-${cenario.name}`)
  const url = `${APP}/e2e/harness.html?token=${encodeURIComponent(token)}&code=${code}&access=${encodeURIComponent(access)}`

  const ctxs = []
  const pages = []
  for (let i = 0; i < 2; i++) {
    const ctx = await browser.newContext({ ignoreHTTPSErrors: true })
    const p = await ctx.newPage()
    await p.goto(url)
    ctxs.push(ctx)
    pages.push(p)
  }

  // Tempo até haver media nos DOIS sentidos. É o número que o utilizador
  // sente como «a chamada demorou a pegar».
  //
  // Sonda-se a 1500 ms e NÃO a 500. O Chrome actualiza o `getStats()` cerca de
  // uma vez por segundo; sondar mais depressa devolve duas leituras com o mesmo
  // carimbo temporal, o delta dá zero, e o teste conclui «sem media» numa
  // chamada perfeitamente saudável. Custou uma matriz inteira de falsos
  // negativos para se perceber — incluindo no cenário de REFERÊNCIA, que é
  // precisamente onde um falso negativo se nota.
  const t0 = Date.now()
  let tempoAteMedia = null
  const limite = t0 + 45_000
  while (Date.now() < limite) {
    await sleep(1500)
    const r = await Promise.all(pages.map((p) => p.evaluate(async () => (await window.__dlx.qos())?.downKbps ?? 0)))
    if (r.every((d) => d > 0)) {
      tempoAteMedia = Date.now() - t0
      break
    }
  }

  // Janela de medição: amostras de 2 em 2 s, e fica-se com a MEDIANA. A média
  // deixa um único pico distorcer o cenário inteiro.
  const amostras = []
  const fim = Date.now() + SETTLE_MS
  while (Date.now() < fim) {
    await sleep(2000)
    const r = await Promise.all(pages.map((p) => p.evaluate(() => window.__dlx.qos())))
    for (const q of r) if (q) amostras.push(q)
  }
  const estados = (await Promise.all(pages.map((p) => p.evaluate(() => window.__dlx.states)))).flat()

  for (const c of ctxs) await c.close()

  const mediana = (xs) => {
    if (!xs.length) return null
    const s = [...xs].sort((a, b) => a - b)
    return s[Math.floor(s.length / 2)]
  }
  return {
    cenario: cenario.name,
    tempoAteMedia,
    amostras: amostras.length,
    score: mediana(amostras.map((a) => a.score)),
    perdaPct: mediana(amostras.map((a) => a.lossPct)),
    jitterMs: mediana(amostras.map((a) => a.jitterMs)),
    downKbps: mediana(amostras.map((a) => a.downKbps)),
    congelamentoMs: mediana(amostras.map((a) => a.freezeMs)),
    ocultacaoPct: mediana(amostras.map((a) => Math.round(a.concealmentRatio * 1000) / 10)),
    par: amostras.at(-1)?.candidatePair ?? null,
    recuperacoes: estados.filter((s) => s === 'reconnecting').length,
  }
}

const soIsto = process.argv.includes('--only') ? process.argv[process.argv.indexOf('--only') + 1] : null

const login = await j(`${API}/api/auth/login`, {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({ email: EMAIL, password: PASSWORD }),
})
if (!login.access_token) throw new Error(`login falhou: ${JSON.stringify(login)}`)

const browser = await chromium.launch({
  args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream', '--autoplay-policy=no-user-gesture-required'],
})

const resultados = []
try {
  for (const c of SCENARIOS) {
    if (soIsto && c.name !== soIsto) continue
    process.stderr.write(`· ${c.name}…\n`)
    resultados.push(await corre(c, login.access_token, browser))
  }
} finally {
  clearNetem()
  await browser.close()
}

const col = (v, n) => String(v ?? '—').padStart(n)
console.log('\ncenário             t→media  amostras  score  perda%  jitter  ↓kbps  congel  ocult%  recup')
console.log('─'.repeat(96))
for (const r of resultados) {
  console.log(
    r.cenario.padEnd(20) +
      col(r.tempoAteMedia != null ? `${r.tempoAteMedia}ms` : 'SEM MEDIA', 8) +
      col(r.amostras, 10) + col(r.score, 7) + col(r.perdaPct, 8) +
      col(r.jitterMs, 8) + col(r.downKbps, 7) + col(r.congelamentoMs, 8) +
      col(r.ocultacaoPct, 8) + col(r.recuperacoes, 7),
  )
}
console.log(`\nJanela de medição: ${SETTLE_MS} ms por cenário · mediana das amostras dos dois participantes.`)
