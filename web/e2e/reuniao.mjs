// Duas pessoas encontram-se MESMO — o caminho completo, pela interface real.
//
// Criar reunião → pré-entrada → entrar → o convidado entra por código → sala de
// espera → o anfitrião admite → o convidado aparece como retrato remoto. É a
// promessa central do produto e não havia teste nenhum a cobri-la: o
// `isolamento.mjs` é sobre inquilinos, o `tempos.mjs` usa o arnês em vez da
// consola, e o `layout-consola.mjs` é sobre disposição.
//
// Três armadilhas que este teste encontrou e que ficam aqui escritas, porque
// cada uma dele fez passar em VAZIO durante uma iteração:
//
//   1. A rota é `#/r/<código>`, não `#/room/<código>`. A rota errada fica na
//      consola e «zero remotos» passa por não haver sala nenhuma.
//   2. A sala abre num ecrã de PRÉ-ENTRADA que já mostra a câmara local — e
//      isso vale tanto para quem cria como para quem entra por código. Esperar
//      por `<video>` dá-se por satisfeito aí. O sinal de entrada é a
//      pré-entrada DESAPARECER.
//   3. `.tile` cobre o retrato LOCAL e os remotos. Contá-los e chamar-lhes
//      «remotos» é medir a própria pessoa.
//
import { chromium } from '@playwright/test'
import { criarConta, entrar } from './sessao.mjs'

const API = process.env.API ?? 'http://127.0.0.1:8180'
const APP = process.env.APP ?? 'http://localhost:5174'
let falhas = 0
const ok = (c, n, d) => {
  console.log(`  ${c ? '✓' : '✗'} ${n}${d ? `  — ${d}` : ''}`)
  if (!c) falhas++
}

const conta = await criarConta(API, 'fant')
const browser = await chromium.launch({
  args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'],
})
const page = await (await browser.newContext({ ignoreHTTPSErrors: true, permissions: ['camera', 'microphone'] })).newPage()
await entrar(page, APP, conta)

// Cria a reunião pelo CAMINHO DO PRODUTO — o botão que um utilizador carrega.
// A primeira versão chamava a API com `conta.token`, que o `sessao.mjs` não
// devolve: a sala nunca era criada, a rota ficava `#/r/undefined`, e o vídeo
// LOCAL fazia a asserção passar na mesma. Zero retratos remotos numa sala que
// não existe não prova nada.
await page.getByRole('button', { name: /nova reuni/i }).first().click()
await page.waitForFunction(() => /^#\/r\/[a-z-]+$/.test(location.hash), null, { timeout: 60000 })
const rota = await page.evaluate(() => location.hash)

// A sala abre num ecrã de PRÉ-ENTRADA, e ele já mostra a câmara local. Esperar
// por `<video>` dá-se por satisfeito aí — foi assim que este teste passou três
// vezes sem nunca ter entrado em sala nenhuma. O sinal de entrada é a
// pré-entrada DESAPARECER.
await page.getByRole('button', { name: /entrar agora/i }).first().click({ timeout: 60000 })
const entrou = await page
  .waitForFunction(() => !/Pronto para entrar/i.test(document.body.innerText || ''), null, { timeout: 90000 })
  .then(() => true)
  .catch(() => false)
ok(entrou, 'a sala ABRIU MESMO — a pré-entrada desapareceu', entrou ? `rota ${rota}` : 'ficou na pré-entrada')
if (!entrou) {
  await browser.close()
  console.log('\n=== 1 FALHARAM ===')
  process.exit(1)
}
await page.waitForTimeout(4000)

const visto = await page.evaluate(() => ({
  tiles: document.querySelectorAll('.tile').length,
  nomes: [...document.querySelectorAll('.tile')].map((t) => (t.textContent || '').trim().slice(0, 24)),
  videos: document.querySelectorAll('video').length,
}))
console.log(`  · observado: ${JSON.stringify(visto)}`)
// `.tile` cobre o retrato LOCAL e os remotos — o de quem está sozinho diz
// «eu». Contar `.tile` e chamar-lhe «remotos» foi o que fez este teste dar
// verde a medir a própria pessoa.
const remotosSozinho = visto.nomes.filter((n) => !/\beu\b/i.test(n)).length
ok(
  visto.tiles === 1 && remotosSozinho === 0,
  'sozinho: vê-se a si (1 retrato, «eu») e NENHUM remoto',
  `tiles=${visto.tiles} remotos=${remotosSozinho} ${JSON.stringify(visto.nomes)}`,
)

// E agora a metade que impede ESTE teste de passar em vazio: se o selector não
// visse um remoto de verdade, a asserção de cima estaria a medir nada. Entra uma
// segunda pessoa e a contagem TEM de subir para 1 — nem 0 (selector cego) nem 2
// (o fantasma que se foi procurar).
const codigo = rota.replace('#/r/', '')
const conta2 = await criarConta(API, 'fan2')
const page2 = await (await browser.newContext({ ignoreHTTPSErrors: true, permissions: ['camera', 'microphone'] })).newPage()
await entrar(page2, APP, conta2)
await page2.goto(`${APP}/#/r/${codigo}`, { waitUntil: 'domcontentloaded' })
// A sala abre num ecrã de PRÉ-ENTRADA («Pronto para entrar?»). Quem entra por
// código passa por ele; quem cria a reunião não. Sem carregar aqui, a segunda
// pessoa fica parada a ver a própria câmara e o teste conclui, erradamente,
// que ela entrou.
await page2.getByRole('button', { name: /entrar agora/i }).first().click({ timeout: 60000 })
await page2.waitForTimeout(6000)
console.log('  · página 2 depois de entrar:', JSON.stringify(await page2.evaluate(() => ({
  videos: document.querySelectorAll('video').length,
  tiles: document.querySelectorAll('.tile').length,
  texto: (document.body.innerText || '').replace(/\s+/g, ' ').slice(0, 160),
}))))
console.log('  · página 1:', JSON.stringify(await page.evaluate(() => ({
  tiles: document.querySelectorAll('.tile').length,
  texto: (document.body.innerText || '').replace(/\s+/g, ' ').slice(0, 160),
}))))

// A segunda pessoa é de OUTRA organização e cai na sala de espera — é a
// co-admissão, e é o que faz os quatro testes `#[ignore]` do hub estarem
// desactualizados. O anfitrião tem de a admitir, como faria um utilizador.
const pilula = page.locator('.waiting-pill')
await pilula.waitFor({ timeout: 60000 }).catch(() => {})
ok(await pilula.isVisible().catch(() => false), 'o anfitrião VÊ o aviso de convidado à espera')
await pilula.click().catch(() => {})
// Com UM convidado o controlo é o «Admitir» da linha; o «Admitir todos» só
// existe a partir de dois. O cartão da sala de espera já está visível — não é
// preciso abrir painel nenhum.
const admitir = page.locator('.admit-accept').first()
await admitir.waitFor({ timeout: 30000 }).catch(() => {})
ok(await admitir.isVisible().catch(() => false), 'o cartão da sala de espera oferece Admitir')
await admitir.click().catch(() => {})

const juntaram = await page
  .waitForFunction(
    () =>
      [...document.querySelectorAll('.tile')].filter((t) => !/\beu\b/i.test(t.textContent || '')).length === 1,
    null,
    { timeout: 90000 },
  )
  .then(() => true)
  .catch(() => false)
const finais = await page.evaluate(() =>
  [...document.querySelectorAll('.tile')].map((t) => (t.textContent || '').trim().slice(0, 20)),
)
ok(juntaram, 'admitido, o convidado aparece como retrato REMOTO no ecrã do anfitrião', JSON.stringify(finais))
ok(finais.length === 2, 'e a sala mostra dois retratos: eu e ele', `${finais.length}`)

await browser.close()
console.log(`\n=== ${falhas === 0 ? 'TODAS PASSARAM' : `${falhas} FALHARAM`} ===`)
process.exit(falhas ? 1 : 0)
