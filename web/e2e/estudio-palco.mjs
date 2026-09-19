#!/usr/bin/env node
// O PALCO do Estúdio num Chromium a sério, com câmara falsa e DOIS browsers:
// o estúdio e um convidado.
//
// O que se prova por PIXÉIS do canvas (a única leitura que não se deixa
// enganar): o layout muda a composição, o cartão de intervalo e o quadro
// enchem o palco, as sobreposições queimam-se na imagem, e o convidado posto
// no palco aparece nela. O que se prova pelo DOM: a fila de espera, a
// admissão, o vídeo recebido por SFU, o chat interno, a pré-escuta só local,
// e o banco de cenas a sobreviver a um recarregamento (IndexedDB).
//
// O que NÃO se prova aqui: as legendas (precisam do modelo Whisper em
// `public/models`, que só a imagem de produção traz) e o som da mistura
// (os faders mexem em GainNode; ouvir o resultado precisa de um ouvido).
//
// Uso:  BASE=http://127.0.0.1:5174 node e2e/estudio-palco.mjs
import { chromium } from '@playwright/test'
import { criarConta, entrar } from './sessao.mjs'

const BASE = process.env.BASE ?? process.env.APP ?? 'http://127.0.0.1:5174'
const API = process.env.API ?? BASE
const FATOR = Number(process.env.E2E_TIMEOUT_FACTOR) || 1
let falhas = 0
const ok = (n, c, d = '') => {
  console.log(`${c ? '  ok  ' : ' FALHA'}  ${n}${d ? `  — ${d}` : ''}`)
  if (!c) falhas++
}

const args = ['--use-fake-device-for-media-stream', '--use-fake-ui-for-media-stream', '--autoplay-policy=no-user-gesture-required']
const [contaEstudio, contaConvidado] = [await criarConta(API, 'pal'), await criarConta(API, 'cnv')]
const bEstudio = await chromium.launch({ args })
const bConvidado = await chromium.launch({ args })
const ctx = await bEstudio.newContext({ viewport: { width: 1440, height: 900 }, permissions: ['camera', 'microphone'] })
const page = await ctx.newPage()
const erros = []
page.on('pageerror', (e) => erros.push(e.message.slice(0, 140)))

await entrar(page, BASE, contaEstudio)
await page.locator('.nav-item', { hasText: /Estúdio|Studio/ }).first().click()
await page.waitForSelector('[data-studio="canvas"]', { timeout: 20000 * FATOR })
const canvas = page.locator('[data-studio="canvas"]')

/** Brilho médio de um rectângulo do canvas, em fracções. */
const brilho = (fx, fy, fw, fh) =>
  canvas.evaluate(
    (c, r) => {
      const g = c.getContext('2d')
      const d = g.getImageData(Math.round(r.fx * c.width), Math.round(r.fy * c.height), Math.round(r.fw * c.width), Math.round(r.fh * c.height)).data
      let s = 0
      for (let i = 0; i < d.length; i += 4) s += (d[i] + d[i + 1] + d[i + 2]) / 3
      return Math.round(s / (d.length / 4))
    },
    { fx, fy, fw, fh },
  )

console.log('\nlayouts')
await page.locator('[data-studio="camara"]').click()
await page.waitForTimeout(1500)
const soloCentro = await brilho(0.35, 0.35, 0.3, 0.3)
await page.locator('[data-studio-layout="grelha"]').click()
await page.waitForTimeout(800)
const grelhaCentro = await brilho(0.35, 0.35, 0.3, 0.3)
ok('fora do «solo», a câmara enche o palco em vez da bolha', grelhaCentro > soloCentro + 10, `${soloCentro} → ${grelhaCentro}`)
await page.locator('[data-studio-layout="solo"]').click()

console.log('\nbanco de cenas')
const cenas = page.locator('[data-studio="cena"]')
await page.waitForFunction(() => document.querySelectorAll('[data-studio="cena"]').length >= 4, null, { timeout: 10000 })
ok('há cenas de partida', (await cenas.count()) >= 4, String(await cenas.count()))
await cenas.nth(2).click() // intervalo · marca
await page.waitForTimeout(600)
const marca = await canvas.evaluate((c) => {
  const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data
  let r = 0
  for (let i = 0; i < d.length; i += 16) if (d[i] > 150 && d[i + 1] < 80) r++
  return r
})
ok('a cena de intervalo desenha a marca', marca > 200, `${marca} pixéis vermelhos`)
await cenas.nth(3).click() // quadro branco
await page.waitForTimeout(600)
ok('a cena de quadro enche o palco de branco', (await brilho(0.1, 0.1, 0.2, 0.2)) > 230)
const q = await page.locator('[data-studio="quadro"]').boundingBox()
const antesTraco = await brilho(0.3, 0.3, 0.3, 0.2)
await page.mouse.move(q.x + q.width * 0.3, q.y + q.height * 0.35)
await page.mouse.down()
for (let i = 1; i <= 20; i++) await page.mouse.move(q.x + q.width * (0.3 + i * 0.015), q.y + q.height * (0.35 + (i % 2) * 0.05))
await page.mouse.up()
await page.waitForTimeout(400)
ok('escrever no quadro deixa traço na imagem', (await brilho(0.3, 0.3, 0.3, 0.2)) < antesTraco - 3, `${antesTraco} → ${await brilho(0.3, 0.3, 0.3, 0.2)}`)

await page.locator('[data-studio="cena-nova"]').click()
await page.fill('[data-studio="cena-nome"]', 'Cena do e2e')
await page.locator('[data-studio="cena-guardar"]').click()
await page.waitForFunction(() => [...document.querySelectorAll('[data-studio="cena"]')].some((b) => b.textContent.includes('Cena do e2e')), null, { timeout: 10000 })
await page.reload()
await page.waitForSelector('[data-studio="canvas"]', { timeout: 20000 * FATOR })
const persistiu = await page
  .waitForFunction(() => [...document.querySelectorAll('[data-studio="cena"]')].some((b) => b.textContent.includes('Cena do e2e')), null, { timeout: 10000 })
  .then(() => true)
  .catch(() => false)
ok('uma cena nova sobrevive ao recarregamento (IndexedDB)', persistiu)

console.log('\nsobreposições')
await cenas.first().click()
await page.locator('[data-studio="camara"]').click()
await page.waitForTimeout(1200)
const cantoAntes = await brilho(0.01, 0.01, 0.15, 0.08)
await page.locator('[data-studio-sobreposicao="logotipo"]').click()
await page.waitForTimeout(500)
const cantoDepois = await brilho(0.01, 0.01, 0.15, 0.08)
ok('o logótipo queima-se no canto', Math.abs(cantoDepois - cantoAntes) > 3, `${cantoAntes} → ${cantoDepois}`)

console.log('\nqualidade')
await page.selectOption('[data-studio="qualidade"]', '2160p50')
await page.waitForTimeout(300)
const dim = await canvas.evaluate((c) => `${c.width}×${c.height}`)
ok('2160p muda o canvas de gravação', dim === '3840×2160', dim)
await page.selectOption('[data-studio="qualidade"]', '1080p30')

console.log('\nsala de convidados')
await page.locator('[data-studio="sala-abrir"]').click()
await page.locator('[data-studio="sala-criar"]').click()
await page.waitForSelector('.st-sala__code', { timeout: 20000 * FATOR })
const codigo = ((await page.locator('.st-sala__code').textContent()) ?? '').trim()
ok('o estúdio cria a sala dos convidados', /^[a-z]+(-[a-z]+)+$/.test(codigo), codigo)

const ctxC = await bConvidado.newContext({ viewport: { width: 1280, height: 800 }, permissions: ['camera', 'microphone'] })
const conv = await ctxC.newPage()
conv.on('pageerror', (e) => erros.push(`convidado: ${e.message.slice(0, 120)}`))
await entrar(conv, BASE, contaConvidado)
await conv.goto(`${BASE}/#/r/${codigo}`)
await conv.getByRole('button', { name: /entrar na sess|join/i }).click({ timeout: 30000 * FATOR })

await page.waitForSelector('[data-studio="sala-admitir"]', { timeout: 30000 * FATOR })
ok('o convidado chega à fila de espera', true)
await page.locator('[data-studio="sala-admitir"]').first().click()
const video = await page
  .waitForFunction(() => document.querySelector('[data-studio="convidado"] video')?.videoWidth > 0, null, { timeout: 30000 * FATOR })
  .then(() => true)
  .catch(() => false)
ok('admitido, o vídeo do convidado chega por SFU', video)

await page.locator('[data-studio-layout="lado-a-lado"]').click()
await page.locator('[data-studio="camara"]').click() // só o convidado no palco
await page.waitForTimeout(800)
const semConvidado = await brilho(0.1, 0.1, 0.8, 0.8)
await page.locator('[data-studio="convidado"]').first().click()
await page.locator('[data-studio="por-no-palco"]').click()
await page.waitForTimeout(2000)
const comConvidado = await brilho(0.1, 0.1, 0.8, 0.8)
ok('«Pôr no palco» desenha o convidado na imagem', comConvidado > semConvidado + 10, `${semConvidado} → ${comConvidado}`)

await page.locator('[data-studio="por-no-palco"]').click()
await page.locator('[data-studio="pre-escuta"]').click()
const mudos = await page.evaluate(() => [...document.querySelectorAll('[data-studio="sala"] audio')].map((a) => a.muted))
ok('a pré-escuta tira o mudo SÓ localmente', mudos.length === 1 && mudos[0] === false, JSON.stringify(mudos))

await conv.getByRole('button', { name: /^chat/i }).first().click({ timeout: 10000 }).catch(() => {})
await conv.locator('.rm-chat__input textarea').fill('mensagem-do-convidado', { timeout: 10000 }).catch(() => {})
await conv.locator('.rm-chat__input textarea').press('Enter').catch(() => {})
await page.getByRole('tab', { name: /Chat/ }).click()
const chat = await page
  .waitForFunction(() => document.querySelector('[data-studio="sala"] .rm-chat__list')?.textContent?.includes('mensagem-do-convidado'), null, { timeout: 15000 })
  .then(() => true)
  .catch(() => false)
ok('o chat interno recebe o convidado', chat)

ok('sem erros de página', erros.length === 0, erros.slice(0, 2).join(' | ') || 'nenhum')
await page.screenshot({ path: '/tmp/estudio-palco.png' })
await bEstudio.close()
await bConvidado.close()
console.log(falhas === 0 ? '\nTUDO VERDE\n' : `\n${falhas} FALHA(S)\n`)
process.exit(falhas === 0 ? 0 : 1)
