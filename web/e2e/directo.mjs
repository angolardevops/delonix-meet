#!/usr/bin/env node
// O DIRECTO, ponta a ponta: o painel, a recusa do servidor a chegar à interface
// tal como foi escrita, e o WebSocket a ser aceite quando as regras passam.
//
// O que NÃO se prova aqui: que a media chega ao YouTube. Isso precisa de um
// destino RTMP a sério e é a linha 2 do portão do ADR-0003 — o que se verifica
// é que o servidor ACEITA a ligação e arranca o processo, o que é o limite do
// que se pode afirmar sem uma plataforma externa.
//
// Uso:  BASE=http://127.0.0.1:5180 node e2e/directo.mjs
import { chromium } from '@playwright/test'
import { criarConta, entrar } from './sessao.mjs'

const BASE = process.env.BASE ?? process.env.APP ?? 'http://127.0.0.1:5180'
const API = process.env.API ?? BASE
let falhas = 0
const ok = (n, c, d = '') => { console.log(`${c ? '  ok  ' : ' FALHA'}  ${n}${d ? `  — ${d}` : ''}`); if (!c) falhas++ }

const conta = await criarConta(API, 'dir')
const browser = await chromium.launch({
  args: ['--use-fake-device-for-media-stream', '--use-fake-ui-for-media-stream',
         '--auto-select-desktop-capture-source=Entire screen'],
})
const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, permissions: ['camera', 'microphone'] })
const page = await ctx.newPage()
const erros = []
page.on('pageerror', (e) => erros.push(e.message.slice(0, 140)))

await entrar(page, BASE, conta)
await page.locator('.nav-item', { hasText: /Estúdio|Studio/ }).first().click()
await page.waitForSelector('.studio-canvas', { timeout: 20000 })

console.log('\no painel')
const painel = page.locator('.studio-directo')
ok('o painel do directo aparece', (await painel.count()) > 0)
ok('o browser sabe codificar H.264',
   await page.evaluate(() => MediaRecorder.isTypeSupported('video/webm;codecs=h264,opus')))

const chave = painel.locator('input[type=password]')
ok('a chave de emissão é um campo de password',
   (await chave.count()) > 0,
   'uma partilha de ecrã a configurar o directo não pode mostrá-la')

const botao = painel.locator('button').filter({ hasText: /Ir para o ar|Go live/ }).first()
ok('o botão está travado sem chave', await botao.isDisabled())

console.log('\nas regras do servidor chegam à interface')
// Liga a câmara para haver o que emitir.
await page.locator('.studio-grupo', { hasText: /A tua imagem|Your picture/ }).locator('button').first().click()
await page.waitForTimeout(1200)
await chave.fill('chave-de-teste-123')
ok('e destrava com chave e imagem', !(await botao.isDisabled()))

// Destino inalcançável de propósito: o que se mede é que o SERVIDOR aceitou a
// ligação (as regras passaram) e não que o YouTube recebeu.
await painel.locator('input').first().fill('rtmp://127.0.0.1:1/live')
await botao.click()
const foiAoAr = await page.waitForSelector('.studio-no-ar', { timeout: 25000 }).then(() => true).catch(() => false)
const motivo = foiAoAr ? '' : ((await painel.locator('.error').textContent().catch(() => '')) ?? '')
ok('o servidor aceita a emissão e a interface mostra NO AR', foiAoAr, motivo || '')

if (foiAoAr) {
  await page.waitForTimeout(2500)
  const contador = await painel.locator('.mono').textContent()
  ok('o contador anda (bytes enviados)', /\d+\.\d MB/.test(contador ?? ''), contador ?? '')
  await painel.locator('button').filter({ hasText: /Terminar|End/ }).first().click()
  await page.waitForSelector('.studio-no-ar', { state: 'detached', timeout: 10000 })
    .then(() => ok('terminar o directo tira o NO AR', true))
    .catch(() => ok('terminar o directo tira o NO AR', false))
}

ok('sem erros de página', erros.length === 0, erros[0] ?? 'nenhum')
await page.screenshot({ path: '/tmp/directo.png' })
await browser.close()
console.log(falhas === 0 ? '\nTUDO VERDE\n' : `\n${falhas} FALHA(S)\n`)
process.exit(falhas === 0 ? 0 : 1)
