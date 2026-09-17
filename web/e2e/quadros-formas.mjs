#!/usr/bin/env node
// Formas novas dos quadros de diagramas num Chromium a sério, contra a API.
//
// Para cada contexto (UML, BPMN, fluxograma, arquitectura, livre):
//   · fotografa cada grupo NOVO da paleta;
//   · coloca uma forma de cada grupo novo (clique) e duas por arrasto;
//   · liga duas formas com a ferramenta de aresta do contexto;
//   · espera pela gravação no IndexedDB, recarrega e confirma as formas lá;
//   · exporta SVG e confirma os nomes das formas lá dentro;
//   · exporta o formato do contexto (XMI/PlantUML, .bpmn, C4-PlantUML) e
//     valida a estrutura (XML pelo DOMParser do browser).
// Na arquitectura prova também que o desenho de um grupo do catálogo só é
// pedido quando o grupo abre (import dinâmico).
//
// Uso:  BASE=http://127.0.0.1:5871 DX_USER=… DX_PASS=… SHOTS=/pasta node e2e/quadros-formas.mjs
//       STATE=/sessao.json reutiliza uma sessão (o login tem limite de tentativas).
//       VIEWPORTS=1440x900,1920x1080 (por omissão os dois). GUARDAR=1 envia o PNG
//       de um quadro de arquitectura para «Quadros» (escreve no servidor).
import { chromium } from '@playwright/test'
import { existsSync, mkdirSync, readFileSync } from 'node:fs'
import { entrar } from './sessao.mjs'

const BASE = process.env.BASE ?? 'http://127.0.0.1:5173'
const SHOTS = process.env.SHOTS ?? ''
const STATE = process.env.STATE ?? ''
const VIEWPORTS = (process.env.VIEWPORTS ?? '1440x900,1920x1080').split(',').map((v) => v.split('x').map(Number))
const GUARDAR = process.env.GUARDAR === '1'

let falhas = 0
const ok = (nome, cond, det = '') => {
  console.log(`${cond ? '  ok  ' : ' FALHA'}  ${nome}${det ? `  — ${det}` : ''}`)
  if (!cond) falhas++
}

/** Grupos NOVOS por contexto, a forma a pôr de cada um, e o par para ligar. */
const CONTEXTOS = {
  uml: {
    grupos: ['activity', 'states', 'components', 'objects'],
    ligar: { a: 'action', b: 'action', aresta: 'controlFlow' },
    formatos: ['xmi', 'plantuml'],
  },
  bpmn: {
    grupos: ['intermediate', 'endEvents', 'taskTypes', 'moreGateways', 'dataAndFlows'],
    ligar: { a: 'sendTask', b: 'receiveTask', aresta: 'sequenceFlow' },
    formatos: ['bpmn'],
  },
  flow: {
    grupos: ['flowProcess', 'flowData', 'flowConnectors'],
    ligar: { a: 'predefinedProcess', b: 'flowDatabase', aresta: 'flow' },
    formatos: [],
  },
  arch: {
    grupos: ['c4', 'cloud', 'aws', 'azure', 'gcp', 'k8s', 'network', 'onprem', 'platform'],
    ligar: { a: 'c4Container', b: 'c4ContainerDb', aresta: 'c4Rel' },
    formatos: ['c4'],
  },
  free: {
    grupos: ['quickShapes', 'stickies'],
    ligar: null,
    formatos: [],
  },
}

const browser = await chromium.launch()

for (const [W, H] of VIEWPORTS) {
  console.log(`\n==== ${W}×${H}`)
  const dir = SHOTS ? `${SHOTS}/${W}x${H}` : ''
  if (dir) mkdirSync(dir, { recursive: true })
  const ctx = await browser.newContext({ viewport: { width: W, height: H }, acceptDownloads: true, ...(STATE && existsSync(STATE) ? { storageState: STATE } : {}) })
  const page = await ctx.newPage()
  const erros = []
  page.on('pageerror', (e) => erros.push(e.message.slice(0, 160)))
  const pedidos = []
  page.on('request', (r) => pedidos.push(r.url()))

  await page.goto(`${BASE}/#/whiteboards`)
  await page.waitForTimeout(1500)
  if (await page.locator('[data-testid=auth-email]').count()) {
    if (!process.env.DX_USER) throw new Error('sessão expirada: passar DX_USER/DX_PASS')
    await entrar(page, BASE, { email: process.env.DX_USER, password: process.env.DX_PASS })
  }

  for (const [notacao, c] of Object.entries(CONTEXTOS)) {
    console.log(`\n-- ${notacao}`)
    // Grupos abertos/fechados ficam guardados neste browser: começa-se do estado por omissão.
    await page.evaluate(() => localStorage.removeItem('dx-diagram-palette-closed'))
    const desde = pedidos.length
    await page.goto(`${BASE}/#/whiteboards/diagram?tipo=${notacao}`)
    await page.waitForSelector('.dg-palette', { timeout: 30000 })
    await page.waitForFunction(() => /#\/whiteboards\/diagram\/d/.test(location.hash), null, { timeout: 15000 })
    const id = page.url().split('/').pop()

    if (notacao === 'arch') {
      await page.waitForTimeout(1500)
      const antes = pedidos.slice(desde).filter((u) => /catalog\/aws/.test(u)).length
      ok('o desenho do grupo AWS não é pedido antes de o grupo abrir', antes === 0, `${antes} pedidos`)
    }

    // Abre só os grupos novos (os outros ficam como estão) e fotografa-os.
    for (const g of c.grupos) {
      const toggle = page.locator(`[data-palette-group="${g}"] .dg-group__toggle`)
      if ((await toggle.getAttribute('aria-expanded')) === 'false') await toggle.click()
      if (notacao === 'arch' && g !== 'c4') {
        await page.waitForFunction((gr) => !document.querySelector(`[data-palette-group="${gr}"] .dg-item__tile rect[stroke-dasharray]`), g, { timeout: 15000 })
      }
      if (dir) await page.locator(`[data-palette-group="${g}"]`).screenshot({ path: `${dir}/${notacao}-grupo-${g}.png` })
    }
    if (notacao === 'arch') {
      ok('abrir o grupo AWS pede o módulo do grupo', pedidos.some((u) => /catalog\/aws/.test(u)))
    }

    // Uma forma de cada grupo novo, por clique.
    const colocados = []
    for (const g of c.grupos) {
      const item = page.locator(`[data-palette-group="${g}"] .dg-item[draggable=true]`).first()
      if (!(await item.count())) continue
      colocados.push(await item.getAttribute('data-palette-item'))
      await item.click()
    }
    const nClique = await page.locator('[data-node-id]').count()
    ok(`uma forma de cada grupo novo no quadro (${colocados.length})`, nClique >= colocados.length, colocados.join(', '))

    // Livre: traço, forma rápida e laço a mover os dois.
    if (notacao === 'free') {
      const svg = await page.locator('.dg-svg').boundingBox()
      const arrasta = async (pts) => {
        await page.mouse.move(svg.x + pts[0][0], svg.y + pts[0][1])
        await page.mouse.down()
        for (const [x, y] of pts.slice(1)) await page.mouse.move(svg.x + x, svg.y + y, { steps: 6 })
        await page.mouse.up()
      }
      await page.locator('[data-palette-item=marker]').click()
      await arrasta([[80, 80], [220, 90]])
      await page.locator('[data-palette-item=quickEllipse]').click()
      await arrasta([[80, 120], [200, 200]])
      await page.locator('[data-palette-item=lasso]').click()
      await arrasta([[40, 40], [260, 40], [260, 240], [40, 240], [40, 40]])
      const antes = await page.locator('[data-stroke-id] path:not([data-ui])').evaluateAll((els) => els.map((e) => e.getAttribute('d')))
      const caixa = await page.locator('.dg-multi').boundingBox()
      await page.mouse.move(caixa.x + 8, caixa.y + 8)
      await page.mouse.down()
      await page.mouse.move(caixa.x + 108, caixa.y + 48, { steps: 8 })
      await page.mouse.up()
      const depois = await page.locator('[data-stroke-id] path:not([data-ui])').evaluateAll((els) => els.map((e) => e.getAttribute('d')))
      ok('marcador e elipse desenhados, laço apanha os dois e move-os', antes.length === 2 && depois.every((d, i) => d !== antes[i]))
      await page.keyboard.press('Escape')
    }

    // Duas formas por arrasto, afastadas, e a ligação entre elas.
    if (c.ligar) {
      const svg = await page.locator('.dg-svg').boundingBox()
      const alvoA = { x: 120, y: svg.height - 160 }
      const alvoB = { x: 420, y: svg.height - 160 }
      await page.locator(`[data-palette-item="${c.ligar.a}"]`).first().dragTo(page.locator('.dg-svg'), { targetPosition: alvoA })
      await page.locator(`[data-palette-item="${c.ligar.b}"]`).first().dragTo(page.locator('.dg-svg'), { targetPosition: alvoB })
      const ids = await page.locator('[data-node-id]').evaluateAll((els) => els.map((e) => e.getAttribute('data-node-id')))
      const [ia, ib] = ids.slice(-2)
      ok('arrastar da paleta coloca a forma onde cai', ids.length === nClique + 2)
      await page.locator(`[data-palette-item="${c.ligar.aresta}"]`).first().click()
      const ba = await page.locator(`[data-node-id="${ia}"]`).boundingBox()
      const bb = await page.locator(`[data-node-id="${ib}"]`).boundingBox()
      await page.mouse.move(ba.x + ba.width / 2, ba.y + ba.height / 2)
      await page.mouse.down()
      await page.mouse.move(bb.x + bb.width / 2, bb.y + bb.height / 2, { steps: 10 })
      await page.mouse.up()
      await page.keyboard.press('Escape')
      const arestas = await page.locator('[data-edge-id]').count()
      ok(`ligar ${c.ligar.a} → ${c.ligar.b} por ${c.ligar.aresta}`, arestas >= 1, `${arestas} arestas`)
    }

    await page.locator('.dg-zoom__fit').click()
    await page.waitForTimeout(400)
    if (dir) await page.screenshot({ path: `${dir}/${notacao}-quadro.png` })

    // Gravado no IndexedDB → recarregar → as mesmas formas.
    const noEcra = await page.locator('[data-node-id]').count()
    const guardado = async () =>
      page.evaluate(
        (docId) =>
          new Promise((res) => {
            const r = indexedDB.open('delonix-diagramas', 1)
            r.onsuccess = () => {
              const g = r.result.transaction('docs').objectStore('docs').get(docId)
              g.onsuccess = () => res(g.result ? { nodes: g.result.nodes.map((n) => n.type + (n.props.catalog ? `:${n.props.catalog}` : '')), edges: g.result.edges.map((e) => e.type), strokes: g.result.strokes.length } : null)
            }
          }),
        id,
      )
    let doc = null
    for (let i = 0; i < 30; i++) {
      doc = await guardado()
      if (doc && doc.nodes.length === noEcra) break
      await page.waitForTimeout(300)
    }
    ok('o modelo fica gravado neste browser com todas as formas', doc?.nodes.length === noEcra, `${doc?.nodes.length}/${noEcra}`)
    await page.reload()
    await page.waitForSelector('[data-node-id]', { timeout: 30000 })
    await page.waitForTimeout(800)
    const reaberto = await page.locator('[data-node-id]').count()
    ok('reabrir mostra as mesmas formas', reaberto === noEcra, `${reaberto}/${noEcra}`)
    if (notacao === 'arch') {
      // Os mosaicos dos grupos usados voltam a chegar depois de recarregar.
      const semDesenho = await page.locator('[data-node-id] rect[fill="#f2f2f3"][width="36"]').count()
      ok('recursos do catálogo reabrem com o desenho do grupo', semDesenho === 0, `${semDesenho} sem desenho`)
    }
    if (dir) await page.screenshot({ path: `${dir}/${notacao}-reaberto.png` })

    // SVG com as formas.
    const exportar = async (f) => {
      const main = page.locator('.dg-export__main')
      const menu = page.locator(`[data-export="${f}"]`)
      const [dl] = await Promise.all([
        page.waitForEvent('download', { timeout: 20000 }),
        (async () => {
          if (f === 'svg' && notacao !== 'uml' && notacao !== 'bpmn') await main.click()
          else {
            await page.locator('.dg-export .dx-iconbtn').click()
            await menu.click()
          }
        })(),
      ])
      return readFileSync(await dl.path(), 'utf8')
    }
    const svg = await exportar('svg')
    // Folhas de texto: um <text> com várias linhas tem-nas em <tspan>.
    const nomes = await page.evaluate(() =>
      [...document.querySelectorAll('[data-node-id] tspan, [data-node-id] text')]
        .filter((t) => !t.querySelector('tspan'))
        .map((t) => t.textContent)
        .filter(Boolean),
    )
    const faltam = nomes.filter((n) => !svg.includes(n.replace(/&/g, '&amp;').replace(/</g, '&lt;')))
    ok('o SVG exportado tem as formas (todos os textos do quadro)', nomes.length > 0 && faltam.length === 0, faltam.slice(0, 3).join(' | '))
    const svgOk = await page.evaluate((s) => !new DOMParser().parseFromString(s, 'image/svg+xml').querySelector('parsererror'), svg)
    ok('o SVG é XML válido', svgOk)
    if (notacao === 'arch') ok('o SVG leva o desenho do catálogo (cor AWS)', /#ff9900/i.test(svg))
    if (notacao === 'free') ok('o SVG leva os traços (marcador translúcido)', /stroke-opacity="0.35"/.test(svg))

    for (const f of c.formatos) {
      const txt = await exportar(f)
      if (f === 'xmi' || f === 'bpmn') {
        const r = await page.evaluate((s) => {
          const d = new DOMParser().parseFromString(s, 'application/xml')
          if (d.querySelector('parsererror')) return { erro: d.querySelector('parsererror').textContent.slice(0, 120) }
          return { tags: [...new Set([...d.getElementsByTagName('*')].map((e) => e.getAttribute('xmi:type') || e.localName))] }
        }, txt)
        ok(`.${f} é XML bem formado`, !r.erro, r.erro)
        if (f === 'xmi') {
          for (const t of ['uml:Activity', 'uml:OpaqueAction', 'uml:ControlFlow', 'uml:StateMachine', 'uml:Component', 'uml:InstanceSpecification']) ok(`XMI tem ${t}`, r.tags?.includes(t))
        } else {
          for (const t of ['intermediateCatchEvent', 'endEvent', 'sendTask', 'receiveTask', 'complexGateway', 'dataStoreReference', 'sequenceFlow', 'BPMNShape']) ok(`.bpmn tem ${t}`, r.tags?.includes(t))
          ok('.bpmn: a sequência liga a tarefa de envio à de recepção', /<bpmn:sequenceFlow id="[^"]+" sourceRef="N_[^"]+" targetRef="N_[^"]+"\/>/.test(txt))
        }
      } else if (f === 'plantuml') {
        ok('PlantUML: blocos equilibrados e actividade presente', (txt.match(/@startuml/g) ?? []).length === (txt.match(/@enduml/g) ?? []).length && /_actividade/.test(txt))
      } else if (f === 'c4') {
        ok('C4-PlantUML: include, contentor BD e relação', /!include <C4\//.test(txt) && /ContainerDb\(/.test(txt) && /Rel\(/.test(txt), txt.split('\n').slice(0, 3).join(' / '))
      }
    }

    if (notacao === 'arch' && GUARDAR && W === VIEWPORTS[0][0]) {
      await page.locator('.dg-bar__save').click()
      await page.locator('[role=dialog] button.dx-btn--primary').last().click()
      // O aviso de sucesso é o que traz a ligação «Ver em Quadros» (o de exportar não a tem).
      const aviso = await page.waitForSelector('.dg-notice .dg-link', { timeout: 60000 }).catch(() => null)
      const erro = await page.locator('[role=dialog] .dx-alert').allTextContents()
      ok('guardar o PNG na biblioteca da organização', !!aviso, erro.join(' | '))
    }
  }

  // Exemplos novos.
  for (const [notacao, variante] of [['arch', 'c4'], ['arch', 'cloud'], ['flow', '1']]) {
    await page.goto(`${BASE}/#/whiteboards/diagram?tipo=${notacao}&exemplo=${variante}`)
    await page.waitForSelector('[data-node-id]', { timeout: 30000 })
    await page.waitForTimeout(1200)
    const estado = await page.locator('.dg-status').innerText()
    ok(`exemplo ${notacao}/${variante} abre válido`, (await page.locator('.dg-chip.is-ok').count()) === 1, estado.replace(/\n/g, ' · '))
    if (dir) await page.screenshot({ path: `${dir}/exemplo-${notacao}-${variante}.png` })
  }

  ok('sem erros de página', erros.length === 0, erros.join(' | '))
  if (STATE) await ctx.storageState({ path: STATE })
  await ctx.close()
}

await browser.close()
console.log(falhas ? `\n${falhas} falha(s)` : '\ntudo ok')
process.exit(falhas ? 1 : 0)
