// Nitidez e palco imersivo — contra um servidor REAL, duas pessoas, câmara falsa.
//
// O que prova, e com que instrumento:
//   1. Perfil «Aula nítida»: a resolução ENVIADA pela câmara de quem apresenta
//      sobe. Medida pelo `getStats` de uma RTCPeerConnection apanhada por um
//      script de arranque — não pelo número que a própria interface mostra.
//   2. Realce na recepção: o canvas WebGL2 existe por cima do vídeo do orador,
//      desenha a um ritmo medido e o custo por frame aparece.
//   3. Palco imersivo: há máscaras de recorte a chegar à GPU, as camadas são
//      desenhadas, e o deslocamento do fundo responde ao rato.
//   4. `prefers-reduced-motion: reduce` desliga-o.
//   5. Custo de CPU do thread principal (CDP `Performance.getMetrics`) com os
//      efeitos desligados e ligados, na mesma página.
//
// O que NÃO prova: a câmara falsa do Chromium não é uma pessoa — a cobertura da
// máscara é reportada, não afirmada —, e sem GPU a WebGL corre em SwiftShader
// (CPU), por isso os tempos por frame são um tecto pessimista, não a medida de
// um portátil com GPU.
//
//   API=http://127.0.0.1:8190 APP=http://127.0.0.1:5611 node web/e2e/nitidez-imersivo.mjs

import { chromium } from '@playwright/test'
import { entrar } from './sessao.mjs'

// O CI é lento (R118): todas as esperas por media multiplicam por isto.
const FATOR = Number(process.env.E2E_TIMEOUT_FACTOR) || 1
const API = process.env.API ?? 'http://127.0.0.1:8190'
const APP = process.env.APP ?? 'http://127.0.0.1:5611'
const HOST = { email: process.env.HOST_EMAIL ?? 'demo@delonix.co.ao', password: process.env.HOST_PASS ?? 'demo12345' }
const GUEST = { email: process.env.GUEST_EMAIL ?? 'teresa.kiala@delonix.co.ao', password: process.env.GUEST_PASS ?? 'Delonix-UI-2026!' }

let falhas = 0
const medidas = {}
const ok = (c, n, d) => {
  console.log(`  ${c ? '✓' : '✗'} ${n}${d ? `  — ${d}` : ''}`)
  if (!c) falhas++
}
const pausa = (ms) => new Promise((r) => setTimeout(r, ms))

// Apanha as RTCPeerConnection da página para medir por fora da interface.
const APANHAR_PCS = () => {
  const Orig = window.RTCPeerConnection
  window.__pcs = []
  window.RTCPeerConnection = function (...a) {
    const pc = new Orig(...a)
    window.__pcs.push(pc)
    return pc
  }
  window.RTCPeerConnection.prototype = Orig.prototype
  Object.setPrototypeOf(window.RTCPeerConnection, Orig)
}

/** O que chega: vídeo recebido pelo anfitrião (diagnóstico, não asserção). */
const recebido = (page) =>
  page.evaluate(async () => {
    const out = []
    for (const pc of window.__pcs ?? []) {
      if (pc.connectionState === 'closed') continue
      ;(await pc.getStats()).forEach((s) => {
        if (s.type === 'inbound-rtp' && s.kind === 'video') out.push({ w: s.frameWidth ?? 0, h: s.frameHeight ?? 0, fps: s.framesPerSecond ?? 0, dec: s.framesDecoded ?? 0 })
      })
    }
    return out
  })

/** Maior camada de vídeo enviada, média de N amostras espaçadas. */
async function medirEnvio(page, amostras = 4) {
  const leituras = []
  for (let i = 0; i < amostras; i++) {
    leituras.push(
      await page.evaluate(async () => {
        const out = []
        const todas = []
        for (const pc of window.__pcs ?? []) {
          todas.push(pc.connectionState)
          if (pc.connectionState === 'closed') continue
          const r = await pc.getStats()
          r.forEach((s) => {
            if (s.type === 'outbound-rtp' && s.kind === 'video') todas.push(`${s.rid}:${s.frameWidth ?? 0}x${s.frameHeight ?? 0}@${s.framesPerSecond ?? 0} enc=${s.framesEncoded ?? 0} ${s.qualityLimitationReason ?? ''} active=${s.active}`)
            // Só camadas A SAIR: uma camada parada guarda o frameWidth do último frame.
            if (s.type === 'outbound-rtp' && s.kind === 'video' && s.frameWidth && (s.framesPerSecond ?? 0) > 0) {
              out.push({ rid: s.rid ?? '', w: s.frameWidth, h: s.frameHeight, fps: s.framesPerSecond ?? 0, lim: s.qualityLimitationReason })
            }
          })
        }
        const params = (window.__pcs ?? [])
          .flatMap((pc) => pc.getSenders())
          .filter((s) => s.track?.kind === 'video')
          .map((s) => {
            const p = s.getParameters()
            return { hint: s.track.contentHint, deg: p.degradationPreference ?? null, top: p.encodings?.map((e) => [e.rid, e.maxBitrate, e.maxFramerate ?? null]) }
          })
        const cam = (window.__pcs ?? []).flatMap((pc) => pc.getSenders()).find((s) => s.track?.kind === 'video')?.track?.getSettings()
        return { camadas: out, todas, params, cam: cam ? { w: cam.width, h: cam.height, fps: cam.frameRate } : null }
      }),
    )
    await pausa(1500)
  }
  const melhores = leituras.map((l) => l.camadas.reduce((a, b) => (b.w * b.h > (a?.w ?? 0) * (a?.h ?? 0) ? b : a), null)).filter(Boolean)
  const px = melhores.length ? Math.round(melhores.reduce((s, m) => s + m.w * m.h, 0) / melhores.length) : 0
  return { px, ultima: leituras.at(-1), melhores }
}

async function cpu(cdp) {
  const { metrics } = await cdp.send('Performance.getMetrics')
  const m = Object.fromEntries(metrics.map((x) => [x.name, x.value]))
  return { task: m.TaskDuration, script: m.ScriptDuration, t: m.Timestamp }
}
async function cpuDuring(cdp, ms) {
  const a = await cpu(cdp)
  await pausa(ms)
  const b = await cpu(cdp)
  const wall = b.t - a.t
  return { mainThreadPct: Math.round(((b.task - a.task) / wall) * 1000) / 10, scriptPct: Math.round(((b.script - a.script) / wall) * 1000) / 10 }
}

async function abrirDefinicoes(page) {
  if (await page.locator('[data-enh="envio"]').isVisible().catch(() => false)) return
  await page.getByRole('button', { name: /mais opções/i }).first().click()
  await page.getByRole('menuitem', { name: /fundos e efeitos/i }).first().click()
  await page.locator('[data-enh="envio"]').waitFor({ timeout: 15000 * FATOR })
}

async function ligar(page, qual) {
  await abrirDefinicoes(page)
  const input = page.locator(`input[data-enh-toggle="${qual}"]`)
  if (!(await input.isChecked())) await input.check({ force: true })
}

async function entrarNaSala(page, codigo) {
  await page.goto(`${APP}/#/r/${codigo}`, { waitUntil: 'domcontentloaded' })
  await page.getByRole('button', { name: /entrar na sessão/i }).first().click({ timeout: 60000 * FATOR })
  await page.waitForFunction(() => !document.querySelector('.rm-prejoin'), null, { timeout: 90000 * FATOR })
}

const browser = await chromium.launch({
  args: [
    '--use-fake-ui-for-media-stream',
    '--use-fake-device-for-media-stream',
    // Sem GPU no CI a WebGL precisa do SwiftShader explicitamente autorizado.
    '--enable-unsafe-swiftshader',
    '--ignore-gpu-blocklist',
  ],
})
const ctxOpts = { ignoreHTTPSErrors: true, permissions: ['camera', 'microphone'], viewport: { width: 1440, height: 900 } }
const hostCtx = await browser.newContext(ctxOpts)
const guestCtx = await browser.newContext(ctxOpts)
await hostCtx.addInitScript(APANHAR_PCS)
await guestCtx.addInitScript(APANHAR_PCS)
const avisos = []
const host = await hostCtx.newPage()
const guest = await guestCtx.newPage()
for (const [quem, pg] of [['anfitrião', host], ['convidado', guest]]) {
  pg.on('console', (m) => {
    if (/\[(nitidez|imersivo)\]/.test(m.text()) || m.type() === 'error') avisos.push(`${quem}: ${m.text().slice(0, 200)}`)
  })
  pg.on('pageerror', (e) => avisos.push(`${quem} (pageerror): ${String(e).slice(0, 300)}`))
}

console.log('· sessão e sala de formação')
await entrar(host, APP, HOST)
await entrar(guest, APP, GUEST)
const sala = await host.evaluate(async () => {
  const r = await fetch('/api/rooms', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${localStorage.getItem('dx_access')}` },
    body: JSON.stringify({ name: 'e2e nitidez', topology: 'sfu', waiting_room: false, e2ee: false, format: 'training' }),
  })
  return { status: r.status, body: await r.json().catch(() => null) }
})
ok(sala.status < 300 && sala.body?.code, 'sala de formação criada no servidor', `${sala.status} ${sala.body?.code ?? JSON.stringify(sala.body)?.slice(0, 120)}`)
const codigo = sala.body?.code
if (!codigo) {
  await browser.close()
  process.exit(1)
}

await entrarNaSala(host, codigo)
await entrarNaSala(guest, codigo)
// Se houver sala de espera, admite-se como um anfitrião faria.
const admitir = host.locator('.rm-admit-accept').first()
if (await admitir.waitFor({ timeout: 8000 * FATOR }).then(() => true).catch(() => false)) await admitir.click()

const juntos = await host
  .waitForFunction(() => document.querySelectorAll('.rm-tile[data-peer="remoto"] video').length === 1, null, { timeout: 90000 * FATOR })
  .then(() => true)
  .catch(() => false)
ok(juntos, 'as duas pessoas estão na sala (retrato remoto no anfitrião)')

// ── Sugestão ao anfitrião numa sala `training` ────────────────────────────────
ok(await host.locator('[data-enh="sugestao"]').isVisible().catch(() => false), 'o anfitrião vê a sugestão do palco imersivo (sala training)')
ok(!(await guest.locator('[data-enh="sugestao"]').isVisible().catch(() => false)), 'o convidado não a vê (não é anfitrião)')

// ── 1. Perfil de envio nítido (quem apresenta: o convidado) ───────────────────
console.log('· perfil de envio nítido')
await pausa(12000) // o controlo de congestão sobe a banda nos primeiros segundos
const antes = await medirEnvio(guest)
console.log('  · antes:', JSON.stringify(antes.melhores), JSON.stringify(antes.ultima.cam), JSON.stringify(antes.ultima.todas))
await ligar(guest, 'envio')
await pausa(12000)
const depois = await medirEnvio(guest)
console.log('  · depois:', JSON.stringify(depois.melhores), JSON.stringify(depois.ultima.cam), JSON.stringify(depois.ultima.todas))
medidas.envio = { antesPx: antes.px, depoisPx: depois.px, antes: antes.melhores.at(-1), depois: depois.melhores.at(-1), camAntes: antes.ultima.cam, camDepois: depois.ultima.cam }
const p = depois.ultima.params[0]
ok(p?.hint === 'detail' && p?.deg === 'maintain-resolution', 'o sender passou a detail + maintain-resolution', JSON.stringify(p))
ok(depois.px > antes.px, 'a resolução ENVIADA subiu (getStats, média de 4 amostras)', `${antes.px} px → ${depois.px} px`)
const perfil = await guest.locator('[data-enh="envio"]').getAttribute('data-perfil')
ok(perfil === 'sharp', 'a interface diz que o perfil está activo', perfil)
const ui = await guest.locator('[data-enh="envio"] dd').allTextContents()
console.log('  · a interface mostra:', JSON.stringify(ui))

console.log('  · o anfitrião recebe:', JSON.stringify(await recebido(host)))

// ── 2. Realce na recepção (quem vê: o anfitrião) ──────────────────────────────
console.log('· realce de nitidez na recepção')
const cdp = await hostCtx.newCDPSession(host)
await cdp.send('Performance.enable')
// Orador em destaque: fixar o retrato remoto.
await host.locator('.rm-tile[data-peer="remoto"]').first().dblclick()
await host.waitForSelector('.rm-stage__main .rm-tile[data-peer="remoto"]', { timeout: 15000 * FATOR })
// Sem frames a chegar não há nada a realçar — e um teste que ligasse o filtro
// a um vídeo parado mediria zero e chamar-lhe-ia «lento».
const aChegar = await host
  .waitForFunction(() => (document.querySelector('.rm-stage__main .rm-tile[data-peer="remoto"] > video')?.videoWidth ?? 0) > 0, null, { timeout: 30000 * FATOR })
  .then(() => true)
  .catch(() => false)
console.log('  · depois de fixar, o anfitrião recebe:', JSON.stringify(await recebido(host)))
ok(aChegar, 'o vídeo do orador em destaque está a chegar ao anfitrião', await host.evaluate(() => { const v = document.querySelector('.rm-stage__main .rm-tile[data-peer="remoto"] > video'); return v ? `${v.videoWidth}×${v.videoHeight}` : 'sem vídeo' }))
if (!aChegar) {
  // Sem vídeo a chegar, as asserções da recepção mediriam a rede e não o efeito.
  // Medido a 2026-09-16: a PC ao SFU cai para `disconnected` segundos depois de
  // entrar, também na branch base sem este código. A recepção prova-se no banco
  // de ensaio (`palco-imersivo.mjs`); aqui fica UMA falha, com a causa.
  console.log('  · a recepção não foi ensaiada contra o servidor: o vídeo remoto não chegou (ver palco-imersivo.mjs)')
  if (avisos.length) console.log('\n· avisos da consola:', JSON.stringify(avisos, null, 2))
  console.log('\n· medições:', JSON.stringify(medidas, null, 2))
  await browser.close()
  console.log(`\n=== ${falhas} FALHARAM ===`)
  process.exit(1)
}
medidas.cpuBase = await cpuDuring(cdp, 8000)
const gl2 = await host.evaluate(() => !!document.createElement('canvas').getContext('webgl2'))
const renderer = await host.evaluate(() => {
  const gl = document.createElement('canvas').getContext('webgl2')
  const ext = gl?.getExtension('WEBGL_debug_renderer_info')
  return gl ? String(ext ? gl.getParameter(ext.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER)) : null
})
medidas.gl = renderer
ok(gl2, 'o browser de teste tem WebGL2', renderer)
await ligar(host, 'realce')
const canvasRealce = await host
  .waitForSelector('.rm-stage__main .rm-tile[data-peer="remoto"] > canvas[data-realce="on"]', { timeout: 15000 * FATOR })
  .then(() => true)
  .catch(() => false)
ok(canvasRealce, 'o canvas do realce está por cima do vídeo do orador')
for (let i = 0; i < 6 && process.env.DEPURAR; i++) {
  console.log('  · dbg:', JSON.stringify(await host.evaluate(() => [...document.querySelectorAll('canvas[data-realce]')].map((c) => ({ w: c.width, f: c.dataset.frames, prevIsVideo: c.previousElementSibling?.tagName, vw: c.previousElementSibling?.videoWidth, peer: c.parentElement?.dataset.peerId, inStage: !!c.closest('.rm-stage__main'), disp: c.style.display })))))
  await pausa(1000)
}
await host.locator('[data-hud="realce"]').waitFor({ timeout: 10000 * FATOR }).catch(() => {})
await pausa(4000)
const hudRealce = (await host.locator('[data-hud="realce"] .enh-pill__cost').textContent().catch(() => '')) ?? ''
const fpsRealce = Number(/(\d+)\s*fps/.exec(hudRealce)?.[1] ?? 0)
const msRealce = Number((/·\s*([\d.,]+)\s*ms/.exec(hudRealce)?.[1] ?? '0').replace(',', '.'))
medidas.realce = { hud: hudRealce, fps: fpsRealce, p95Ms: msRealce, cpu: await cpuDuring(cdp, 8000) }
ok(fpsRealce > 5, 'o filtro corre com fps medido', hudRealce)
const dimsRealce = await host.evaluate(() => {
  const c = document.querySelector('canvas[data-realce="on"]')
  const v = c?.previousElementSibling
  return c ? { cw: c.width, ch: c.height, vw: v?.videoWidth, vh: v?.videoHeight, box: [v?.clientWidth, v?.clientHeight] } : null
})
console.log('  · canvas/vídeo:', JSON.stringify(dimsRealce))
ok(!!dimsRealce && dimsRealce.cw > 1 && Math.abs(dimsRealce.cw / dimsRealce.ch - dimsRealce.vw / dimsRealce.vh) < 0.02, 'o canvas tem a proporção do vídeo', JSON.stringify(dimsRealce))
// Ver original esconde o canvas sem o parar.
const verOriginal = host.locator('[data-hud="realce"]').getByRole('button', { name: /ver original/i })
if (await verOriginal.isVisible().catch(() => false)) {
  await verOriginal.click()
  ok((await host.evaluate(() => getComputedStyle(document.querySelector('canvas[data-realce="on"]')).visibility)) === 'hidden', '«Ver original» mostra o vídeo sem o filtro')
  await verOriginal.click()
} else {
  ok(false, '«Ver original» existe no HUD do realce', `avisos: ${JSON.stringify(avisos)}`)
}

// ── 3. Palco imersivo ─────────────────────────────────────────────────────────
console.log('· palco imersivo')
await ligar(host, 'imersivo')
const canvasImersivo = await host
  .waitForSelector('.rm-stage__main .rm-tile[data-peer="remoto"] > canvas[data-imersivo="on"]', { timeout: 20000 * FATOR })
  .then(() => true)
  .catch(() => false)
ok(canvasImersivo, 'o canvas do palco imersivo está por cima do vídeo do orador')
ok(
  await host.waitForFunction(() => !document.querySelector('canvas[data-realce="on"]'), null, { timeout: 5000 * FATOR }).then(() => true).catch(() => false),
  'o realce sai do mesmo vídeo (os efeitos não se empilham)',
)
const mascaras = await host
  .waitForFunction(() => Number(document.querySelector('canvas[data-imersivo="on"]')?.dataset.mascaras ?? 0) > 5, null, { timeout: 60000 * FATOR })
  .then(() => host.evaluate(() => ({ ...document.querySelector('canvas[data-imersivo="on"]').dataset })))
  .catch(() => host.evaluate(() => ({ ...(document.querySelector('canvas[data-imersivo="on"]')?.dataset ?? {}) })))
ok(Number(mascaras.mascaras) > 5 && mascaras.camadas === 'fundo,sombra,pessoa', 'o segmentador produz máscaras e as camadas são desenhadas', JSON.stringify(mascaras))

const area = await host.locator('.rm-stagearea').boundingBox()
await host.mouse.move(area.x + 5, area.y + area.height / 2, { steps: 5 })
await pausa(900)
const esq = Number(await host.locator('canvas[data-imersivo="on"]').getAttribute('data-bg-x'))
await host.mouse.move(area.x + area.width - 5, area.y + area.height / 2, { steps: 5 })
await pausa(900)
const dir = Number(await host.locator('canvas[data-imersivo="on"]').getAttribute('data-bg-x'))
const fonte = await host.locator('canvas[data-imersivo="on"]').getAttribute('data-fonte')
ok(esq < -0.02 && dir > 0.02, 'o fundo desloca-se com o movimento (rato à esquerda → à direita)', `bgX ${esq} → ${dir}, fonte=${fonte}`)
await host.locator('[data-hud="imersivo"]').waitFor({ timeout: 10000 * FATOR }).catch(() => {})
await pausa(3000)
const hudImersivo = (await host.locator('[data-hud="imersivo"] .enh-pill__cost').textContent().catch(() => '')) ?? ''
medidas.imersivo = {
  hud: hudImersivo,
  cpu: await cpuDuring(cdp, 8000),
  estado: await host.locator('[data-enh="imersivo"] [role="status"]').textContent().catch(() => null),
}
ok(/\d+\s*fps/.test(hudImersivo), 'o palco imersivo mostra o custo medido', hudImersivo)

// ── 4. Movimento reduzido desliga-o ──────────────────────────────────────────
console.log('· prefers-reduced-motion')
await host.emulateMedia({ reducedMotion: 'reduce' })
const saiu = await host
  .waitForFunction(() => !document.querySelector('canvas[data-imersivo="on"]'), null, { timeout: 5000 * FATOR })
  .then(() => true)
  .catch(() => false)
ok(saiu, 'com movimento reduzido o palco imersivo desliga-se')
const aviso = await host.locator('[data-enh="imersivo"] .dx-alert').textContent().catch(() => '')
ok(/movimento reduzido/i.test(aviso ?? ''), 'e diz porquê', aviso)
// O realce não é movimento: volta a ocupar o vídeo.
ok(
  await host.waitForSelector('canvas[data-realce="on"]', { timeout: 5000 * FATOR }).then(() => true).catch(() => false),
  'o realce (sem movimento) volta a aplicar-se ao orador',
)

if (avisos.length) console.log('\n· avisos da consola:', JSON.stringify(avisos, null, 2))
console.log('\n· medições:', JSON.stringify(medidas, null, 2))
await browser.close()
console.log(`\n=== ${falhas === 0 ? 'TODAS PASSARAM' : `${falhas} FALHARAM`} ===`)
process.exit(falhas ? 1 : 0)
