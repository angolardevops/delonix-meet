// Prova Playwright da biblioteca e do leitor contra a API real (sessão de
// comparar.mjs em .sessao.json), a 1440×900 e a 390×844.
//
//   node fidelidade/gravacoes.mjs --app http://127.0.0.1:5602
//
// Prova: dados reais na tabela, «—» onde o servidor não dá dado, painel e
// leitor a carregar o ficheiro, duração e resolução medidas, saltar por cena,
// velocidade, sem scroll horizontal. NÃO prova capítulos/legendas/comentários
// do servidor (o contrato ainda não está na main).
import { chromium } from '@playwright/test'
import { existsSync } from 'node:fs'
const DIR = new URL('./', import.meta.url).pathname
const args = process.argv.slice(2)
const APP = args.includes('--app') ? args[args.indexOf('--app') + 1] : 'http://127.0.0.1:5602'
const STATE = `${DIR}.sessao.json`
let falhas = 0
const ok = (n, c, d = '') => {
  console.log(`${c ? '  ok  ' : ' FALHA'}  ${n}${d ? '  — ' + d : ''}`)
  if (!c) falhas++
}

const b = await chromium.launch()
for (const [w, h] of [
  [1440, 900],
  [390, 844],
]) {
  console.log(`\n== ${w}×${h}`)
  const ctx = await b.newContext({ viewport: { width: w, height: h }, storageState: existsSync(STATE) ? STATE : undefined })
  const p = await ctx.newPage()
  p.on('pageerror', (e) => ok('sem erros de página', false, e.message))
  await p.goto(`${APP}/#/recordings`)
  await p.waitForSelector('.rec-row', { timeout: 30000 })
  const linhas = await p.locator('.rec-row').count()
  ok('biblioteca com gravações reais', linhas > 0, `${linhas} linhas`)
  const nome = await p.locator('.rec-row__name').first().innerText()
  ok('nome vem do servidor', nome.length > 0, nome)
  if (w > 700) {
    const cab = await p.locator('.rec-table thead th').allInnerTexts()
    ok('seis colunas do template', cab.length === 6, cab.join(' | '))
    ok('colunas sem dado mostram «—», não números', (await p.locator('.rec-row__dur .rec-none').count()) === linhas)
  }
  const chips = await p.locator('.rec-chip').allInnerTexts()
  ok('nenhum chip de categoria sem dado', !chips.some((c) => /Videoaulas|Emissões|4K/.test(c)), chips.join(' | '))
  const largura = await p.evaluate(() => document.documentElement.scrollWidth - innerWidth)
  ok('sem scroll horizontal na biblioteca', largura <= 0, `${largura}px`)

  await p.locator('.rec-row__open', { hasText: /sessão 3/ }).first().click()
  await p.waitForSelector('.rec-panel video', { timeout: 30000 })
  ok('painel abre o leitor com o ficheiro', true)
  if (w <= 700) ok('em ecrã estreito o painel abre por cima', (await p.locator('.rec-panel.is-open').count()) === 1)
  await p.waitForFunction(() => !/—/.test(document.querySelector('.rec-player__times span:last-child')?.textContent ?? '—'), null, { timeout: 20000 })
  ok('duração medida no ficheiro', true, await p.locator('.rec-player__times span:last-child').innerText())
  await p.waitForTimeout(800)
  await p.screenshot({ path: `${DIR}app/gravacoes-painel-${w}.png` })

  await p.getByRole('button', { name: /página inteira/i }).first().click()
  await p.waitForSelector('.pl-video video', { timeout: 30000 })
  await p.waitForFunction(() => /\d{2}:\d{2} \/ \d{2}:\d{2}/.test(document.querySelector('.pl-controls__time')?.textContent ?? ''), null, { timeout: 20000 })
  ok('leitor em página inteira com tempo medido', true, await p.locator('.pl-controls__time').innerText())
  const res = await p.locator('.pl-badges .rec-badge').innerText().catch(() => '')
  ok('resolução medida no ficheiro', /\d+p/.test(res), res)
  await p.getByRole('button', { name: /pausar/i }).click()
  await p.locator('.rec-chapter').nth(2).click()
  await p.waitForTimeout(800)
  const t = await p.evaluate(() => document.querySelector('video')?.currentTime ?? 0)
  ok('clicar numa cena salta o vídeo', t > 50, `${t.toFixed(1)} s`)
  await p.locator('.pl-controls__rate').click()
  const rate = await p.evaluate(() => document.querySelector('video')?.playbackRate)
  ok('velocidade muda', rate === 1.25, String(rate))
  await p.evaluate(() => document.querySelector('video')?.pause())
  const largura2 = await p.evaluate(() => document.documentElement.scrollWidth - innerWidth)
  ok('sem scroll horizontal no leitor', largura2 <= 0, `${largura2}px`)
  await p.waitForTimeout(1500)
  await p.screenshot({ path: `${DIR}app/gravacoes-leitor-${w}.png`, fullPage: w <= 700 })
  await ctx.close()
}
await b.close()
console.log(`\n=== ${falhas === 0 ? 'TODAS PASSARAM' : falhas + ' FALHARAM'} ===`)
process.exit(falhas ? 1 : 0)
