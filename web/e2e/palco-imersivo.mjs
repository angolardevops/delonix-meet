// Realce e palco imersivo — banco de ensaio com GPU e câmara falsa, sem rede.
//
// Monta os hooks e componentes verdadeiros (`palco-imersivo.entry.tsx`) no Vite
// do próprio web/ e mede:
//   · realce: frames desenhados, fps, p95 por frame (com `gl.finish`), CPU do
//     thread principal (CDP), «Ver original», e que a decisão do orçamento bate
//     com o custo medido (corre se cabe; desliga-se e diz porquê se não cabe);
//   · palco imersivo: máscaras do segmentador na GPU, camadas, paralaxe pelo
//     rato e pelo giroscópio simulado, envelope da fala, custo;
//   · `prefers-reduced-motion` desliga o imersivo e devolve o vídeo ao realce;
//   · 375 px: nada transborda e os controlos continuam alcançáveis por teclado.
//
// GPU: `GPU=egl` (omissão) usa o Chromium completo em headless com ANGLE sobre
// a GPU da máquina; `GPU=swiftshader` usa o render por software — o tecto
// pessimista de uma máquina sem aceleração.
//
//   APP=http://127.0.0.1:5611 node web/e2e/palco-imersivo.mjs
//   APP=http://127.0.0.1:5611 GPU=swiftshader SRC=1920x1080 node web/e2e/palco-imersivo.mjs

import { chromium } from '@playwright/test'

const FATOR = Number(process.env.E2E_TIMEOUT_FACTOR) || 1
const APP = process.env.APP ?? 'http://127.0.0.1:5611'
const GPU = process.env.GPU ?? 'egl'
const [SW, SH] = (process.env.SRC ?? '1280x720').split('x').map(Number)
const OUT = process.env.OUT ?? null

let falhas = 0
const medidas = { gpu: GPU, fonte: `${SW}x${SH}` }
const ok = (c, n, d) => {
  console.log(`  ${c ? '✓' : '✗'} ${n}${d ? `  — ${d}` : ''}`)
  if (!c) falhas++
}
const pausa = (ms) => new Promise((r) => setTimeout(r, ms))

const launch =
  GPU === 'swiftshader'
    ? { args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream', '--enable-unsafe-swiftshader'] }
    : { channel: 'chromium', args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream', '--use-gl=angle', '--use-angle=gl-egl', '--ignore-gpu-blocklist', '--enable-gpu'] }
const browser = await chromium.launch(launch)
const ctx = await browser.newContext({ permissions: ['camera'], viewport: { width: 1440, height: 900 } })
const page = await ctx.newPage()
const erros = []
page.on('pageerror', (e) => erros.push(String(e).slice(0, 300)))
page.on('console', (m) => {
  if (m.type() === 'error' || /\[(nitidez|imersivo)\]/.test(m.text())) erros.push(m.text().slice(0, 300))
})
const cdp = await ctx.newCDPSession(page)
await cdp.send('Performance.enable')
const cpu = async () => {
  const { metrics } = await cdp.send('Performance.getMetrics')
  const m = Object.fromEntries(metrics.map((x) => [x.name, x.value]))
  return { task: m.TaskDuration, t: m.Timestamp }
}
const cpuDurante = async (ms) => {
  const a = await cpu()
  await pausa(ms)
  const b = await cpu()
  return Math.round(((b.task - a.task) / (b.t - a.t)) * 1000) / 10
}
const ds = (sel) => page.evaluate((s) => ({ ...(document.querySelector(s)?.dataset ?? {}) }), sel)
const hud = async (qual) => (await page.locator(`[data-hud="${qual}"] .enh-pill__cost`).textContent().catch(() => '')) ?? ''
const numeros = (txt) => ({
  fps: Number(/(\d+)\s*fps/.exec(txt)?.[1] ?? 0),
  p95: Number((/·\s*([\d.,]+)\s*ms/.exec(txt)?.[1] ?? '0').replace(',', '.')),
})
const alternar = (qual) => page.locator(`input[data-enh-toggle="${qual}"]`).click({ force: true })

await page.goto(`${APP}/e2e/palco-imersivo.html?w=${SW}&h=${SH}`, { waitUntil: 'load' })
await page.waitForFunction(() => (document.querySelector('.rm-tile video')?.videoWidth ?? 0) > 0, null, { timeout: 30000 * FATOR })
medidas.renderer = await page.evaluate(() => {
  const gl = document.createElement('canvas').getContext('webgl2')
  const e = gl?.getExtension('WEBGL_debug_renderer_info')
  return gl ? String(gl.getParameter(e ? e.UNMASKED_RENDERER_WEBGL : gl.RENDERER)) : null
})
medidas.video = await page.evaluate(() => {
  const v = document.querySelector('.rm-tile video')
  return `${v.videoWidth}x${v.videoHeight} em ${v.clientWidth}x${v.clientHeight}`
})
console.log(`· GPU: ${medidas.renderer} · vídeo ${medidas.video}`)
ok(!!medidas.renderer, 'WebGL2 disponível')
await pausa(1500)
medidas.cpuSemEfeito = await cpuDurante(5000)

// ── Realce ────────────────────────────────────────────────────────────────────
console.log('· realce de nitidez')
ok(await page.locator('[data-enh="sugestao"]').isVisible(), 'a sugestão ao anfitrião aparece (sala de formação)')
await alternar('realce')
const desenhou = await page
  .waitForFunction(() => Number(document.querySelector('canvas[data-realce="on"]')?.dataset.frames ?? 0) > 20, null, { timeout: 15000 * FATOR })
  .then(() => true)
  .catch(() => false)
ok(desenhou, 'o canvas do realce desenha frames por cima do vídeo', JSON.stringify(await ds('canvas[data-realce]')))
await page.locator('[data-hud="realce"]').waitFor({ timeout: 5000 * FATOR }).catch(() => {})
const hudRealce = await hud('realce')
const r1 = numeros(hudRealce)
medidas.realce = { hud: hudRealce, ...r1 }
ok(r1.fps > 5, 'o filtro corre com fps medido', hudRealce)
const dims = await page.evaluate(() => {
  const c = document.querySelector('canvas[data-realce="on"]')
  const v = c?.previousElementSibling
  return c && v ? { canvas: `${c.width}x${c.height}`, prop: Math.abs(c.width / c.height - v.videoWidth / v.videoHeight), fit: getComputedStyle(c).objectFit } : null
})
ok(!!dims && dims.prop < 0.02 && dims.fit === 'cover', 'o canvas tem a proporção e o enquadramento do vídeo', JSON.stringify(dims))
const verOriginal = page.locator('[data-hud="realce"]').getByRole('button', { name: /ver original/i })
if (await verOriginal.isVisible().catch(() => false)) {
  await verOriginal.click()
  ok((await page.evaluate(() => getComputedStyle(document.querySelector('canvas[data-realce="on"]')).visibility)) === 'hidden', '«Ver original» esconde o filtro sem o parar')
  await verOriginal.click()
}
// A decisão do orçamento tem de bater com o custo medido.
medidas.realce.cpu = await cpuDurante(6000)
const realceAinda = await page.locator('canvas[data-realce="on"]').count()
const avisoRealce = (await page.locator('[data-enh="realce"] .dx-alert').textContent().catch(() => '')) ?? ''
const ultimo = numeros(await hud('realce'))
if (realceAinda) {
  medidas.realce.p95Estavel = ultimo.p95
  ok(ultimo.p95 <= 8, 'continua ligado e o p95 cabe no orçamento (8 ms)', `${ultimo.fps} fps · p95 ${ultimo.p95} ms`)
} else {
  medidas.realce.desligadoAuto = avisoRealce
  ok(/não aguentou/.test(avisoRealce), 'desligou-se sozinho e diz porquê (p95 acima do orçamento)', avisoRealce)
  await page.locator('[data-hud="aviso"]').getByRole('button').click().catch(() => {})
  // Volta a ligar à mão para os passos seguintes.
  await alternar('realce')
  await alternar('realce')
}

// ── Palco imersivo ────────────────────────────────────────────────────────────
console.log('· palco imersivo')
await alternar('imersivo')
ok(
  await page.waitForSelector('canvas[data-imersivo="on"]', { timeout: 15000 * FATOR }).then(() => true).catch(() => false),
  'o canvas do palco imersivo está por cima do vídeo',
)
ok(await page.waitForFunction(() => !document.querySelector('canvas[data-realce="on"]'), null, { timeout: 5000 }).then(() => true).catch(() => false), 'o realce sai do mesmo vídeo (não se empilham)')
const masc = await page
  .waitForFunction(() => Number(document.querySelector('canvas[data-imersivo="on"]')?.dataset.mascaras ?? 0) > 10, null, { timeout: 60000 * FATOR })
  .then(() => true)
  .catch(() => false)
const d1 = await ds('canvas[data-imersivo]')
ok(masc && d1.camadas === 'fundo,sombra,pessoa', 'o segmentador entrega máscaras e as três camadas são desenhadas', JSON.stringify(d1))

const area = await page.locator('.rm-stagearea').boundingBox()
await page.mouse.move(area.x + 4, area.y + area.height / 2, { steps: 6 })
await pausa(800)
const esq = Number((await ds('canvas[data-imersivo]')).bgX)
await page.mouse.move(area.x + area.width - 4, area.y + area.height / 2, { steps: 6 })
await pausa(800)
const dd = await ds('canvas[data-imersivo]')
ok(esq < -0.02 && Number(dd.bgX) > 0.02 && dd.fonte === 'pointer', 'paralaxe: o fundo segue o rato de um lado ao outro', `bgX ${esq} → ${dd.bgX} (${dd.fonte})`)

await page.evaluate(() => window.dispatchEvent(new DeviceOrientationEvent('deviceorientation', { alpha: 0, beta: 40, gamma: 0 })))
for (let i = 0; i < 8; i++) {
  await page.evaluate(() => window.dispatchEvent(new DeviceOrientationEvent('deviceorientation', { alpha: 0, beta: 40, gamma: -20 })))
  await pausa(100)
}
const dor = await ds('canvas[data-imersivo]')
ok(dor.fonte === 'orientation' && Number(dor.bgX) < -0.02, 'paralaxe: o giroscópio simulado ganha ao rato e desloca o fundo', `bgX ${dor.bgX} (${dor.fonte})`)

await page.locator('[data-banco="a-falar"]').check()
await pausa(900)
const fala = await ds('canvas[data-imersivo]')
await page.locator('[data-banco="a-falar"]').uncheck()
ok(Number(fala.fala) > 0.8 && Number(fala.escala) > 1.02 * 1.008, '4.ª dimensão: a falar, o envelope sobe e a pessoa aproxima-se', `envelope ${fala.fala}, escala ${fala.escala}`)

await page.locator('[data-hud="imersivo"]').waitFor({ timeout: 5000 * FATOR }).catch(() => {})
await pausa(2500)
const hudImersivo = await hud('imersivo')
medidas.imersivo = { hud: hudImersivo, ...numeros(hudImersivo), cpu: await cpuDurante(6000) }
const estado = await page.locator('[data-enh="imersivo"] [role="status"]').textContent().catch(() => '')
medidas.imersivo.estado = estado
medidas.imersivo.aindaLigado = (await page.locator('canvas[data-imersivo="on"]').count()) > 0
medidas.imersivo.aviso = (await page.locator('[data-enh="imersivo"] .dx-alert').textContent().catch(() => '')) ?? ''
ok(/\d+\s*fps/.test(hudImersivo) || /não aguentou/.test(medidas.imersivo.aviso), 'o custo do palco imersivo é medido (ou desligou-se por orçamento, dizendo porquê)', hudImersivo || medidas.imersivo.aviso)

// ── Movimento reduzido ────────────────────────────────────────────────────────
console.log('· prefers-reduced-motion')
if (!medidas.imersivo.aindaLigado) {
  await alternar('imersivo')
  await alternar('imersivo')
  await page.waitForSelector('canvas[data-imersivo="on"]', { timeout: 10000 * FATOR }).catch(() => {})
}
await page.emulateMedia({ reducedMotion: 'reduce' })
ok(await page.waitForFunction(() => !document.querySelector('canvas[data-imersivo="on"]'), null, { timeout: 5000 }).then(() => true).catch(() => false), 'com movimento reduzido o palco imersivo desliga-se')
ok(/movimento reduzido/.test((await page.locator('[data-enh="imersivo"] .dx-alert').textContent().catch(() => '')) ?? ''), 'e diz porquê')
await page.emulateMedia({ reducedMotion: 'no-preference' })
ok(await page.waitForSelector('canvas[data-imersivo="on"]', { timeout: 10000 * FATOR }).then(() => true).catch(() => false), 'sem a preferência volta a ligar sozinho (o pedido da pessoa mantém-se)')

// ── Teclado ───────────────────────────────────────────────────────────────────
console.log('· teclado e 375 px')
await page.locator('input[data-enh-toggle="imersivo"]').focus()
await page.keyboard.press('Space')
ok(await page.waitForFunction(() => !document.querySelector('canvas[data-imersivo="on"]'), null, { timeout: 5000 }).then(() => true).catch(() => false), 'desliga-se pelo teclado (Espaço no interruptor focado)')
const foco = await page.evaluate(() => getComputedStyle(document.activeElement).outlineStyle + '/' + getComputedStyle(document.activeElement.closest('label') ?? document.activeElement).outlineStyle)
medidas.foco = foco

// ── 375 px ────────────────────────────────────────────────────────────────────
await page.setViewportSize({ width: 375, height: 812 })
if (!(await page.locator('input[data-enh-toggle="realce"]').isChecked())) await alternar('realce')
await page.locator('[data-hud="realce"]').waitFor({ timeout: 10000 * FATOR }).catch(() => {})
await pausa(1500)
const larg = await page.evaluate(() => ({ doc: document.documentElement.scrollWidth, hud: document.querySelector('.enh-hud')?.getBoundingClientRect().width ?? 0, suggest: document.querySelector('.enh-suggest')?.getBoundingClientRect().right ?? 0 }))
ok(larg.doc <= 375 && larg.hud > 0 && larg.hud <= 375 && larg.suggest <= 375, 'a 375 px o HUD (visível) e a sugestão cabem no ecrã', JSON.stringify(larg))
if (OUT) await page.screenshot({ path: `${OUT}/palco-375.png` })
await page.setViewportSize({ width: 1440, height: 900 })
if (OUT) await page.screenshot({ path: `${OUT}/palco-1440.png` })

ok(erros.filter((e) => !/favicon|Failed to load resource/.test(e)).length === 0, 'sem erros na consola', JSON.stringify(erros.slice(0, 5)))
medidas.cpuSemEfeito = `${medidas.cpuSemEfeito} %`
console.log('\n· medições:', JSON.stringify(medidas, null, 2))
await browser.close()
console.log(`\n=== ${falhas === 0 ? 'TODAS PASSARAM' : `${falhas} FALHARAM`} ===`)
process.exit(falhas ? 1 : 0)
