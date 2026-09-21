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
import { chromium } from '@playwright/test'
import { criarConta, entrar } from './sessao.mjs'

const BASE = process.env.BASE ?? process.env.APP ?? 'http://127.0.0.1:4180'
const API = process.env.API ?? BASE
const conta = await criarConta(API, 'lay')
let falhas = 0

function ok(nome, condicao, detalhe = '') {
  console.log(`${condicao ? '  ok  ' : ' FALHA'}  ${nome}${detalhe ? `  — ${detalhe}` : ''}`)
  if (!condicao) falhas++
}

const browser = await chromium.launch()

// ---------------------------------------------------------------- 3.1.1
console.log('\n3.1.1 · a gaveta em ecrã estreito (375×812)')
{
  const ctx = await browser.newContext({ locale: 'pt-PT', ignoreHTTPSErrors: true, viewport: { width: 375, height: 812 } })
  const page = await ctx.newPage()
  // Entra a sério. Injectar um token falso funcionava contra um mock e falha
  // contra o servidor real: leva 401, o cliente renova, falha, faz logout, e o
  // teste morre no ecrã de login (ver e2e/sessao.mjs).
  await entrar(page, BASE, conta)
  await page.setViewportSize({ width: 375, height: 812 })
  await page.reload()
  await page.waitForSelector('.shell', { timeout: 30_000 })

  const nav = page.locator('.shell-nav')
  const caixaFechada = await nav.boundingBox()
  ok('o rail está FORA do ecrã com a gaveta fechada', caixaFechada.x + caixaFechada.width <= 1,
     `x=${Math.round(caixaFechada.x)} largura=${Math.round(caixaFechada.width)}`)

  const conteudo = await page.locator('.shell-main').boundingBox()
  ok('o conteúdo ocupa a largura toda (antes perdia 224px)', Math.round(conteudo.x) === 0 && Math.round(conteudo.width) === 375,
     `x=${Math.round(conteudo.x)} largura=${Math.round(conteudo.width)}`)

  // O botão da gaveta vive na barra de cada página (PageBar).
  await page.click('.page-bar__burger')
  await page.waitForTimeout(450)
  const caixaAberta = await nav.boundingBox()
  // A prova que faltava: a gaveta ENTRA mesmo no ecrã.
  ok('a gaveta desliza para dentro do ecrã', Math.round(caixaAberta.x) === 0 && caixaAberta.width > 200,
     `x=${Math.round(caixaAberta.x)} largura=${Math.round(caixaAberta.width)}`)
  ok('o transform computado é `none` com a gaveta aberta',
     (await nav.evaluate((e) => getComputedStyle(e).transform)) === 'none')
  ok('o backdrop cobre o ecrã', await page.locator('.shell-nav-backdrop').isVisible())
  ok('aria-expanded acompanha', (await page.getAttribute('.page-bar__burger', 'aria-expanded')) === 'true')

  // Entrar por código no telemóvel: a Início tem a caixa no corpo (R103), e a
  // pesquisa da gaveta abre a paleta, que também aceita o código colado.
  ok('a pesquisa está alcançável na gaveta', await page.locator('.shell-nav__search').isVisible())

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
// Aponta aos sítios onde o anel é NOSSO e não do browser: o campo sem borda da
// paleta (anel no contentor) e o botão do rail (rede :focus-visible).
{
  const ctx = await browser.newContext({ locale: 'pt-PT', ignoreHTTPSErrors: true, viewport: { width: 1280, height: 800 } })
  const page = await ctx.newPage()
  await entrar(page, BASE, conta)

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

  // O botão de pesquisa do rail: um botão, logo coberto pela rede :focus-visible.
  const r1 = await anelVisivel(page, '.shell-nav', '.shell-nav__search')
  ok('o botão de pesquisa do rail mostra foco', r1.mudou)

  // O Cmd-K: teclado é a única forma de o usar.
  await page.keyboard.press('Control+k')
  await page.waitForSelector('.palette__search input', { timeout: 5000 })

  // O contentor recebe o anel via :focus-within (o input não tem borda).
  const viaFocusWithin = await page.evaluate(() => {
    const c = document.querySelector('.palette__search')
    c.querySelector('input').focus()
    return { focusWithin: c.matches(':focus-within'), sombra: getComputedStyle(c).boxShadow }
  })
  await page.keyboard.press('Escape')
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
