#!/usr/bin/env node
// Editor do Estúdio num Chromium a sério, com câmara e microfone FALSOS.
//
// Prova o caminho inteiro do projecto não destrutivo: gravar → o projecto nasce
// com as faixas como fontes → aparar pela entrada/saída → desfazer/refazer →
// cor → gravação automática sobrevive a recarregar a página → exportar no
// browser e medir a DURAÇÃO do ficheiro produzido (vídeo e podcast).
//
// Uso:  BASE=http://127.0.0.1:5514 API=http://127.0.0.1:8214 node e2e/editor.mjs
//       SHOTS=/caminho  guarda capturas a 1440×900 e 375×812.
import { chromium } from '@playwright/test'
import { criarConta, entrar } from './sessao.mjs'

const FATOR = Number(process.env.E2E_TIMEOUT_FACTOR) || 1
const BASE = process.env.BASE ?? 'http://127.0.0.1:5173'
const API = process.env.API ?? BASE
const SHOTS = process.env.SHOTS ?? ''
const conta = process.env.DX_USER ? { email: process.env.DX_USER, password: process.env.DX_PASS } : await criarConta(API, 'edt')

let falhas = 0
const ok = (nome, cond, det = '') => {
  console.log(`${cond ? '  ok  ' : ' FALHA'}  ${nome}${det ? `  — ${det}` : ''}`)
  if (!cond) falhas++
}

const browser = await chromium.launch({
  args: ['--use-fake-device-for-media-stream', '--use-fake-ui-for-media-stream', '--autoplay-policy=no-user-gesture-required'],
})
const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, permissions: ['camera', 'microphone'], acceptDownloads: true })
const page = await ctx.newPage()
const erros = []
page.on('pageerror', (e) => erros.push(e.message.slice(0, 160)))

const duracaoMostrada = async () => {
  const t = (await page.locator('[data-studio="duracao"]').textContent()) ?? ''
  const m = t.match(/(\d+):(\d{2})/)
  return m ? Number(m[1]) * 60 + Number(m[2]) : NaN
}

await entrar(page, BASE, conta)

// ---- gravar um clip
console.log('\ngravação')
await page.goto(`${BASE}/#/studio`)
await page.waitForSelector('[data-studio="canvas"]', { timeout: 20000 })
await page.locator('[data-studio-grupo="imagem"] [data-studio="camara"]').click()
await page.waitForTimeout(1500)
await page.locator('[data-studio="acoes"] [data-studio="gravar"]').click()
await page.waitForSelector('[data-studio="tempo"]', { timeout: 10000 })
await page.waitForTimeout(6500)
await page.locator('[data-studio="acoes"] [data-studio="parar"]').click()

await page.waitForSelector('[data-studio="bin"] .ed-src', { timeout: 30000 })
ok('parar abre o editor em #/studio?vista=edicao', page.url().includes('vista=edicao'), page.url())
const fontes = await page.locator('[data-studio="bin"] [data-fonte]').evaluateAll((els) => els.map((e) => e.getAttribute('data-fonte')))
ok('as faixas da gravação entram como fontes separadas', ['completo', 'video', 'audio', 'camara'].every((f) => fontes.includes(f)), fontes.join(','))
await page.waitForFunction(() => document.querySelectorAll('[data-faixa="V1"] .ed-clip').length === 1 && document.querySelectorAll('[data-faixa="A1"] .ed-clip').length === 1, null, { timeout: 15000 })
ok('V1 e A1 têm um clipe cada (imagem e voz ligadas)', true)
const dur0 = await duracaoMostrada()
ok('a linha de tempo tem a duração da gravação', dur0 >= 5 && dur0 <= 9, `${dur0}s`)
const temOnda = await page.waitForSelector('[data-faixa="A1"] .ed-clip__wave', { timeout: 20000 }).then(() => true).catch(() => false)
ok('a faixa A1 desenha a onda sonora', temOnda)
if (SHOTS) await page.screenshot({ path: `${SHOTS}/editor-1440.png` })

// ---- aparar pela entrada/saída
console.log('\naparar, desfazer, refazer')
await page.locator('[data-faixa="V1"] .ed-clip').first().click({ position: { x: 40, y: 10 } })
await page.waitForSelector('[data-studio="corte"]')
await page.locator('[data-studio="corte-de"]').fill('00:00:01')
await page.locator('[data-studio="corte-de"]').press('Enter')
await page.locator('[data-studio="corte-ate"]').fill('00:00:04')
await page.locator('[data-studio="corte-ate"]').press('Enter')
await page.waitForTimeout(300)
const dur1 = await duracaoMostrada()
ok('aparar para 1–4 s deixa a linha de tempo com 3 s', dur1 === 3, `${dur1}s`)
const a1 = await page.locator('[data-faixa="A1"] .ed-clip').count()
ok('o áudio ligado foi aparado com a imagem', a1 === 1)
await page.locator('[data-studio="desfazer"]').click()
await page.locator('[data-studio="desfazer"]').click()
await page.waitForTimeout(200)
ok('desfazer duas vezes volta à duração original', (await duracaoMostrada()) === dur0, `${await duracaoMostrada()}s`)
await page.locator('[data-studio="refazer"]').click()
await page.locator('[data-studio="refazer"]').click()
await page.waitForTimeout(200)
ok('refazer volta aos 3 s', (await duracaoMostrada()) === 3)

// ---- lâmina e cor
await page.locator('[data-ferramenta="lamina"]').click()
const caixa = await page.locator('[data-faixa="V1"] .ed-clip').first().boundingBox()
await page.mouse.click(caixa.x + caixa.width / 2, caixa.y + caixa.height / 2)
await page.waitForTimeout(200)
ok('a lâmina divide o clipe em dois (V1 e A1)', (await page.locator('[data-faixa="V1"] .ed-clip').count()) === 2 && (await page.locator('[data-faixa="A1"] .ed-clip').count()) === 2)
await page.locator('[data-ferramenta="seleccionar"]').click()
await page.locator('[data-faixa="V1"] .ed-clip').first().click({ position: { x: 20, y: 10 } })
await page.locator('#ed-cor-contraste').fill('40')
await page.waitForTimeout(300)
const filtro = await page.locator('[data-studio="preview"]').evaluate((v) => v.style.filter)
ok('a correcção de cor chega à pré-visualização', /contrast\(1\.4/.test(filtro), filtro)

// ---- gravação automática e recarregar
console.log('\npersistência')
await page.waitForFunction(() => /\d/.test(document.querySelector('[data-studio="guardado-auto"]')?.textContent ?? ''), null, { timeout: 10000 })
await page.waitForTimeout(1200)
await page.reload()
await page.waitForSelector('[data-faixa="V1"] .ed-clip', { timeout: 30000 })
ok('recarregar reabre o projecto com as edições (IndexedDB)', (await duracaoMostrada()) === 3 && (await page.locator('[data-faixa="V1"] .ed-clip').count()) === 2)

// ---- legendas
console.log('\nlegendas')
await page.locator('[data-studio="ir-exportar"]').waitFor()
await page.goto(`${BASE}/#/studio?vista=legendas`)
await page.waitForSelector('[data-studio="transcrever"]', { timeout: 20000 })
const semModelo = await page.locator('[data-studio="transcrever"]').isDisabled()
console.log(`  --    modelo Whisper ${semModelo ? 'NÃO instalado neste servidor — transcrição não verificada' : 'disponível'}`)
if (SHOTS) await page.screenshot({ path: `${SHOTS}/legendas-1440.png` })

// ---- exportar
console.log('\nexportações')
await page.goto(`${BASE}/#/studio?vista=exportacoes`)
await page.waitForSelector('[data-studio="exportar"]', { timeout: 20000 })
if (SHOTS) await page.screenshot({ path: `${SHOTS}/exportacoes-1440.png` })
const suporta = await page.evaluate(() => typeof VideoEncoder === 'function' && typeof AudioEncoder === 'function')
ok('o browser tem WebCodecs', suporta)

async function exportar(preset, esperado, seletor) {
  await page.locator(`.ed-preset:has(input[value="${preset}"])`).click()
  const [download] = await Promise.all([
    page.waitForEvent('download', { timeout: 240000 * FATOR }).catch(() => null),
    page.locator('[data-studio="exportar"]').click(),
  ])
  const dur = await page
    .waitForFunction(
      (sel) => {
        const els = [...document.querySelectorAll(sel)]
        const v = els[els.length - 1]
        return v && Number.isFinite(v.duration) && v.duration > 0 ? v.duration : null
      },
      seletor,
      { timeout: 240000 * FATOR },
    )
    .then((h) => h.jsonValue())
    .catch(() => null)
  const falhou = await page.locator('.ed-job[data-estado="falhou"]').count()
  ok(`exportar ${preset} produz um ficheiro com a duração do projecto (±0,6 s)`, dur !== null && Math.abs(dur - esperado) < 0.6, dur ? `${dur.toFixed(2)}s` : falhou ? await page.locator('.ed-job[data-estado="falhou"] .ed-tone--erro').first().textContent() : 'sem resultado')
  ok(`e o browser recebe o ficheiro (${preset})`, !!download, download ? download.suggestedFilename() : '')
}
if (suporta) {
  const t0 = Date.now()
  await exportar('web720', 3, 'video[data-studio="render"]')
  console.log(`  --    exportação 720p de 3 s levou ${((Date.now() - t0) / 1000).toFixed(1)} s`)
  await exportar('podcast', 3, 'audio[data-studio="render"]')
  const hist = await page.locator('.ed-exp__table tbody tr').count()
  ok('o histórico deste browser regista as exportações', hist >= 2, `${hist} linhas`)
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/exportacoes-feitas-1440.png` })
}

// ---- ecrã estreito
if (SHOTS) {
  await page.setViewportSize({ width: 375, height: 812 })
  for (const v of ['edicao', 'legendas', 'exportacoes']) {
    await page.goto(`${BASE}/#/studio?vista=${v}`)
    await page.waitForTimeout(2500)
    const larg = await page.evaluate(() => document.documentElement.scrollWidth)
    ok(`a 375 px a vista ${v} não transborda na horizontal`, larg <= 380, `${larg}px`)
    await page.screenshot({ path: `${SHOTS}/${v}-375.png`, fullPage: true })
  }
}

ok('sem erros de página', erros.length === 0, erros.slice(0, 3).join(' | ') || 'nenhum')
await browser.close()
console.log(falhas === 0 ? '\nTUDO VERDE\n' : `\n${falhas} FALHA(S)\n`)
process.exit(falhas === 0 ? 0 : 1)
