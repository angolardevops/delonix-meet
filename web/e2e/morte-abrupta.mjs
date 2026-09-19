// O pod morre sem aviso. Os participantes voltam sozinhos?
//
// NÃO CORRE NO CI: a recuperação que ele mede é INCONSTANTE — quatro corridas
// locais contra o mesmo commit deram falha, êxito, e duas sem terminar no
// prazo. Pô-lo a bloquear merges seria um portão que falha ao acaso, e um
// portão desses perde a credibilidade toda (R62). Corre-se à mão enquanto a
// causa da inconstância não estiver estabelecida:
//
//   DELONIX_ALLOW_INSECURE=1 BIND_ADDR=0.0.0.0:8186 API=http://127.0.0.1:8186 \
//     APP=http://localhost:5186 SERVER_BIN=<caminho absoluto do binário> \
//     SERVER_CWD=<dir do servidor> MORTO_MS=6000 node web/e2e/morte-abrupta.mjs
//
// O `drain.mjs` cobre o encerramento ORDENADO — SIGTERM, aviso `Draining`,
// prazo para migrar. Isto é o outro caso: SIGKILL, sem aviso nenhum, que é o
// que acontece num OOM, num nó que se despenha ou num `docker kill`.
//
// O que já ficou PROVADO com ele, e motivou a correcção no `Room.tsx`: antes,
// um corte de seis segundos deixava toda a gente na reunião presa em
// «Erro: Internal Server Error», sem retorno mesmo depois de o servidor voltar
// — o `catch` do arranque da sala pintava a mensagem técnica e parava ali.
//
// Duas asserções que ele já teve e que passavam em VAZIO, aqui para não
// voltarem: «a página recarregou» (a recuperação por nova tentativa não
// recarrega) e «a sala está aberta com um retrato» (o DOM não muda quando o
// socket cai). A prova que não engana é FUNCIONAL: entra outra pessoa e o
// anfitrião tem de a ver, admitir e passar a vê-la.
import { chromium } from '@playwright/test'
import { execFileSync, spawn } from 'node:child_process'
import { criarConta, entrar } from './sessao.mjs'

const API = process.env.API ?? 'http://127.0.0.1:8180'
const APP = process.env.APP ?? 'http://localhost:5174'
const BIN = process.env.SERVER_BIN
const MORTO_MS = Number(process.env.MORTO_MS ?? 6000)
if (!BIN) {
  console.error('falta SERVER_BIN — o caminho do binário do servidor a ressuscitar')
  process.exit(2)
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
let falhas = 0
const ok = (c, n, d) => {
  console.log(`  ${c ? '✓' : '✗'} ${n}${d ? `  — ${d}` : ''}`)
  if (!c) falhas++
}
const vivo = () =>
  fetch(`${API}/api/status`).then((r) => r.ok).catch(() => false)

// ---------------------------------------------------------- uma chamada real
const conta = await criarConta(API, 'morte')
const browser = await chromium.launch({
  args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'],
})
const ctx = { ignoreHTTPSErrors: true, permissions: ['camera', 'microphone'] }
const page = await (await browser.newContext(ctx)).newPage()
await entrar(page, APP, conta)
await page.getByRole('button', { name: /nova reuni/i }).first().click()
await page.waitForFunction(() => /^#\/r\/[a-z-]+$/.test(location.hash), null, { timeout: 60000 })
const rota = await page.evaluate(() => location.hash)
await page.getByRole('button', { name: /entrar agora/i }).first().click({ timeout: 60000 })
const entrou = await page
  .waitForFunction(() => !/Pronto para entrar/i.test(document.body.innerText || ''), null, { timeout: 90000 })
  .then(() => true)
  .catch(() => false)
ok(entrou, 'chamada estabelecida antes da morte', entrou ? `rota ${rota}` : 'não entrou')
if (!entrou) {
  await browser.close()
  process.exit(1)
}

// Marcador que só um RECARREGAMENTO apaga. Sem ele, a asserção de recuperação
// passa em vazio: o DOM não muda quando o socket cai, por isso «a sala está
// aberta e tem retratos» continua verdadeiro com o servidor morto. Mediu-se
// «0,0s de recuperação» antes de isto existir.
await page.evaluate(() => {
  window.__antesDaMorte = true
})

// ------------------------------------------------------------------ SIGKILL
// `pgrep -f` compara com a linha de COMANDO, que aqui é relativa
// (`./target/release/delonix-server`); o caminho absoluto não casa. Procura-se
// pelo executável a sério, em `/proc/<pid>/exe`.
const pid = Number(
  execFileSync('bash', [
    '-lc',
    `for d in /proc/[0-9]*; do [ "$(readlink $d/exe 2>/dev/null)" = "${BIN}" ] && basename $d && break; done`,
  ])
    .toString()
    .trim(),
)
ok(Number.isFinite(pid) && pid > 0, 'servidor encontrado para matar', `pid ${pid}`)
console.log(`  · SIGKILL ao pid ${pid} — sem aviso, sem drain`)
process.kill(pid, 'SIGKILL')
for (let k = 0; k < 20 && (await vivo()); k++) await sleep(250)
ok(!(await vivo()), 'o servidor está MESMO morto')

// O que o cliente faz DURANTE a morte. Sem isto não se distingue «o socket não
// fechou» de «fechou e o cliente não reagiu» — duas avarias em sítios opostos.
for (let k = 0; k < Math.ceil(MORTO_MS / 1000); k++) {
  await sleep(1000)
  const t = await page
    .evaluate(() => (document.body.innerText || '').replace(/\s+/g, ' ').slice(0, 90))
    .catch(() => '<página a recarregar>')
  console.log(`  · t+${k + 1}s: ${t}`)
}
const filho = spawn(BIN, [], {
  cwd: process.env.SERVER_CWD ?? '.',
  env: { ...process.env },
  detached: true,
  stdio: 'ignore',
})
filho.unref()
for (let k = 0; k < 60 && !(await vivo()); k++) await sleep(500)
ok(await vivo(), `o servidor voltou depois de ${MORTO_MS} ms morto`)

// ------------------------------------------ a sinalização voltou a servir?
//
// «A página recarregou» era a asserção errada: a recuperação por nova tentativa
// NÃO recarrega, e exigi-lo dava falha com o produto já correcto. E «a sala está
// aberta com um retrato» passa em vazio — o DOM não muda quando o socket cai.
//
// A prova que não engana é FUNCIONAL: entra uma segunda pessoa por código, e o
// anfitrião tem de a ver na sala de espera, admiti-la, e passar a vê-la como
// retrato remoto. Nada disso acontece se a sinalização dele não tiver voltado.
const codigo = rota.replace('#/r/', '')
const conta2 = await criarConta(API, 'mort2')
const page2 = await (await browser.newContext(ctx)).newPage()
await entrar(page2, APP, conta2)
await page2.goto(`${APP}/#/r/${codigo}`, { waitUntil: 'domcontentloaded' })
await page2.getByRole('button', { name: /entrar agora/i }).first().click({ timeout: 60000 })

const pilula = page.locator('.waiting-pill')
const viuPedido = await pilula
  .waitFor({ timeout: 90000 })
  .then(() => true)
  .catch(() => false)
ok(
  viuPedido,
  `depois de ${MORTO_MS} ms de nó morto, o anfitrião RECEBE o pedido de entrada`,
  viuPedido ? 'sinalização viva' : 'não recebeu — a sinalização não recuperou',
)

if (viuPedido) {
  await page.locator('.admit-accept').first().click({ timeout: 30000 }).catch(() => {})
  const juntou = await page
    .waitForFunction(
      () =>
        [...document.querySelectorAll('.tile')].filter((t) => !/\\beu\\b/i.test(t.textContent || '')).length === 1,
      null,
      { timeout: 90000 },
    )
    .then(() => true)
    .catch(() => false)
  ok(juntou, 'e a reunião volta a funcionar de facto — o convidado aparece')
}

const texto = (await page.evaluate(() => (document.body.innerText || '').replace(/\s+/g, ' ').slice(0, 160))) ?? ''
ok(!/Internal Server Error|\bErro:/i.test(texto), 'e não fica mensagem técnica no ecrã', texto.slice(0, 90))

await browser.close()
console.log(`\n=== ${falhas === 0 ? 'TODAS PASSARAM' : `${falhas} FALHARAM`} ===`)
process.exit(falhas ? 1 : 0)
