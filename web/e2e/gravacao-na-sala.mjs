// O painel de gravações DENTRO da sala é mais uma vista da gravação, e tem a
// regra das outras (R59): não oferece nada sobre uma gravação sem ficheiro,
// nem o que o servidor vai recusar a quem está a ver.
//
// `GET /api/rooms/{code}/recordings` devolvia seis campos sem estado. Uma
// gravação a compor ou falhada aparecia no painel com «0.0 MB» e o botão de
// descarregar; o clique dava `409` ou `400`, e o ecrã «não foi possível
// descarregar». E o botão era oferecido a TODOS os participantes, quando o
// `?dl=1` só aceita o dono ou um administrador.
//
// Entra na sala pela interface real (não pelo arnês: o que se prova é o
// painel) e verifica:
//   1. as quatro linhas — a compor, falhada, pronta minha, pronta de outra
//      pessoa — e que só a pronta minha é um botão;
//   2. que o progresso e a passagem a «pronta» chegam sem recarregar — e que
//      só quem tem o painel aberto relê;
//   3. que uma gravação do servidor PARADA NA SALA entra na lista sozinha.
//      Antes só aparecia a quem voltasse a entrar. Sem ffmpeg (o CI) acaba
//      falhada, com a causa; com ffmpeg acaba pronta — o teste aceita as
//      duas, e em nenhuma há botão sem ficheiro.
//
// As três primeiras linhas entram por SQL, como o gravador as escreve: a
// janela real de `processing` é curta demais para um teste a apanhar parada.
//
// Uso: APP=http://localhost:5174 API=http://127.0.0.1:8180 node e2e/gravacao-na-sala.mjs
//      (a base: `PG=<contentor>`, ou `PG_EXEC` — ver `pg.mjs`). `FOTOS=<pasta>`
//      guarda uma captura de cada passo.
import { chromium } from '@playwright/test'
import { sql } from './pg.mjs'

const API = process.env.API ?? 'http://127.0.0.1:8180'
const APP = process.env.APP ?? 'http://localhost:5174'
const FOTOS = process.env.FOTOS ?? ''
const PW = 'UmaPasswordForte123!'
let falhas = 0
const chk = (c, n) => { console.log(`  ${c ? '✓' : '✗'} ${n}`); if (!c) falhas++ }
const j = (path, o = {}) =>
  fetch(`${API}${path}`, {
    method: o.method ?? 'GET',
    headers: { ...(o.token ? { Authorization: `Bearer ${o.token}` } : {}), ...(o.body ? { 'Content-Type': 'application/json' } : {}) },
    ...(o.body ? { body: JSON.stringify(o.body) } : {}),
  }).then(async (r) => ({ s: r.status, j: await r.json().catch(() => null) }))

async function conta(prefixo) {
  const m = Math.random().toString(36).slice(2, 7)
  const email = `${prefixo}${m}@${prefixo}${m}.local`
  await j('/api/auth/register', { method: 'POST', body: { org_name: `${prefixo} ${m}`, email, username: `${prefixo}${m}`, password: PW } })
  const tok = (await j('/api/auth/login', { method: 'POST', body: { email, password: PW } })).j.access_token
  return { email, tok, uid: sql(`SELECT id FROM users WHERE email = '${email}'`), m }
}

// Quem vê (anfitriã da sala) e outra pessoa, de outra organização, que também
// gravou nesta sala: a gravação dela vê-se, mas não é de quem vê descarregar.
const eu = await conta('gs')
const outra = await conta('go')
const sala = (await j('/api/rooms', { token: eu.tok, method: 'POST', body: { name: 'gravações na sala', topology: 'sfu' } })).j
chk(/^[0-9a-f-]{36}$/.test(eu.uid) && /^[0-9a-f-]{36}$/.test(outra.uid) && !!sala?.code, 'contas e sala criadas')

const nomes = { compor: `A compor ${eu.m}`, falhada: `Falhada ${eu.m}`, minha: `Minha ${eu.m}`, alheia: `Alheia ${eu.m}` }
const CAUSA = 'O disco encheu a meio da composição.'
// As colunas e os valores do `recorder::insert_processing`, com 40 % feitos.
const idCompor = sql(
  `INSERT INTO recordings (room_id, uploader_id, filename, size_bytes, status, progress_pct, progress_at, kind)
   VALUES ('${sala.id}', '${eu.uid}', '${nomes.compor}.webm', 0, 'processing', 40, now(), 'meeting') RETURNING id`,
)
sql(
  `INSERT INTO recordings (room_id, uploader_id, filename, size_bytes, status, failure_reason, created_at)
   VALUES ('${sala.id}', '${eu.uid}', '${nomes.falhada}.webm', 0, 'failed', '${CAUSA}', now() - interval '1 hour')`,
)
// Prontas, sem ficheiro em disco: aqui lê-se a lista, não se descarrega.
sql(
  `INSERT INTO recordings (room_id, uploader_id, filename, size_bytes, created_at)
   VALUES ('${sala.id}', '${eu.uid}', '${nomes.minha}.webm', 3145728, now() - interval '2 hours')`,
)
sql(
  `INSERT INTO recordings (room_id, uploader_id, filename, size_bytes, created_at)
   VALUES ('${sala.id}', '${outra.uid}', '${nomes.alheia}.webm', 3145728, now() - interval '3 hours')`,
)
chk(/^[0-9a-f-]{36}$/.test(idCompor), 'quatro gravações na sala: a compor, falhada, pronta minha, pronta de outra pessoa')

// ---------------------------------------------------------------------------
//  O ecrã: entrar na sala e abrir o painel
// ---------------------------------------------------------------------------
const b = await chromium.launch({ args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'] })
const ctx = await b.newContext({ locale: 'pt-PT', ignoreHTTPSErrors: true, viewport: { width: 1280, height: 800 }, permissions: ['camera', 'microphone'] })
const p = await ctx.newPage()
const foto = async (n) => { if (FOTOS) await p.screenshot({ path: `${FOTOS}/${n}.png` }) }
await p.goto(`${APP}/#/login`, { waitUntil: 'domcontentloaded', timeout: 120000 })
await p.waitForSelector('[data-testid=auth-email]', { timeout: 120000 })
await p.evaluate((k) => { localStorage.setItem('dx_tour_v1', 'done'); localStorage.setItem(k, JSON.stringify({ step: 0, done: true, off: true })) }, `dx_tour_home:${eu.uid}`)
await p.fill('[data-testid=auth-email]', eu.email)
await p.fill('[data-testid=auth-password]', PW)
await p.waitForTimeout(1500)
await p.locator('[data-testid=auth-submit]').click()
await p.waitForFunction(() => !document.querySelector('[data-testid=auth-email]'), null, { timeout: 60000 })

// Quantas vezes o browser pede a LISTA das gravações da sala. Conta-se desde
// antes de entrar: com o painel fechado tem de ficar a zero.
let listas = 0
p.on('request', (r) => { if (r.method() === 'GET' && new RegExp(`/api/rooms/${sala.code}/recordings$`).test(r.url())) listas++ })

// A rota é `#/r/<código>`, e a sala abre numa PRÉ-ENTRADA: o sinal de entrada
// é ela desaparecer (as armadilhas estão escritas no `reuniao.mjs`).
await p.goto(`${APP}/#/r/${sala.code}`, { waitUntil: 'domcontentloaded' })
await p.getByRole('button', { name: /entrar na sessão/i }).first().click({ timeout: 60000 })
const entrou = await p.waitForFunction(() => !document.querySelector('.rm-prejoin'), null, { timeout: 90000 }).then(() => true, () => false)
chk(entrou, 'entrou na sala pela interface (a pré-entrada desapareceu)')
if (!entrou) {
  await foto('nao-entrou')
  await b.close()
  console.log('\n=== 1 FALHARAM ===')
  process.exit(1)
}

// Com o painel FECHADO não há releitura de fundo: o hook vive em todos os
// participantes, e uma sala cheia não pode multiplicar os pedidos de uma
// gravação a compor. Espera-se mais do que um intervalo de releitura (4 s).
let releituras = 0
p.on('request', (r) => { if (/\/api\/recordings\/[^/]+\/details/.test(r.url())) releituras++ })
await p.waitForTimeout(7000)
chk(releituras === 0, `painel fechado: nenhuma releitura de fundo da gravação a compor → ${releituras}`)
chk(listas === 0, `painel fechado: entrar na sala não pede a lista das gravações → ${listas} pedidos`)

await p.getByRole('button', { name: /^participantes/i }).first().click({ timeout: 30000 })
await p.locator('.rm-rec').first().waitFor({ timeout: 30000 }).catch(() => {})
chk(listas >= 1, `abrir o painel lê a lista (controlo) → ${listas}`)
// A API, lida só DEPOIS de o painel ter linhas: é a prova de que a entrada na
// sala já ficou registada (antes disso a rota responde `403` a quem a criou).
const resposta = await j(`/api/rooms/${sala.code}/recordings`, { token: eu.tok })
const lista = resposta.j
chk(resposta.s === 200 && Array.isArray(lista), `API: a lista da sala responde a quem participa → ${resposta.s}`)
const daApi = (nome) => (Array.isArray(lista) ? lista.find((r) => r.filename === `${nome}.webm`) : null)
chk(daApi(nomes.compor)?.status === 'processing' && daApi(nomes.compor)?.progress_pct === 40, `API: a compor traz status e progresso → ${daApi(nomes.compor)?.status}/${daApi(nomes.compor)?.progress_pct}`)
chk(daApi(nomes.falhada)?.status === 'failed' && daApi(nomes.falhada)?.failure_reason === CAUSA, 'API: a falhada traz a causa')
chk(daApi(nomes.minha)?.status === 'ready' && daApi(nomes.minha)?.can_download === true, 'API: a pronta minha descarrega-se')
chk(daApi(nomes.alheia)?.status === 'ready' && daApi(nomes.alheia)?.can_download === false, 'API: a pronta de outra pessoa vê-se, mas não é minha para descarregar')
const recusa = await j(`/api/recordings/${daApi(nomes.alheia)?.id}/content?dl=1`, { token: eu.tok })
chk(recusa.s === 403, `e o servidor recusa mesmo esse download → ${recusa.s}`)

const linha = (nome) => p.locator('.rm-rec', { hasText: nome })
const texto = async (nome) => (await linha(nome).first().innerText().catch(() => '')).replace(/\s+/g, ' ')
chk(await p.locator('.rm-rec').count() === 4, `painel: as quatro gravações estão na lista → ${await p.locator('.rm-rec').count()}`)

// A compor: inerte, com o progresso, e nunca «0.0 MB».
chk(await linha(nomes.compor).count() === 1 && await p.locator('button.rm-rec', { hasText: nomes.compor }).count() === 0, 'a compor: a linha NÃO é um botão (R59)')
chk(/a processar 40%/i.test(await texto(nomes.compor)), `a compor: diz «A processar 40%» → ${JSON.stringify(await texto(nomes.compor))}`)
chk(!/MB/.test(await texto(nomes.compor)) && !/falhad/i.test(await texto(nomes.compor)), 'a compor: sem «0.0 MB» e sem «Falhada»')
// Falhada: inerte, com a causa.
chk(await p.locator('button.rm-rec', { hasText: nomes.falhada }).count() === 0, 'falhada: a linha NÃO é um botão (R59)')
chk(/falhada/i.test(await texto(nomes.falhada)) && (await texto(nomes.falhada)).includes(CAUSA), `falhada: diz «Falhada» e a causa → ${JSON.stringify(await texto(nomes.falhada))}`)
// Pronta e minha: o botão de sempre (controlo).
chk(await p.locator('button.rm-rec', { hasText: nomes.minha }).count() === 1, 'pronta minha: é um botão de descarregar (controlo)')
chk(/3\s?MB/.test(await texto(nomes.minha)), `pronta minha: mostra o tamanho → ${JSON.stringify(await texto(nomes.minha))}`)
// Pronta de outra pessoa: vê-se, sem botão.
chk(await linha(nomes.alheia).count() === 1 && await p.locator('button.rm-rec', { hasText: nomes.alheia }).count() === 0, 'pronta de outra pessoa: aparece, sem botão — o servidor recusaria o download')
chk(/só quem gravou/i.test(await texto(nomes.alheia)), `pronta de outra pessoa: e diz de quem é o download → ${JSON.stringify(await texto(nomes.alheia))}`)
chk(await p.locator('ul.rm-recs > li').count() === 4, 'as linhas são uma lista (ul/li), inertes incluídas')
chk(await p.locator('button.rm-rec').count() === 1, `no painel inteiro há UM botão de descarregar → ${await p.locator('button.rm-rec').count()}`)
// Carregar numa linha inerte não faz pedido nenhum ao ficheiro.
let pedidos = 0
const contar = (r) => { if (/\/api\/recordings\/[^/]+\/content/.test(r.url())) pedidos++ }
p.on('request', contar)
await linha(nomes.compor).first().click()
await linha(nomes.falhada).first().click()
await linha(nomes.alheia).first().click()
await p.waitForTimeout(1000)
p.off('request', contar)
chk(pedidos === 0, `carregar nas linhas inertes não pede ficheiro nenhum → ${pedidos} pedidos`)
await foto('painel')

// O progresso anda e a gravação fica pronta SEM sair da sala.
sql(`UPDATE recordings SET progress_pct = 80, progress_at = now() WHERE id = '${idCompor}'`)
const viu80 = await p.waitForFunction(
  (n) => [...document.querySelectorAll('.rm-rec')].some((e) => e.textContent.includes(n) && /a processar 80%/i.test(e.textContent)),
  nomes.compor, { timeout: 20000 },
).then(() => true, () => false)
chk(viu80, 'o progresso actualiza-se sozinho (40% → 80%)')
chk(releituras >= 1, `e foi a releitura de fundo que o trouxe, agora com o painel aberto (controlo) → ${releituras}`)
sql(`UPDATE recordings SET status = 'ready', size_bytes = 3145728, progress_pct = NULL, progress_at = NULL WHERE id = '${idCompor}'`)
const pronta = await p.waitForFunction(
  (n) => [...document.querySelectorAll('button.rm-rec')].some((e) => e.textContent.includes(n)),
  nomes.compor, { timeout: 20000 },
).then(() => true, () => false)
chk(pronta, 'quando a composição acaba, a linha passa a botão de descarregar, sem recarregar')
chk(/3\s?MB/.test(await texto(nomes.compor)) && !/a processar/i.test(await texto(nomes.compor)), 'e mostra o tamanho em vez de «A processar»')
const anuncio = await p.locator('ul.rm-recs + [role=status]').innerText().catch(() => '')
chk(anuncio.includes(nomes.compor) && /pronta/i.test(anuncio), `e a passagem é anunciada a quem não vê o ecrã → ${JSON.stringify(anuncio)}`)
await foto('pronta')

// ---------------------------------------------------------------------------
//  Uma gravação do servidor parada NA SALA entra na lista sozinha
// ---------------------------------------------------------------------------
const menu = async (item) => {
  await p.getByRole('button', { name: /mais opções/i }).first().click({ timeout: 15000 })
  await p.getByRole('menuitemcheckbox', { name: item }).or(p.getByRole('menuitem', { name: item })).first().click({ timeout: 15000 })
}
const antes = await p.locator('.rm-rec').count()
await menu(/gravar no servidor/i)
await p.waitForTimeout(4000)
await menu(/parar gravação no servidor/i)
const apareceu = await p.waitForFunction((n) => document.querySelectorAll('.rm-rec').length === n + 1, antes, { timeout: 20000 }).then(() => true, () => false)
chk(apareceu, `parada a gravação do servidor, ela entra na lista sem voltar a entrar na sala → ${await p.locator('.rm-rec').count()} linhas (eram ${antes})`)
// Assenta: pronta (com ffmpeg) ou falhada com causa (sem ele). A compor não fica.
const assentou = await p.waitForFunction(() => !document.querySelector('.rm-rec--processing'), null, { timeout: 90000 }).then(() => true, () => false)
chk(assentou, 'e deixa de estar «a processar» sozinha, sem recarregar')
const depois = (await j(`/api/rooms/${sala.code}/recordings`, { token: eu.tok })).j
const nova = Array.isArray(depois) ? depois.find((r) => /servidor/.test(r.filename)) : null
chk(!!nova && (nova.status === 'ready' || nova.status === 'failed'), `a API diz como acabou → ${nova?.status}`)
if (nova) {
  const eBotao = await p.locator('button.rm-rec', { hasText: nova.filename }).count() === 1
  const t = await texto(nova.filename)
  if (nova.status === 'ready') {
    chk(eBotao, 'acabou pronta: o painel oferece descarregar')
  } else {
    chk(!eBotao, 'acabou falhada: o painel NÃO oferece descarregar (R59)')
    chk(!!nova.failure_reason && t.includes(nova.failure_reason), `e diz a causa → ${JSON.stringify(t.slice(0, 140))}`)
  }
}
await foto('servidor')
await b.close()

console.log(`\n=== ${falhas === 0 ? 'TODAS PASSARAM' : falhas + ' FALHARAM'} ===`)
process.exit(falhas ? 1 : 0)
