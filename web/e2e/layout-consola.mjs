#!/usr/bin/env node
// Verificação de LAYOUT da consola num motor de browser a sério.
//
// Existe por uma razão medida: o painel de browser do agente NÃO resolve
// `transform` nem `outline` — um `!important` inline nessas propriedades também
// não altera o valor computado. Duas correções do levantamento (a gaveta móvel,
// achado 3.1.1, e o anel de foco, achado 4.3) vivem exactamente nessas duas
// propriedades, e ficaram por confirmar nos PRs #11 e #13.
//
// Isto corre contra o `dist` construído, servido por um servidor qualquer.
//
// Requer o Playwright, que NÃO está no package.json — ver a nota no fim deste
// ficheiro sobre porquê e o que decidir.
//
//   npm i -D playwright && npx playwright install chromium
//   (cd web && npm run build)
//   <servir web/dist em BASE>
//   BASE=http://127.0.0.1:4180 node e2e/layout-consola.mjs
import { chromium } from 'playwright'

const BASE = process.env.BASE ?? 'http://127.0.0.1:4180'
let falhas = 0

function ok(nome, condicao, detalhe = '') {
  console.log(`${condicao ? '  ok  ' : ' FALHA'}  ${nome}${detalhe ? `  — ${detalhe}` : ''}`)
  if (!condicao) falhas++
}

const browser = await chromium.launch()

// ---------------------------------------------------------------- 3.1.1
console.log('\n3.1.1 · a gaveta em ecrã estreito (375×812)')
{
  const ctx = await browser.newContext({ ignoreHTTPSErrors: true, viewport: { width: 375, height: 812 } })
  const page = await ctx.newPage()
  await page.goto(BASE)
  await page.evaluate(() => {
    localStorage.setItem('dx_user', JSON.stringify({ id: 1, username: 'walter', email: 'w@delonix.local', locale: 'pt' }))
    localStorage.setItem('dx_access', 'tok')
  })
  await page.goto(BASE)
  await page.waitForSelector('.shell', { timeout: 10_000 })

  const nav = page.locator('.shell-nav')
  const caixaFechada = await nav.boundingBox()
  ok('o rail está FORA do ecrã com a gaveta fechada', caixaFechada.x + caixaFechada.width <= 1,
     `x=${Math.round(caixaFechada.x)} largura=${Math.round(caixaFechada.width)}`)

  const conteudo = await page.locator('.shell-main').boundingBox()
  ok('o conteúdo ocupa a largura toda (antes perdia 224px)', Math.round(conteudo.x) === 0 && Math.round(conteudo.width) === 375,
     `x=${Math.round(conteudo.x)} largura=${Math.round(conteudo.width)}`)

  await page.click('.app-bar-burger')
  await page.waitForTimeout(450)
  const caixaAberta = await nav.boundingBox()
  // A prova que faltava: a gaveta ENTRA mesmo no ecrã.
  ok('a gaveta desliza para dentro do ecrã', Math.round(caixaAberta.x) === 0 && caixaAberta.width > 200,
     `x=${Math.round(caixaAberta.x)} largura=${Math.round(caixaAberta.width)}`)
  ok('o transform computado é `none` com a gaveta aberta',
     (await nav.evaluate((e) => getComputedStyle(e).transform)) === 'none')
  ok('o backdrop cobre o ecrã', await page.locator('.shell-nav-backdrop').isVisible())
  ok('aria-expanded acompanha', (await page.getAttribute('.app-bar-burger', 'aria-expanded')) === 'true')

  const campo = page.locator('.qa-drawer .app-bar-join input')
  ok('entrar por código está alcançável na gaveta', await campo.isVisible())
  await campo.fill('azul-monte-rio')
  ok('e aceita escrita', (await campo.inputValue()) === 'azul-monte-rio')

  await page.keyboard.press('Escape')
  await page.waitForTimeout(400)
  const caixaEsc = await nav.boundingBox()
  ok('Escape fecha a gaveta', caixaEsc.x + caixaEsc.width <= 1)

  ok('sem scroll horizontal', await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth))
  await ctx.close()
}

// ---------------------------------------------------------------- 4.3
console.log('\n4.3 · o anel nos controlos que ESTAVAM cegos')
//
// O teste tem de apontar aos seis sítios que tinham `outline: none` sem
// substituto — não a um botão qualquer. Um botão que nunca perdeu o anel do
// browser passa o teste sem provar nada.
{
  const ctx = await browser.newContext({ ignoreHTTPSErrors: true, viewport: { width: 1280, height: 800 } })
  const page = await ctx.newPage()
  await page.goto(BASE)
  await page.evaluate(() => {
    localStorage.setItem('dx_user', JSON.stringify({ id: 1, username: 'walter', email: 'w@delonix.local', locale: 'pt' }))
    localStorage.setItem('dx_access', 'tok')
  })
  await page.goto(BASE)
  await page.waitForSelector('.shell', { timeout: 10_000 })

  // Mede o anel POR PIXÉIS: é a única leitura que não depende de como o
  // Chromium serializa o `outline` do próprio browser.
  async function anelVisivel(page, seletorContentor, seletorFoco) {
    const antes = await page.locator(seletorContentor).screenshot()
    await page.locator(seletorFoco).focus()
    // `focus()` programático não activa :focus-visible; o Tab a partir do
    // elemento anterior activa. Simula-se com uma tecla real.
    await page.keyboard.press('Shift+Tab')
    await page.keyboard.press('Tab')
    await page.waitForTimeout(150)
    const depois = await page.locator(seletorContentor).screenshot()
    return { mudou: Buffer.compare(antes, depois) !== 0, bytes: [antes.length, depois.length] }
  }

  // A barra de topo: o campo de código fundido no contentor.
  const r1 = await anelVisivel(page, '.qa-bar .app-bar-join', '.qa-bar .app-bar-join input')
  ok('o campo de código da barra mostra foco', r1.mudou)

  // O Cmd-K: era o pior dos seis — teclado é a única forma de o usar.
  await page.keyboard.press('Control+k')
  await page.waitForSelector('.cmd-search input', { timeout: 5000 })
  const r2 = await anelVisivel(page, '.cmd-search', '.cmd-search input')
  ok('o campo do Cmd-K mostra foco', r2.mudou)
  await page.keyboard.press('Escape')

  // E o contentor recebe mesmo o anel via :focus-within.
  const viaFocusWithin = await page.evaluate(() => {
    const c = document.querySelector('.qa-bar .app-bar-join')
    c.querySelector('input').focus()
    return { focusWithin: c.matches(':focus-within'), sombra: getComputedStyle(c).boxShadow }
  })
  ok('o contentor casa :focus-within', viaFocusWithin.focusWithin)
  ok('e ganha o anel do sistema (box-shadow, não none)',
     viaFocusWithin.sombra !== 'none', viaFocusWithin.sombra.slice(0, 46))

  await ctx.close()
}

await browser.close()
console.log(falhas === 0 ? '\nTUDO VERDE\n' : `\n${falhas} FALHA(S)\n`)
process.exit(falhas === 0 ? 0 : 1)

// ---------------------------------------------------------------------------
//  Nota sobre a dependência, para quem decidir se isto entra no CI
//
//  O `ws` do e2e/isolamento.mjs está em devDependencies, e o convénio do repo é
//  esse. O Playwright NÃO foi acrescentado por decisão própria: obriga o `npm
//  ci` de TODOS os jobs a descarregá-lo, mais um `playwright install chromium`,
//  e isso é custo de build para toda a gente — é uma decisão de quem mantém o
//  repo, não um efeito lateral de uma correcção de layout.
//
//  Este arnês foi corrido à mão e os dois portões foram VISTOS a ficar
//  vermelhos com o invariante partido:
//    · regra da gaveta trocada por um selector inexistente -> 2 falhas;
//    · anéis de :focus-within removidos                    -> 2 falhas.
//  Se entrar no CI, é aqui que se liga.
// ---------------------------------------------------------------------------
