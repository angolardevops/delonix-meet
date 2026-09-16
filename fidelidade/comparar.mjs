// Cópia do notas-ui-template/comparar.mjs para o ramo frontend/l1-diagramas:
// escreve em fidelidade/{app,lado-a-lado} (não nas pastas partilhadas), entra
// UMA vez e reutiliza a sessão (o servidor de validação limita logins), e tem
// as rotas reais deste ramo.
//
//   node fidelidade/comparar.mjs --app http://127.0.0.1:5505 [Doc...] [--mobile]
import { chromium } from '@playwright/test'
import { readFileSync, existsSync } from 'node:fs'
const DIR = new URL('./', import.meta.url).pathname
const REF = new URL('../../notas-ui-template/ref/', import.meta.url).pathname
const args = process.argv.slice(2)
const appIdx = args.indexOf('--app')
const APP = appIdx >= 0 ? args.splice(appIdx, 2)[1] : 'http://127.0.0.1:5505'
const mobIdx = args.indexOf('--mobile')
const MOBILE = mobIdx >= 0 ? (args.splice(mobIdx, 1), true) : false
const USER = process.env.DX_USER ?? 'demo@delonix.co.ao'
const PASS = process.env.DX_PASS ?? 'demo12345'
const STATE = `${DIR}.sessao.json`

const ROTAS = {
  DelonixCanvasUML: { hash: '/whiteboards/diagram?tipo=uml&exemplo=1' },
  DelonixCanvasBPMN: { hash: '/whiteboards/diagram?tipo=bpmn&exemplo=1' },
  DelonixPlayer: { player: true },
}
const want = args.length ? args : Object.keys(ROTAS)
const size = MOBILE ? { width: 390, height: 844 } : { width: 1440, height: 900 }

const b = await chromium.launch()
const ctx = await b.newContext({ viewport: size, storageState: existsSync(STATE) ? STATE : undefined })
const p = await ctx.newPage()
p.on('pageerror', (e) => console.log('pageerror', e.message))

async function entrar() {
  if (process.env.FAKE) {
    // Sem servidor: sessão falsa. Prova layout e estados vazios/erro, não dados.
    await ctx.addInitScript(() => {
      localStorage.setItem('dx_user', JSON.stringify({ id: '00000000-0000-0000-0000-000000000001', email: 'demo@delonix.co.ao', username: 'Demo' }))
      localStorage.setItem('dx_access', 'falso')
      localStorage.setItem('dx_tour_v1', 'done')
    })
    return
  }
  await p.goto(`${APP}/#/`)
  await p.waitForTimeout(1500)
  if (await p.locator('.shell').count()) return
  await p.goto(`${APP}/#/login`)
  await p.fill('[data-testid=auth-email]', USER)
  await p.fill('[data-testid=auth-password]', PASS)
  await p.press('[data-testid=auth-password]', 'Enter')
  await p.waitForSelector('.shell', { timeout: 30000 })
  await ctx.storageState({ path: STATE })
}

await entrar()
for (const doc of want) {
  const r = ROTAS[doc]
  if (!r) { console.log('sem rota', doc); continue }
  let hash = r.hash
  if (r.player) {
    const lib = await p.evaluate(async () => (await fetch('/api/recordings', { headers: { Authorization: `Bearer ${localStorage.getItem('dx_access')}` } })).json())
    const rec = Array.isArray(lib) ? lib.find((x) => x.status !== 'failed') : null
    if (!rec) { console.log('sem gravações na biblioteca', JSON.stringify(lib).slice(0, 200)); continue }
    hash = `/recordings/${rec.id}`
  }
  await p.goto(`${APP}/#${hash}`)
  await p.waitForTimeout(r.player ? 6000 : 2500)
  const suf = MOBILE ? '-390' : ''
  await p.screenshot({ path: `${DIR}app/${doc}${suf}.png` })
  const ref = `${REF}${doc}.png`
  if (MOBILE || !existsSync(ref)) { console.log('ok (só app)', doc); continue }
  const img = (f) => 'data:image/png;base64,' + readFileSync(f).toString('base64')
  const cmp = await b.newPage({ viewport: { width: 2896, height: 940 } })
  await cmp.setContent(`<body style="margin:0;background:#222;font:12px monospace;color:#ccc;display:flex;gap:16px;padding:0">
    <div><div style="height:20px;padding:2px 6px">TEMPLATE · ${doc}</div><img src="${img(ref)}" width="1440" height="900"></div>
    <div><div style="height:20px;padding:2px 6px">APP · ${APP}</div><img src="${img(`${DIR}app/${doc}.png`)}" width="1440" height="900"></div></body>`)
  await cmp.screenshot({ path: `${DIR}lado-a-lado/${doc}.png` })
  await cmp.close()
  console.log('ok', doc)
}
await b.close()
