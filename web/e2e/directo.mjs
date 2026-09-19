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
import { texto } from './estudio-textos.mjs'

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
await page.waitForSelector('[data-studio="canvas"]', { timeout: 20000 })

console.log('\no painel')
const painel = page.locator('[data-studio="directo"]')
ok('o painel do directo aparece', (await painel.count()) > 0 && texto('directo.titulo').test((await painel.textContent()) ?? ''))
ok('o browser sabe codificar H.264',
   await page.evaluate(() => MediaRecorder.isTypeSupported('video/webm;codecs=h264,opus')))

// Um cartão por destino, com o estado que o browser conhece; os campos
// editam-se num diálogo aberto a partir do cartão.
const cartao = painel.locator('[data-studio="destino"]').first()
ok('há um cartão por destino, sem chave', (await cartao.getAttribute('data-estado')) === 'sem-chave')
await cartao.locator('[data-studio="destino-editar"]').click()
const chave = page.locator('[data-studio="destino-form"] [data-studio="destino-chave"][type=password]')
ok('a chave de emissão é um campo de password',
   (await chave.count()) > 0,
   'uma partilha de ecrã a configurar o directo não pode mostrá-la')
await page.keyboard.press('Escape')

const botao = painel.locator('[data-studio="ir-para-o-ar"]')
ok('o botão diz «ir para o ar»', texto('directo.irParaOAr').test((await botao.textContent()) ?? ''))
ok('o botão está travado sem chave', await botao.isDisabled())

console.log('\nas regras do servidor chegam à interface')
// Liga a câmara para haver o que emitir.
await page.locator('[data-studio-grupo="imagem"] [data-studio="camara"]').click()
await page.waitForTimeout(1200)
await cartao.locator('[data-studio="destino-editar"]').click()
await chave.fill('chave-de-teste-123')
// Destino inalcançável de propósito: o que se mede é que o SERVIDOR aceitou a
// ligação (as regras passaram) e não que o YouTube recebeu.
await page.locator('[data-studio="destino-form"] [data-studio="destino-url"]').fill('rtmp://127.0.0.1:1/live')
await page.locator('[data-studio="destino-guardar"]').click()
ok('o cartão passa a «pronto»', (await cartao.getAttribute('data-estado')) === 'pronto')
ok('e destrava com chave e imagem', !(await botao.isDisabled()))
await botao.click()
// Espera por um dos DOIS desfechos: no ar, ou recusado com razão.
const desfecho = await page
  .waitForSelector('[data-studio="no-ar"], [data-studio="directo-erro"]', { state: 'attached', timeout: 25000 })
  .then((h) => h.getAttribute('data-studio'))
  .catch(() => null)
const foiAoAr = desfecho === 'no-ar'
const motivo = foiAoAr ? '' : ((await painel.locator('[data-studio="directo-erro"]').textContent({ timeout: 2000 }).catch(() => '')) ?? '')

// DOIS AMBIENTES, e a asserção tem de ser honesta nos dois.
//
// O runner do CI NÃO tem ffmpeg — o repo já conta com isso noutro teste
// (`gravacao-falhada.mjs` verifica precisamente o caminho de falha). Exigir
// «NO AR» aqui daria vermelho por uma razão que não é do código.
//
// O que se exige em ambos: ou a emissão arranca, ou é recusada com uma razão
// NOMEADA. O que nunca é aceitável é falhar sem dizer porquê — foi essa a
// diferença entre o erro opaco que este teste apanhou e a mensagem de agora.
if (foiAoAr) {
  ok('o servidor aceita a emissão e a interface mostra NO AR', true)
} else {
  const nomeada = /ffmpeg/i.test(motivo) || /ponta-a-ponta|H\.264|destino|emissões/i.test(motivo)
  ok('sem ffmpeg, o servidor recusa com uma razão NOMEADA', nomeada, motivo || '(sem razão nenhuma)')
  ok('e a razão diz o que fazer', /administra|Desliga|chave|máximo/i.test(motivo), motivo || '')
}

if (foiAoAr) {
  await page.waitForTimeout(2500)
  const meta = await page.locator('[data-studio="topo-meta"]').textContent()
  ok('o débito do codificador aparece no topo', /\d[\d\s\u00a0.,]* kbps/.test(meta ?? ''), meta ?? '')
  ok('o cartão diz NO AR', (await cartao.getAttribute('data-estado')) === 'no-ar')
  const terminar = painel.locator('[data-studio="sair-do-ar"]')
  ok('o botão de terminar diz o que faz', texto('directo.parar').test((await terminar.textContent()) ?? ''))
  await terminar.click()
  await page.waitForSelector('[data-studio="no-ar"]', { state: 'detached', timeout: 10000 })
    .then(() => ok('terminar o directo tira o NO AR', true))
    .catch(() => ok('terminar o directo tira o NO AR', false))
}

ok('sem erros de página', erros.length === 0, erros[0] ?? 'nenhum')
await page.screenshot({ path: '/tmp/directo.png' })
await browser.close()
console.log(falhas === 0 ? '\nTUDO VERDE\n' : `\n${falhas} FALHA(S)\n`)
process.exit(falhas === 0 ? 0 : 1)
