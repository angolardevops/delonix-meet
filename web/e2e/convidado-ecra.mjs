// Um CONVIDADO SEM CONTA entra numa reunião PELA INTERFACE — o link, um nome,
// a sala de espera, e o anfitrião admite.
//
// O `convidado.mjs` prova o servidor (API e sinalização, sem ecrã); o
// `reuniao.mjs` prova a sala com duas CONTAS. Faltava o caminho que decide a
// adopção: alguém de fora, sem conta, a conseguir entrar a partir de um link.
// Durante meses o servidor teve a rota e nenhum ecrã a chamava — quem abria o
// link caía no login.
//
// O que se mede, por ordem:
//   1. sem sessão, o link de sala mostra a entrada de convidado — não o login;
//   2. um nome e «pedir para entrar» levam à pré-entrada e, entrando, à SALA DE
//      ESPERA (nunca directo à sala);
//   3. o anfitrião vê o pedido, admite, e passam a ver-se um ao outro — com o
//      NOME que o convidado escreveu no retrato dele;
//   4. o convidado não tem o que é da conta: nem gravar, nem convidar;
//   5. um F5 a meio devolve-o à sala SEM nova espera (o bilhete e o lugar
//      sobrevivem à recarga);
//   6. sair devolve-o ao ecrã de convidado, não a uma consola que ele não tem;
//   7. controlo negativo: uma sala que não aceita convidados diz-lho e manda-o
//      iniciar sessão; um código que não existe diz que não existe.
//
// O QUE NÃO PROVA: a qualidade da media (a câmara é a falsa do Chromium), a
// sala com cifra ponta-a-ponta, o telemóvel, e a renovação do bilhete ao fim
// de cinco minutos (está em `convidado.test.ts`, com relógio fingido).
//
// Uso:  APP=http://localhost:5174 API=http://127.0.0.1:8180 node web/e2e/convidado-ecra.mjs
import { chromium } from '@playwright/test'
import { criarConta, entrar } from './sessao.mjs'

const API = process.env.API ?? 'http://127.0.0.1:8180'
const APP = process.env.APP ?? 'http://localhost:5174'
const NOME = 'Marta Convidada'
let falhas = 0
const ok = (c, n, d) => {
  console.log(`  ${c ? '✓' : '✗'} ${n}${d ? `  — ${d}` : ''}`)
  if (!c) falhas++
}

const browser = await chromium.launch({
  args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'],
})
const novoContexto = () => browser.newContext({ locale: 'pt-PT', ignoreHTTPSErrors: true, permissions: ['camera', 'microphone'] })

// ---- o anfitrião: conta, reunião, dentro da sala ---------------------------
const conta = await criarConta(API, 'anf')
const anfitriao = await (await novoContexto()).newPage()
await entrar(anfitriao, APP, conta)
await anfitriao.getByRole('button', { name: /iniciar agora/i }).first().click()
await anfitriao.waitForFunction(() => /^#\/r\/[a-z-]+$/.test(location.hash), null, { timeout: 60000 })
const codigo = (await anfitriao.evaluate(() => location.hash)).replace('#/r/', '')
await anfitriao.getByRole('button', { name: /entrar na sessão/i }).first().click({ timeout: 60000 })
await anfitriao.waitForFunction(() => !document.querySelector('.rm-prejoin'), null, { timeout: 90000 })
// O nome com que o anfitrião aparece nos retratos é o `username` da conta.
const nomeDoAnfitriao = await anfitriao.evaluate(() => JSON.parse(localStorage.getItem('dx_user')).username)
console.log(`  · sala ${codigo}, anfitrião «${nomeDoAnfitriao}» dentro`)

// ---- 1. o convidado abre o link SEM sessão ---------------------------------
const convidado = await (await novoContexto()).newPage()
const pedidosComSessao = []
convidado.on('request', (r) => {
  if (r.url().includes('/api/') && r.headers()['authorization']) pedidosComSessao.push(r.url())
})
await convidado.goto(`${APP}/#/r/${codigo}`, { waitUntil: 'domcontentloaded', timeout: 120000 })
const entrada = convidado.locator('[data-testid=convidado-entrada]')
await entrada.waitFor({ timeout: 60000 }).catch(() => {})
ok(await entrada.isVisible().catch(() => false), 'sem sessão, o link mostra a entrada de CONVIDADO')
ok((await convidado.locator('[data-testid=auth-email]').count()) === 0, 'e não o formulário de login')
ok((await entrada.textContent().catch(() => ''))?.includes(codigo), 'com o código da reunião à vista')

// Sem nome o botão não deixa pedir.
ok(await convidado.locator('[data-testid=convidado-entrar]').isDisabled(), 'sem nome, «pedir para entrar» está desligado')

// ---- 2. nome → pré-entrada → sala de espera --------------------------------
await convidado.fill('#convidado-nome', NOME)
await convidado.locator('[data-testid=convidado-entrar]').click()
await convidado.getByRole('button', { name: /entrar na sessão/i }).first().click({ timeout: 60000 })
const emEspera = await convidado
  .locator('.rm-waiting')
  .waitFor({ timeout: 60000 })
  .then(() => true)
  .catch(() => false)
ok(emEspera, 'o convidado fica na SALA DE ESPERA — não entra sozinho')

// ---- 3. o anfitrião admite, e vêem-se --------------------------------------
const pilula = anfitriao.locator('.rm-occupancy__waiting')
await pilula.waitFor({ timeout: 60000 }).catch(() => {})
ok(await pilula.isVisible().catch(() => false), 'o anfitrião VÊ o pedido de entrada')
const cartao = anfitriao.locator('.rm-admit-accept').first()
await cartao.waitFor({ timeout: 30000 }).catch(() => {})
const textoDaEspera = (await anfitriao.locator('body').innerText().catch(() => '')).replace(/\s+/g, ' ')
ok(textoDaEspera.includes(NOME), 'com o nome que o convidado escreveu')
await cartao.click().catch(() => {})

// Dois retratos, e o remoto é QUEM se espera. Sem o nome, isto dava-se por
// satisfeito num instante transitório: logo a seguir a um F5 a lista traz, por
// um momento, o lugar reservado da própria pessoa como se fosse outra — e
// «dois retratos, um remoto» passava a medir o convidado a ver-se a si.
const doisRetratos = (pagina, remotoContem) =>
  pagina
    .waitForFunction(
      (nome) => {
        const tiles = [...document.querySelectorAll('.rm-tile')]
        const remotos = tiles.filter((t) => t.getAttribute('data-peer') === 'remoto')
        if (tiles.length !== 2 || remotos.length !== 1) return null
        if (!(remotos[0].textContent || '').includes(nome)) return null
        return { nomes: tiles.map((t) => (t.textContent || '').trim().slice(0, 40)), comVideo: remotos[0].querySelector('video') !== null }
      },
      remotoContem,
      { timeout: 90000 },
    )
    .then((h) => h.jsonValue())
    .catch(() => null)

const vistoPeloAnfitriao = await doisRetratos(anfitriao, NOME)
ok(!!vistoPeloAnfitriao, 'o anfitrião vê o convidado na sala, com o nome que ele escreveu', JSON.stringify(vistoPeloAnfitriao))
const vistoPeloConvidado = await doisRetratos(convidado, nomeDoAnfitriao)
ok(!!vistoPeloConvidado, 'o convidado vê o ANFITRIÃO (1 retrato remoto, o dele)', JSON.stringify(vistoPeloConvidado))
ok(!(await convidado.locator('.rm-waiting').isVisible().catch(() => false)), 'e saiu da sala de espera')

// ---- 4. o que é da conta não está lá ---------------------------------------
const rotulos = await convidado.evaluate(() => [...document.querySelectorAll('button')].map((b) => (b.getAttribute('aria-label') || b.textContent || '').trim().toLowerCase()))
ok(!rotulos.some((r) => r === 'gravar' || r.startsWith('gravar ')), 'o convidado não tem o botão de gravar')
const rotulosDoAnfitriao = await anfitriao.evaluate(() => [...document.querySelectorAll('button')].map((b) => (b.getAttribute('aria-label') || b.textContent || '').trim().toLowerCase()))
ok(rotulosDoAnfitriao.some((r) => r === 'gravar' || r.startsWith('gravar ')), 'controlo: o anfitrião TEM o botão de gravar', 'se isto falhar, a asserção de cima mede nada')
ok(pedidosComSessao.length === 0, 'nenhum pedido do convidado levou credenciais de conta', pedidosComSessao.slice(0, 2).join(' '))
ok((await convidado.evaluate(() => localStorage.getItem('dx_user'))) === null, 'e o browser dele não ficou com sessão nenhuma')

// ---- 5. F5: volta à sala sem nova espera -----------------------------------
await convidado.reload({ waitUntil: 'domcontentloaded' })
const deVolta = await doisRetratos(convidado, nomeDoAnfitriao)
ok(!!deVolta, 'depois de recarregar, o convidado está de volta à sala, com o anfitrião', JSON.stringify(deVolta))
ok(!(await convidado.locator('.rm-waiting').isVisible().catch(() => false)), 'sem passar outra vez pela sala de espera')
const anfitriaoDepois = await doisRetratos(anfitriao, NOME)
ok(!!anfitriaoDepois, 'e o anfitrião continua a vê-lo — um só, não ele e o seu fantasma', JSON.stringify(anfitriaoDepois))
ok(!(await anfitriao.locator('.rm-admit-accept').first().isVisible().catch(() => false)), 'sem voltar a pedir admissão ao anfitrião')

// ---- 6. sair devolve-o ao ecrã de convidado --------------------------------
await convidado.getByRole('button', { name: /^sair/i }).first().click({ timeout: 30000 }).catch(() => {})
// A saída pode pedir confirmação (um diálogo do kit): aceita-se, se aparecer.
await convidado.getByRole('button', { name: /^sair/i }).last().click({ timeout: 3000 }).catch(() => {})
const saiu = await convidado
  .locator('[data-testid=convidado-saiu]')
  .waitFor({ timeout: 30000 })
  .then(() => true)
  .catch(() => false)
ok(saiu, 'ao sair, o convidado vê «saíste da reunião» — não o login nem a consola')
ok((await convidado.evaluate((c) => sessionStorage.getItem(`dx_convidado_${c}`), codigo)) === null, 'e o bilhete foi esquecido')

// ---- 7. controlos negativos -------------------------------------------------
// Uma sala que NÃO aceita convidados: o servidor recusa e o ecrã manda iniciar sessão.
const fechar = await anfitriao.evaluate(async (c) => {
  const r = await fetch(`/api/rooms/${c}`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${localStorage.getItem('dx_access')}` },
    body: JSON.stringify({ allow_guests: false }),
  })
  return r.status
}, codigo)
ok(fechar === 200, 'o anfitrião fecha a sala a convidados sem conta', `PATCH → ${fechar}`)
const outro = await (await novoContexto()).newPage()
await outro.goto(`${APP}/#/r/${codigo}`, { waitUntil: 'domcontentloaded', timeout: 120000 })
await outro.fill('#convidado-nome', 'Outro Convidado')
await outro.locator('[data-testid=convidado-entrar]').click()
const erroFechada = outro.locator('[data-testid=convidado-erro]')
await erroFechada.waitFor({ timeout: 30000 }).catch(() => {})
ok(/conta/i.test((await erroFechada.textContent().catch(() => '')) ?? ''), 'sala fechada a convidados: o ecrã diz que é preciso conta')
ok((await outro.locator('.rm-prejoin').count()) === 0, 'e não avança para a sala')
await outro.locator('[data-testid=convidado-tenho-conta]').click()
await outro.waitForSelector('[data-testid=auth-email]', { timeout: 30000 }).catch(() => {})
ok((await outro.locator('[data-testid=auth-pendente]').textContent().catch(() => ''))?.includes(codigo) ?? false, '«tenho conta» leva ao login com a sala como destino')

// Um código que não existe.
const perdido = await (await novoContexto()).newPage()
await perdido.goto(`${APP}/#/r/zzz-zzzz-zzz`, { waitUntil: 'domcontentloaded', timeout: 120000 })
await perdido.fill('#convidado-nome', 'Perdido')
await perdido.locator('[data-testid=convidado-entrar]').click()
const erroPerdido = perdido.locator('[data-testid=convidado-erro]')
await erroPerdido.waitFor({ timeout: 30000 }).catch(() => {})
ok(/não encontr/i.test((await erroPerdido.textContent().catch(() => '')) ?? ''), 'código que não existe: o ecrã diz que não encontrou a reunião')

await browser.close()
console.log(falhas ? `\n=== ${falhas} FALHARAM ===` : '\n=== tudo passou ===')
process.exit(falhas ? 1 : 0)
