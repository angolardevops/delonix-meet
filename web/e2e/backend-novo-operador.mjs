// Armazenamento da plataforma (operador) pela interface, contra `/api/operator/v1/storage*`.
//
// Precisa de uma conta que o servidor declare operadora (`PLATFORM_ADMIN_USER_IDS`)
// — por isso recebe a conta já criada:
//   EMAIL=… API=http://127.0.0.1:8460 APP=http://127.0.0.1:5460 node e2e/backend-novo-operador.mjs
import { chromium } from '@playwright/test'
import { entrar, PASSWORD } from './sessao.mjs'

const APP = process.env.APP ?? 'http://127.0.0.1:5460'
const EMAIL = process.env.EMAIL
if (!EMAIL) throw new Error('EMAIL em falta')
let falhas = 0
const ok = (c, n, d) => {
  console.log(`  ${c ? '✓' : '✗'} ${n}${d ? `  — ${d}` : ''}`)
  if (!c) falhas++
}
const browser = await chromium.launch()
const page = await (await browser.newContext({ locale: 'pt-PT', acceptDownloads: true, viewport: { width: 1440, height: 900 } })).newPage()
const pedidos = []
page.on('response', (r) => {
  const u = new URL(r.url())
  if (u.pathname.startsWith('/api/operator/')) pedidos.push(`${r.request().method()} ${u.pathname} ${r.status()}`)
})
await entrar(page, APP, { email: EMAIL, password: PASSWORD })
await page.goto(`${APP}/#/integrations`, { waitUntil: 'domcontentloaded' })
await page.getByRole('button', { name: 'Testar ligação' }).waitFor({ timeout: 30000 })
ok(pedidos.includes('GET /api/operator/v1/storage 200'), 'lê a configuração → GET operator/v1/storage 200', pedidos.join(' | '))
const cartao = page.locator('.dx-card, section').filter({ has: page.getByRole('button', { name: 'Testar ligação' }) }).last()
await cartao.getByRole('button', { name: 'Guardar' }).click()
await page.waitForTimeout(1500)
ok(pedidos.includes('PUT /api/operator/v1/storage 200'), 'guardar → PUT operator/v1/storage 200 (devolve o recurso)')
await page.getByRole('button', { name: 'Testar ligação' }).click()
await page.waitForTimeout(2000)
ok(pedidos.some((p) => p.startsWith('POST /api/operator/v1/storage/test 200')), 'testar → POST operator/v1/storage/test 200')
const manifesto = page.getByRole('button', { name: 'Descarregar manifesto do volume' })
if (await manifesto.count()) {
  const [dl] = await Promise.all([page.waitForEvent('download', { timeout: 15000 }).catch(() => null), manifesto.click()])
  ok(!!dl && pedidos.some((p) => p.startsWith('GET /api/operator/v1/storage/pvc-manifest 200')), 'manifesto → GET operator/v1/storage/pvc-manifest 200 e download', dl?.suggestedFilename())
} else {
  console.log('  · botão do manifesto não visível neste tipo de armazenamento')
}
console.log(`  · pedidos: ${pedidos.join(' | ')}`)
await browser.close()
console.log(falhas ? `\n=== ${falhas} FALHARAM ===` : '\n=== TUDO VERDE ===')
process.exit(falhas ? 1 : 0)
