#!/usr/bin/env node
// Selecção múltipla e grupos nos diagramas (template v5), num Chromium a sério.
//
// BPMN (exemplo) e UML (classes), a 1440×900:
//   · ⇧clique em três elementos → «3 seleccionados», realce e caixa com alças;
//   · Agrupar (botão) → moldura do grupo, inspector «Selecção · 3 …»;
//   · arrastar UM membro move o grupo INTEIRO (as distâncias mantêm-se);
//   · ⇧⌘G/Ctrl+⇧+G desagrupa; Ctrl+Z desfaz (o grupo volta);
//   · caixa de selecção (arrastar no fundo) e laço escolhem os mesmos três;
//   · alinhar à esquerda → o mesmo x; distribuir → espaços iguais;
//   · recarregar a página → o grupo continua (IndexedDB);
//   · exportar .bpmn / XMI / PlantUML / SVG → o grupo está lá dentro.
//
// Uso:  BASE=http://127.0.0.1:5480 DX_USER=… DX_PASS=… SHOTS=/pasta node e2e/v5-seleccao-diagramas.mjs
import { chromium } from '@playwright/test'
import { mkdirSync, readFileSync } from 'node:fs'
import { entrar } from './sessao.mjs'

const BASE = process.env.BASE ?? 'http://127.0.0.1:5173'
const SHOTS = process.env.SHOTS ?? ''
if (SHOTS) mkdirSync(SHOTS, { recursive: true })

let falhas = 0
const ok = (nome, cond, det = '') => {
  console.log(`${cond ? '  ok  ' : ' FALHA'}  ${nome}${det ? `  — ${det}` : ''}`)
  if (!cond) falhas++
}
const shot = async (page, nome) => SHOTS && page.screenshot({ path: `${SHOTS}/${nome}.png` })

const browser = await chromium.launch()
const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, acceptDownloads: true })
const page = await ctx.newPage()
const erros = []
page.on('pageerror', (e) => erros.push(e.message))
await entrar(page, BASE, { email: process.env.DX_USER, password: process.env.DX_PASS })

const mod = process.platform === 'darwin' ? 'Meta' : 'Control'
const caixa = (id) => page.locator(`[data-node-id="${id}"]`).boundingBox()
const posicoes = (ids) => page.evaluate((ids) => ids.map((id) => {
  const r = document.querySelector(`[data-node-id="${id}"]`).getBoundingClientRect()
  return [Math.round(r.x), Math.round(r.y), Math.round(r.width)]
}), ids)
const descarregar = async (formato) => {
  await page.locator('.dg-export [aria-haspopup=menu]').click()
  const [d] = await Promise.all([page.waitForEvent('download', { timeout: 20000 }), page.locator(`[data-export="${formato}"]`).click()])
  return readFileSync(await d.path(), 'utf8')
}
const arrastar = async (from, to, opts = {}) => {
  await page.mouse.move(from.x, from.y)
  await page.mouse.down()
  for (const p of opts.via ?? []) await page.mouse.move(p.x, p.y, { steps: 6 })
  await page.mouse.move(to.x, to.y, { steps: 10 })
  await page.mouse.up()
}

async function cenario({ nome, url, ids, formatos }) {
  console.log(`\n── ${nome}`)
  await page.goto(`${BASE}/#/whiteboards/diagram${url}`)
  await page.waitForSelector(`[data-node-id="${ids[0]}"]`, { timeout: 30000 })
  await page.waitForTimeout(600)
  ok('mini-barra com seleccionar, mover e mão', (await page.locator('.dg-minibar [data-tool]').count()) === 3)
  ok('ajuda de teclado «V · M · H · ⇧clique · ⌘G»', /V · M · H · ⇧/.test(await page.locator('.dg-keys').innerText()))

  // ⇧clique em três
  await page.locator('.dg-svg').click({ position: { x: 5, y: 5 } })
  for (const [i, id] of ids.entries()) {
    const b = await caixa(id)
    // `mouse.click` não aceita modificadores: o ⇧ carrega-se no teclado.
    if (i > 0) await page.keyboard.down('Shift')
    await page.mouse.click(b.x + b.width / 2, b.y + 8)
    if (i > 0) await page.keyboard.up('Shift')
  }
  const barra = page.locator('.dx-selbar')
  ok('⇧clique: barra «3 seleccionados»', /3/.test(await barra.locator('.dx-selbar__count').innerText().catch(() => '')))
  ok('caixa tracejada com 4 alças', (await page.locator('.dg-multi__handle').count()) === 4)
  await shot(page, `${nome}-1-shift-clique`)

  // Agrupar
  await barra.getByRole('button', { name: /Agrupar|Group|Grouper|编组/ }).first().click()
  ok('agrupar: moldura do grupo no quadro', (await page.locator('[data-group-id]').count()) === 1)
  ok('inspector «Selecção · 3 …» com o grupo', /3/.test(await page.locator('[data-selcard] strong').first().innerText()) && (await page.locator('.dg-selcard__group').count()) === 1)
  await shot(page, `${nome}-2-agrupado`)

  // Mover o grupo arrastando UM membro
  await page.locator('.dg-svg').click({ position: { x: 5, y: 5 } })
  const antes = await posicoes(ids)
  const b0 = await caixa(ids[1])
  await arrastar({ x: b0.x + b0.width / 2, y: b0.y + 8 }, { x: b0.x + b0.width / 2 + 60, y: b0.y + 8 + 40 })
  const depois = await posicoes(ids)
  const dx = depois.map((d, i) => d[0] - antes[i][0])
  const dy = depois.map((d, i) => d[1] - antes[i][1])
  ok('arrastar um membro move o grupo inteiro', dx.every((v) => Math.abs(v - dx[0]) <= 1 && v > 20) && dy.every((v) => Math.abs(v - dy[0]) <= 1 && v > 10), `dx=${dx} dy=${dy}`)
  await shot(page, `${nome}-3-grupo-movido`)

  // Desagrupar por teclado e desfazer
  await page.keyboard.press(`${mod}+Shift+G`)
  ok('⇧⌘G desagrupa', (await page.locator('[data-group-id]').count()) === 0)
  await page.keyboard.press(`${mod}+Z`)
  ok('Ctrl+Z desfaz o desagrupar (o grupo volta)', (await page.locator('[data-group-id]').count()) === 1)
  await page.keyboard.press(`${mod}+Z`)
  const aposUndoMover = await posicoes(ids)
  ok('Ctrl+Z desfaz o movimento do grupo', aposUndoMover.every((p, i) => p[0] === antes[i][0] && p[1] === antes[i][1]))
  await page.keyboard.press(`${mod}+Shift+Z`)

  // Persistência: recarregar
  await page.waitForTimeout(900)
  await page.reload()
  await page.waitForSelector(`[data-node-id="${ids[0]}"]`, { timeout: 30000 })
  await page.waitForTimeout(600)
  ok('recarregar: o grupo continua (IndexedDB)', (await page.locator('[data-group-id]').count()) === 1)

  // Exportações com o grupo
  for (const f of formatos) {
    const txt = await descarregar(f)
    const tem =
      f === 'bpmn' ? /<bpmn:group id="SelGroup_[^"]+" categoryValueRef="SelCategoryValue_/.test(txt) && /bpmnElement="SelGroup_/.test(txt)
      : f === 'xmi' ? /<xmi:Extension extender="Delonix Meet">[\s\S]*<group xmi:id="grp_/.test(txt)
      : f === 'plantuml' ? /^' group .+: .+, .+/m.test(txt)
      : f === 'svg' ? /stroke-dasharray="6 4"/.test(txt) && !/data-ui/.test(txt)
      : false
    let xmlOk = true
    if (f === 'bpmn' || f === 'xmi' || f === 'svg') xmlOk = await page.evaluate((x) => !new DOMParser().parseFromString(x, 'application/xml').querySelector('parsererror'), txt)
    ok(`exportar ${f}: o grupo está lá dentro${f === 'plantuml' ? '' : ' e o XML é válido'}`, tem && xmlOk, `${txt.length} bytes`)
  }

  // Desagrupar (botão), caixa de selecção e alinhar/distribuir
  const b1 = await caixa(ids[0])
  await page.mouse.click(b1.x + b1.width / 2, b1.y + 8)
  await page.locator('.dx-selbar').getByRole('button', { name: /Desagrupar|Ungroup|Dissocier|取消编组/ }).click()
  ok('desagrupar pelo botão', (await page.locator('[data-group-id]').count()) === 0)
  await page.keyboard.press('Escape')
  const bs = await Promise.all(ids.map(caixa))
  const x0 = Math.min(...bs.map((b) => b.x)) - 14
  const y0 = Math.min(...bs.map((b) => b.y)) - 14
  const x1 = Math.max(...bs.map((b) => b.x + b.width)) + 14
  const y1 = Math.max(...bs.map((b) => b.y + b.height)) + 14
  await page.locator('.dg-minibar [data-tool=select]').click()
  await arrastar({ x: x0, y: y0 }, { x: x1, y: y1 })
  const nCaixa = Number((await page.locator('.dx-selbar__count').innerText().catch(() => '0')).match(/\d+/)?.[0] ?? 0)
  ok('caixa de selecção apanha os elementos', nCaixa >= 3, `${nCaixa}`)
  await shot(page, `${nome}-4-caixa`)

  // Laço sobre os mesmos: primeiro Escape, depois a ferramenta laço da paleta
  await page.keyboard.press('Escape')
  const laco = page.locator('[data-palette-item=lasso]')
  if (await laco.count()) {
    await laco.first().click()
    await arrastar({ x: x0, y: y0 }, { x: x0, y: y0 }, { via: [{ x: x1, y: y0 }, { x: x1, y: y1 }, { x: x0, y: y1 }] })
    const nLaco = Number((await page.locator('.dx-selbar__count').innerText().catch(() => '0')).match(/\d+/)?.[0] ?? 0)
    ok('laço apanha os elementos', nLaco >= 3, `${nLaco}`)
    await page.locator('[data-selcard] .dg-selcard__via').innerText().then((v) => ok('inspector diz como se escolheu («laço»)', /laço|lasso|套索/.test(v), v))
    await page.keyboard.press('Escape')
    await page.keyboard.press('v')
  }

  // Só os três, por ⇧clique, para alinhar e distribuir
  for (const [i, id] of ids.entries()) {
    const b = await caixa(id)
    // `mouse.click` não aceita modificadores: o ⇧ carrega-se no teclado.
    if (i > 0) await page.keyboard.down('Shift')
    await page.mouse.click(b.x + b.width / 2, b.y + 8)
    if (i > 0) await page.keyboard.up('Shift')
  }
  await page.locator('.dx-selbar [data-align=left]').click()
  const alinhados = await posicoes(ids)
  ok('alinhar à esquerda: o mesmo x', alinhados.every((p) => Math.abs(p[0] - alinhados[0][0]) <= 1), JSON.stringify(alinhados.map((p) => p[0])))
  await page.keyboard.press(`${mod}+Z`)
  const bx = await Promise.all(ids.map(caixa))
  await page.locator('.dx-selbar [data-distribute=x]').click()
  const dist = (await Promise.all(ids.map(caixa))).sort((a, b) => a.x - b.x)
  const gaps = [dist[1].x - (dist[0].x + dist[0].width), dist[2].x - (dist[1].x + dist[1].width)]
  ok('distribuir na horizontal: espaços iguais', Math.abs(gaps[0] - gaps[1]) <= 3, `vãos=${gaps.map(Math.round)} antes=${bx.map((b) => Math.round(b.x))}`)
  await shot(page, `${nome}-5-distribuido`)
  await page.keyboard.press(`${mod}+Z`)
}

await cenario({ nome: 'bpmn', url: '?tipo=bpmn&exemplo=1', ids: ['x_m', 'x_o', 'x_emit'], formatos: ['bpmn', 'svg'] })
await cenario({ nome: 'uml', url: '?tipo=uml&exemplo=1', ids: ['x_session', 'x_recording', 'x_transcript'], formatos: ['xmi', 'plantuml', 'svg'] })

ok('sem erros de página', erros.length === 0, erros.join(' | '))
await browser.close()
console.log(falhas ? `\n${falhas} FALHA(S)` : '\ntudo ok')
process.exit(falhas ? 1 : 0)
