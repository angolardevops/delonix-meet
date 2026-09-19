// Prova de comportamento do editor de diagramas num Chromium a sério (sem
// servidor: sessão falsa; o editor não precisa de API até «Guardar»).
import { chromium } from '@playwright/test'
const APP = process.env.APP ?? 'http://127.0.0.1:5505'
let falhas = 0
const ok = (n, c, d = '') => { console.log(`${c ? '  ok  ' : ' FALHA'}  ${n}${d ? '  — ' + d : ''}`); if (!c) falhas++ }
const b = await chromium.launch()
async function sessao(viewport) {
  const ctx = await b.newContext({ viewport, acceptDownloads: true })
  await ctx.addInitScript(() => {
    localStorage.setItem('dx_user', JSON.stringify({ id: '00000000-0000-0000-0000-000000000001', email: 'demo@delonix.co.ao', username: 'Demo' }))
    localStorage.setItem('dx_access', 'falso'); localStorage.setItem('dx_tour_v1', 'done')
  })
  const p = await ctx.newPage()
  p.on('pageerror', (e) => { console.log('pageerror', e.message); falhas++ })
  return { ctx, p }
}
{
  console.log('\n1440×900 · UML')
  const { p } = await sessao({ width: 1440, height: 900 })
  await p.goto(`${APP}/#/whiteboards/diagram?tipo=uml`)
  await p.waitForSelector('.dg-svg')
  await p.waitForFunction(() => /#\/whiteboards\/diagram\/d/.test(location.hash))
  ok('um quadro novo ganha id no endereço', true, await p.evaluate(() => location.hash))
  await p.getByRole('button', { name: 'Classe', exact: true }).click()
  await p.getByRole('button', { name: 'Classe', exact: true }).click()
  ok('dois cliques na paleta = duas classes', (await p.locator('[data-node-id]').count()) === 2)
  const nodes = p.locator('[data-node-id]')
  // Arrastar a segunda para a direita.
  const b2 = await nodes.nth(1).boundingBox()
  await p.mouse.move(b2.x + 20, b2.y + 10); await p.mouse.down(); await p.mouse.move(b2.x + 320, b2.y + 40, { steps: 8 }); await p.mouse.up()
  const b2b = await nodes.nth(1).boundingBox()
  ok('arrastar move o elemento', b2b.x > b2.x + 200, `${Math.round(b2.x)} → ${Math.round(b2b.x)}`)
  // Ligar pela ferramenta Herança.
  await p.getByRole('button', { name: 'Herança' }).click()
  const a = await nodes.nth(0).boundingBox(); const c = await nodes.nth(1).boundingBox()
  await p.mouse.move(a.x + a.width / 2, a.y + 12); await p.mouse.down(); await p.mouse.move(c.x + c.width / 2, c.y + 12, { steps: 6 }); await p.mouse.up()
  ok('ligar cria uma aresta', (await p.locator('[data-edge-id]').count()) === 1)
  await p.keyboard.press('Escape')
  // Inspector: tipo de relação
  await p.locator('[data-edge-id] .dg-edge__hit').first().dispatchEvent('pointerdown', { button: 0, pointerId: 1 })
  ok('a aresta seleccionada mostra Generalização', await p.getByText('Generalização').first().isVisible())
  // Interface + herança → aviso com correcção
  const sel = p.locator('.dg-card select').first()
  await sel.selectOption('realization')
  ok('mudar para realização entre classes gera erro corrigível', await p.getByRole('button', { name: 'Corrigir automaticamente' }).isVisible())
  await p.getByRole('button', { name: 'Corrigir automaticamente' }).click()
  ok('corrigir automaticamente deixa o modelo válido', await p.locator('.dg-chip.is-ok').isVisible())
  // Desfazer
  await p.locator('.dg-svg').click({ position: { x: 5, y: 5 } })
  await p.keyboard.press('Control+z')
  ok('Ctrl+Z desfaz a correcção', await p.locator('.dg-chip.is-warn').isVisible())
  // Exportações
  for (const [menu, ext] of [['XMI 2.5 (.xmi)', '.xmi'], ['PlantUML (.puml)', '.puml'], ['Imagem vectorial (.svg)', '.svg'], ['Imagem (.png)', '.png'], ['Modelo editável (.json)', '.json']]) {
    await p.getByRole('button', { name: 'Mais formatos e importar' }).click()
    const [dl] = await Promise.all([p.waitForEvent('download'), p.getByRole('menuitem', { name: menu }).click()])
    const path = await dl.path(); const fs = await import('node:fs'); const size = fs.statSync(path).size
    const head = fs.readFileSync(path).subarray(0, 8).toString('latin1')
    ok(`exporta ${ext}`, dl.suggestedFilename().endsWith(ext) && size > 100, `${dl.suggestedFilename()} ${size} B ${JSON.stringify(head)}`)
  }
  // Persistência
  const hash = await p.evaluate(() => location.hash)
  await p.waitForTimeout(900)
  await p.reload(); await p.waitForSelector('[data-node-id]')
  ok('recarregar mantém o modelo (IndexedDB)', (await p.locator('[data-node-id]').count()) === 2, hash)
  // Procurar
  await p.getByPlaceholder('Procurar elemento…').fill('Classe2')
  await p.locator('.dg-find__results button').first().click()
  ok('procurar selecciona o elemento', (await p.locator('.dg-sel__box').count()) === 1)
  // BPMN
  await p.getByRole('tab', { name: 'BPMN 2.0' }).click()
  await p.getByRole('button', { name: 'Piscina' }).click()
  await p.getByRole('button', { name: 'Tarefa', exact: true }).click()
  ok('BPMN: tarefa sem início/fim tem avisos', await p.locator('.dg-chip.is-warn').isVisible())
  await p.getByRole('tab', { name: /Validação/ }).click()
  await p.getByRole('button', { name: 'Corrigir automaticamente' }).click()
  ok('BPMN: corrigir acrescenta início e fim', (await p.locator('[data-node-id]').count()) >= 6, String(await p.locator('[data-node-id]').count()))
  const [bp] = await Promise.all([p.waitForEvent('download'), p.getByRole('button', { name: 'Exportar .bpmn' }).click()])
  const fs = await import('node:fs'); const xml = fs.readFileSync(await bp.path(), 'utf8')
  ok('BPMN exportado tem processo, eventos e DI', /<bpmn:startEvent/.test(xml) && /<bpmn:endEvent/.test(xml) && /BPMNShape/.test(xml))
  await p.getByRole('tab', { name: 'Livre' }).click()
  await p.getByRole('button', { name: 'Caneta' }).click()
  const s = await p.locator('.dg-svg').boundingBox()
  await p.mouse.move(s.x + 100, s.y + 100); await p.mouse.down(); for (let i = 0; i < 10; i++) await p.mouse.move(s.x + 100 + i * 10, s.y + 100 + i * 4); await p.mouse.up()
  ok('Livre: a caneta desenha um traço', (await p.locator('[data-stroke-id]').count()) === 1)
  await p.screenshot({ path: new URL('./app/interaccao-1440.png', import.meta.url).pathname })
  // A página Quadros lista o diagrama local
  await p.goto(`${APP}/#/whiteboards`); await p.waitForTimeout(1500)
  ok('Quadros: lista os diagramas deste browser e tem «Novo diagrama»', (await p.locator('.board-local__item').count()) >= 1 && await p.getByRole('button', { name: 'Novo diagrama' }).isVisible())
  await p.screenshot({ path: new URL('./app/quadros-1440.png', import.meta.url).pathname })
}
{
  console.log('\n390×844 · telemóvel')
  const { p } = await sessao({ width: 390, height: 844 })
  await p.goto(`${APP}/#/whiteboards/diagram?tipo=bpmn&exemplo=1`)
  await p.waitForSelector('[data-node-id]')
  const sw = await p.evaluate(() => document.documentElement.scrollWidth)
  ok('sem scroll horizontal', sw <= 390, String(sw))
  const cv = await p.locator('.dg-canvas').boundingBox()
  ok('o quadro ocupa a largura', cv.width >= 380, JSON.stringify(cv))
  await p.screenshot({ path: new URL('./app/DelonixCanvasBPMN-390.png', import.meta.url).pathname })
  await p.getByRole('button', { name: 'Elementos' }).click(); await p.waitForTimeout(300)
  const pal = await p.locator('.dg-side--left').boundingBox()
  ok('a gaveta de elementos entra no ecrã', pal.x >= -1 && pal.x < 50, JSON.stringify(pal))
  await p.screenshot({ path: new URL('./app/DelonixCanvasBPMN-390-paleta.png', import.meta.url).pathname })
  await p.getByRole('button', { name: 'Tarefa', exact: true }).click()
  await p.getByRole('button', { name: 'Propriedades' }).click(); await p.waitForTimeout(300)
  const ins = await p.locator('.dg-side--right').boundingBox()
  ok('a gaveta de propriedades entra no ecrã', ins.x + ins.width <= 391 && ins.x > 50, JSON.stringify(ins))
  await p.screenshot({ path: new URL('./app/DelonixCanvasBPMN-390-propriedades.png', import.meta.url).pathname })
}
await b.close()
console.log(falhas ? `\n${falhas} falha(s)` : '\ntudo ok')
process.exit(falhas ? 1 : 0)
