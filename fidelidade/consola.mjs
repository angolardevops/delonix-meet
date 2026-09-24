// Cópia do notas-ui-template/comparar.mjs para o ramo frontend/l2-consola
// (páginas da consola: Entrar, Início, Agenda, Chamadas, Integrações,
// Administração, Inteligência e idiomas). Escreve em fidelidade/consola/{app,lado-a-lado}
// (não nas pastas partilhadas), entra UMA vez por conta e reutiliza a sessão.
//
//   node fidelidade/consola.mjs --app http://127.0.0.1:5604 [Doc...] [--mobile]
//   DX_USER=ana.mbala@delonix.co.ao DX_PASS=... node fidelidade/consola.mjs DelonixAdmin
import { chromium } from '@playwright/test'
import { readFileSync, existsSync } from 'node:fs'
const DIR = new URL('./consola/', import.meta.url).pathname
const REF = new URL('../../notas-ui-template/ref/', import.meta.url).pathname
const args = process.argv.slice(2)
const appIdx = args.indexOf('--app')
const APP = appIdx >= 0 ? args.splice(appIdx, 2)[1] : 'http://127.0.0.1:5604'
const mobIdx = args.indexOf('--mobile')
const MOBILE = mobIdx >= 0 ? (args.splice(mobIdx, 1), true) : false
const USER = process.env.DX_USER ?? 'demo@delonix.co.ao'
const PASS = process.env.DX_PASS ?? 'demo12345'
const STATE = `${DIR}.sessao-${USER.replace(/[^a-z0-9]/gi, '_')}.json`

const ROTAS = {
  DelonixAuth: { hash: '/login', anon: true },
  DelonixHome: { hash: '/' },
  DelonixSchedule: { hash: '/calendar/new' },
  DelonixCall: { hash: '/directory' },
  DelonixIntegrations: { hash: '/integrations' },
  DelonixAdmin: { hash: '/admin' },
  DelonixAISettings: { hash: '/ai' },
}
const want = args.length ? args : Object.keys(ROTAS)
const size = MOBILE ? { width: 390, height: 844 } : { width: 1440, height: 900 }

const b = await chromium.launch()
const ctx = await b.newContext({ viewport: size, storageState: existsSync(STATE) ? STATE : undefined })
const p = await ctx.newPage()
p.on('pageerror', (e) => console.log('pageerror', e.message))

async function entrar() {
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

let entrou = false
for (const doc of want) {
  const r = ROTAS[doc]
  if (!r) { console.log('sem rota', doc); continue }
  let page = p
  if (r.anon) {
    const anon = await b.newContext({ viewport: size })
    page = await anon.newPage()
  } else if (!entrou) { await entrar(); entrou = true }
  await page.goto(`${APP}/#${r.hash}`)
  await page.waitForLoadState('networkidle', { timeout: 20000 }).catch(() => {})
  await page.waitForTimeout(2500)
  const suf = MOBILE ? '-390' : ''
  await page.screenshot({ path: `${DIR}app/${doc}${suf}.png`, fullPage: !!process.env.FULL })
  if (r.anon) await page.context().close()
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
