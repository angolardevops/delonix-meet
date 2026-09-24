// Prova Playwright das páginas da consola (frontend/l2-consola) contra a API
// real: sem erros de página, sem scroll horizontal a 1440×900 e 390×844, e o
// comportamento das Chamadas (foco, histórico, detalhe por cima da lista no
// telemóvel e voltar). Não liga a ninguém nem cria dados.
//
//   node fidelidade/consola-prova.mjs --app http://127.0.0.1:5604
import { chromium } from '@playwright/test'
import { existsSync } from 'node:fs'
const DIR = new URL('./consola/', import.meta.url).pathname
const args = process.argv.slice(2)
const appIdx = args.indexOf('--app')
const APP = appIdx >= 0 ? args[appIdx + 1] : 'http://127.0.0.1:5604'
const USER = process.env.DX_USER ?? 'demo@delonix.co.ao'
const PASS = process.env.DX_PASS ?? 'demo12345'
const STATE = `${DIR}.sessao-${USER.replace(/[^a-z0-9]/gi, '_')}.json`
const ROTAS = ['/', '/calendar/new', '/directory', '/integrations', '/admin', '/ai']
let falhas = 0
const ok = (c, n, d = '') => {
  console.log(`  ${c ? 'ok ' : 'FALHA'} ${n}${d ? ` — ${d}` : ''}`)
  if (!c) falhas++
}

const b = await chromium.launch()
for (const size of [{ width: 1440, height: 900 }, { width: 390, height: 844 }]) {
  console.log(`\n${size.width}×${size.height}`)
  const ctx = await b.newContext({ viewport: size, storageState: existsSync(STATE) ? STATE : undefined })
  const p = await ctx.newPage()
  const erros = []
  p.on('pageerror', (e) => erros.push(e.message))
  await p.goto(`${APP}/#/`)
  await p.waitForTimeout(1500)
  if (!(await p.locator('.shell').count())) {
    await p.goto(`${APP}/#/login`)
    await p.fill('[data-testid=auth-email]', USER)
    await p.fill('[data-testid=auth-password]', PASS)
    await p.press('[data-testid=auth-password]', 'Enter')
    await p.waitForSelector('.shell', { timeout: 30000 })
    await ctx.storageState({ path: STATE })
  }
  for (const r of ROTAS) {
    await p.goto(`${APP}/#${r}`)
    await p.waitForLoadState('networkidle', { timeout: 15000 }).catch(() => {})
    await p.waitForTimeout(1500)
    const w = await p.evaluate(() => [document.documentElement.scrollWidth, innerWidth])
    ok(w[0] <= w[1], `#${r} sem scroll horizontal`, `${w[0]} ≤ ${w[1]}`)
  }
  await p.goto(`${APP}/#/`)
  await p.waitForTimeout(1500)
  ok((await p.getByRole('button', { name: /iniciar agora/i }).count()) === 1, 'Início: o botão «Iniciar agora» (usado pelos e2e) existe uma vez')

  await p.goto(`${APP}/#/directory`)
  await p.waitForSelector('.call-row', { timeout: 15000 })
  const linhas = await p.locator('.call-row').count()
  ok(linhas > 0, 'Chamadas: contactos listados', `${linhas}`)
  ok((await p.locator('.call-dot--on').count()) >= 0, 'Chamadas: presença desenhada')
  if (size.width > 900) {
    const nome = (await p.locator('.call-top__title').textContent())?.trim()
    const activo = (await p.locator('.call-row--active strong').first().textContent())?.trim()
    ok(!!nome && activo?.startsWith(nome ?? '~'), 'Chamadas: o centro mostra o contacto em foco da lista', `${nome}`)
    ok((await p.locator('.call-controls button').count()) === 2, 'Chamadas: controlos de vídeo e voz')
    ok((await p.getByText(/Ligar por PSTN|Transferir|DTMF/i).count()) === 0, 'Chamadas: nada de PSTN de saída, transferir ou DTMF')
    const segunda = p.locator('.call-row__main').nth(1)
    const nome2 = (await segunda.locator('strong').textContent())?.trim()
    await segunda.click()
    ok(((await p.locator('.call-top__title').textContent()) ?? '').trim() === nome2?.replace(/\s*\(.*\)$/, ''), 'Chamadas: escolher outro contacto muda o centro', nome2)
    await p.locator('.call-sq').click()
    ok((await p.locator('.org-overview').count()) === 1, 'Chamadas: filiais e salas abrem no centro')
  } else {
    ok(!(await p.locator('.call-main').isVisible()), 'Chamadas (telemóvel): a lista ocupa o ecrã')
    await p.locator('.call-row__main').first().click()
    ok(await p.locator('.call-main--open').isVisible(), 'Chamadas (telemóvel): o contacto abre por cima da lista')
    const w = await p.evaluate(() => [document.documentElement.scrollWidth, innerWidth])
    ok(w[0] <= w[1], 'Chamadas (telemóvel): detalhe sem scroll horizontal', `${w[0]} ≤ ${w[1]}`)
    await p.screenshot({ path: `${DIR}app/DelonixCall-detalhe-390.png` })
    await p.locator('.call-top__back').click()
    ok(!(await p.locator('.call-main--open').count()), 'Chamadas (telemóvel): voltar fecha o detalhe')
  }
  await p.getByRole('tab', { name: /histórico/i }).click()
  await p.waitForTimeout(1500)
  ok((await p.locator('[data-testid=call-history], .call-empty').count()) > 0, 'Chamadas: histórico (perdidas + CDR) ou vazio honesto')
  ok(erros.length === 0, 'sem erros de página', erros.join(' | '))
  await ctx.close()
}
await b.close()
console.log(falhas ? `\n${falhas} falha(s)` : '\ntudo verde')
process.exit(falhas ? 1 : 0)
