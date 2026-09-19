// Um F5 a meio da reunião devolve o mesmo lugar? (R91)
//
// É a prova do PRODUTO, não da biblioteca. Os testes de unidade do hub cobrem o
// segredo, a janela e as recusas; nada disso vale se o cliente não guardar o
// segredo, não o enviar, ou se o servidor o perder entre o socket velho e o
// novo.
//
// PORQUE O TESTE É SOBRE O CONVIDADO E NÃO SOBRE O ANFITRIÃO: a primeira
// versão fazia o F5 como anfitrião e passava com a reclamação DESLIGADA —
// verificado de propósito. É a armadilha do R69: quem cria a sala volta a
// entrar directamente de qualquer maneira, por isso «está dentro» e «não está
// na sala de espera» são verdade nos dois estados. O dano real é no convidado
// de outra organização: sem lugar reservado, o F5 devolve-o à SALA DE ESPERA e
// a reunião fica à espera que o anfitrião o admita outra vez.
//
//   APP=http://127.0.0.1:5180 API=http://127.0.0.1:8190 node e2e/reentrada.mjs
import { chromium } from '@playwright/test'
import { entrar, criarConta } from './sessao.mjs'

const APP = process.env.APP ?? 'http://localhost:5174'
const API = process.env.API ?? 'http://localhost:8180'
let falhas = 0
const ok = (cond, nome, detalhe = '') => {
  console.log(`  ${cond ? '✓' : '✗'} ${nome}${detalhe ? `  — ${detalhe}` : ''}`)
  if (!cond) falhas++
}

const browser = await chromium.launch({
  args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'],
})
const novaPagina = async () =>
  (await browser.newContext({ ignoreHTTPSErrors: true, permissions: ['camera', 'microphone'] })).newPage()

// ---- O anfitrião abre a sala ----
const page = await novaPagina()
await entrar(page, APP, await criarConta(API, 'reent'))
await page.getByRole('button', { name: /nova reuni/i }).first().click()
await page.waitForFunction(() => /^#\/r\/[a-z-]+$/.test(location.hash), null, { timeout: 60000 })
const codigo = await page.evaluate(() => location.hash.split('/r/')[1])
await page.getByRole('button', { name: /entrar agora/i }).first().click({ timeout: 60000 })

// ESPERAR pelo segredo, não lê-lo de imediato: a pré-entrada desaparece ao
// clicar, ANTES de o `joined` chegar. Ler nesse instante dava «nenhum» com o
// produto correcto.
const segredoAnfitriao = await page
  .waitForFunction((c) => sessionStorage.getItem(`dx_seat_${c}`), codigo, { timeout: 90000 })
  .then((h) => h.jsonValue())
  .catch(() => null)
ok(!!segredoAnfitriao, 'quem entra recebe e guarda o segredo do lugar',
   segredoAnfitriao ? `${segredoAnfitriao.slice(0, 8)}… (${segredoAnfitriao.length} chars)` : 'NENHUM em 90s')

// ---- O convidado entra pela sala de espera ----
const page2 = await novaPagina()
await entrar(page2, APP, await criarConta(API, 'reent2'))
await page2.goto(`${APP}/#/r/${codigo}`, { waitUntil: 'domcontentloaded' })
await page2.getByRole('button', { name: /entrar agora/i }).first().click({ timeout: 60000 })

const pilula = page.locator('.waiting-pill')
await pilula.waitFor({ timeout: 90000 }).catch(() => {})
await pilula.click().catch(() => {})
const admitir = page.locator('.admit-accept').first()
await admitir.waitFor({ timeout: 30000 }).catch(() => {})
await admitir.click().catch(() => {})

const entrou = await page2
  .waitForFunction(
    (c) => !!sessionStorage.getItem(`dx_seat_${c}`)
        && !/À espera que o anfitrião/i.test(document.body.innerText || ''),
    codigo,
    { timeout: 90000 },
  )
  .then(() => true)
  .catch(() => false)
ok(entrou, 'o convidado foi admitido e guardou o segredo do lugar')

// ---- O F5 do convidado: é aqui que se decide ----
await page2.reload({ waitUntil: 'domcontentloaded' })
// ESPERAR QUE ASSENTE antes de ler. A sala de espera só aparece depois de o
// servidor responder, e uma leitura no intervalo encontra a barra montada sem
// o aviso ainda renderizado — condição verdadeira nos DOIS estados, que é
// precisamente a armadilha do R69. Esta asserção passou com a reclamação
// desligada até este `waitForTimeout` existir.
await page2.locator('.controls-bar').waitFor({ timeout: 60000 }).catch(() => {})
await page2.waitForTimeout(8000)
const estado = await page2.evaluate(() => ({
  espera: /À espera que o anfitrião/i.test(document.body.innerText || ''),
  barra: document.querySelectorAll('.controls-bar').length,
  texto: (document.body.innerText || '').replace(/\s+/g, ' ').slice(0, 110),
}))
ok(!estado.espera && estado.barra === 1,
   'depois do F5 o CONVIDADO volta à sala SEM ser readmitido',
   estado.espera ? `caiu na SALA DE ESPERA: ${estado.texto}` : estado.texto)

// ---- Sair não é cair ----
// Sem esperar pelo botão, o clique cai em nada e o `.catch` engole-o: o teste
// reportava «o lugar não foi largado» quando na verdade nunca tinha carregado
// em sair. Um `catch` silencioso num PASSO (não numa asserção) transforma uma
// falha do teste numa falha do produto.
const sair = page2.locator('.ctrl.hangup')
await sair.waitFor({ state: 'visible', timeout: 60000 })
await sair.click()
const largou = await page2
  .waitForFunction((c) => sessionStorage.getItem(`dx_seat_${c}`) === null, codigo, { timeout: 30000 })
  .then(() => true)
  .catch(() => false)
const aoSair = await page2.evaluate((c) => sessionStorage.getItem(`dx_seat_${c}`), codigo)
ok(largou, 'ao SAIR de propósito o lugar é largado — sair não é cair',
   largou ? 'segredo apagado' : `ainda lá está: ${String(aoSair).slice(0, 12)}…`)

await browser.close()
console.log(`\n=== ${falhas === 0 ? 'TODAS PASSARAM' : `${falhas} FALHARAM`} ===`)
process.exit(falhas ? 1 : 0)
