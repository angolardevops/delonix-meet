// Uma gravação que o servidor ainda está a COMPOR tem de se ler «a processar»
// — não «falhada».
//
// O `recorder::insert_processing` cria a linha quando a gravação pára, ANTES
// de o ffmpeg correr, e só no fim a passa a `ready`. A regra que deriva o
// estado não conhecia `processing` e respondia `failed` sem causa: quem parava
// uma gravação via-a falhada até a composição acabar.
//
// A janela real dura de centenas de milissegundos a minutos, conforme o
// tamanho — não dá para a apanhar com um `sleep`. Por isso a linha entra por
// SQL, com as colunas e os valores que o gravador escreve, e fica parada o
// tempo que o teste precisar; no fim é o teste que a passa a `ready`, como o
// gravador faz.
//
// Verifica a API e as VISTAS (R59: um estado novo verifica-se em todas):
// lista, grelha e o cartão do início — o estado e o progresso à vista, nenhuma
// acção oferecida, nenhum leitor — e que a biblioteca e o início passam a
// «pronta» sem a pessoa recarregar a página. Uma gravação PRONTA ao lado serve
// de controlo: o que se recusa à que está a compor oferece-se a essa.
//
// E a releitura de fundo não pode custar o ecrã: com os pedidos dela a falhar,
// a biblioteca fica como estava.
//
// Uso: APP=http://localhost:5174 API=http://127.0.0.1:8180 node e2e/gravacao-a-compor.mjs
//      (a base: `PG=<contentor>`, ou `PG_EXEC` — ver `pg.mjs`). `FOTOS=<pasta>`
//      guarda uma captura de cada vista.
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

const m = Math.random().toString(36).slice(2, 7)
const email = `gc${m}@gc${m}.local`
await j('/api/auth/register', { method: 'POST', body: { org_name: `GC ${m}`, email, username: `gc${m}`, password: PW } })
const tok = (await j('/api/auth/login', { method: 'POST', body: { email, password: PW } })).j.access_token
const sala = (await j('/api/rooms', { token: tok, method: 'POST', body: { name: 'a compor', topology: 'sfu' } })).j
const uid = sql(`SELECT id FROM users WHERE email = '${email}'`)
chk(/^[0-9a-f-]{36}$/.test(uid) && !!sala?.id, 'conta e sala criadas')
// Quem grava esteve na sala: sem isto a biblioteca lia-a como «partilhada comigo».
sql(`INSERT INTO room_participants (room_id, user_id) VALUES ('${sala.id}', '${uid}') ON CONFLICT DO NOTHING`)

// As colunas e os valores do `recorder::insert_processing`, com 40 % feitos.
const nome = `A compor ${m}`
const id = sql(
  `INSERT INTO recordings (room_id, uploader_id, filename, size_bytes, status, progress_pct, progress_at, kind)
   VALUES ('${sala.id}', '${uid}', '${nome}.webm', 0, 'processing', 40, now(), 'meeting') RETURNING id`,
)
chk(/^[0-9a-f-]{36}$/.test(id), 'linha em `processing` inserida como o gravador a insere')
// O controlo: uma gravação pronta (sem ficheiro em disco — aqui só se lê a lista).
const nomePronta = `Pronta ${m}`
sql(
  `INSERT INTO recordings (room_id, uploader_id, filename, size_bytes, created_at)
   VALUES ('${sala.id}', '${uid}', '${nomePronta}.webm', 4, now() - interval '1 hour')`,
)

// ---------------------------------------------------------------------------
//  A API
// ---------------------------------------------------------------------------
const lista = (await j('/api/recordings', { token: tok })).j
const rec = Array.isArray(lista) ? lista.find((r) => r.id === id) : null
chk(!!rec, 'a gravação a compor está na biblioteca')
chk(rec?.status === 'processing' && rec?.state === 'processing', `status/state = processing → ${rec?.status}/${rec?.state}`)
chk(rec?.progress_pct === 40, `o progresso chega a quem lê → ${rec?.progress_pct}`)
chk(rec?.failure_reason === null, 'sem causa de falha: não falhou')
const d = await j(`/api/recordings/${id}/content`, { token: tok })
chk(d.s === 409 && d.j?.code === 'recording.processing', `o ficheiro ainda não existe → ${d.s} ${d.j?.code}`)
chk(typeof d.j?.error === 'string' && !/falhou/i.test(d.j.error), 'e a recusa não diz que «falhou»')

// ---------------------------------------------------------------------------
//  O ecrã
// ---------------------------------------------------------------------------
const b = await chromium.launch()
const p = await (await b.newContext({ locale: 'pt-PT', ignoreHTTPSErrors: true, viewport: { width: 1280, height: 800 } })).newPage()
const foto = async (n) => { if (FOTOS) await p.screenshot({ path: `${FOTOS}/${n}.png` }) }
await p.goto(`${APP}/#/login`, { waitUntil: 'domcontentloaded', timeout: 120000 })
await p.waitForSelector('[data-testid=auth-email]', { timeout: 120000 })
// A guia de boas-vindas do início taparia os cartões: fica dada como vista.
await p.evaluate((k) => localStorage.setItem(k, JSON.stringify({ step: 0, done: true, off: true })), `dx_tour_home:${uid}`)
await p.fill('[data-testid=auth-email]', email)
await p.fill('[data-testid=auth-password]', PW)
await p.waitForTimeout(1500)
await p.locator('[data-testid=auth-submit]').click()
await p.waitForFunction(() => !document.querySelector('[data-testid=auth-email]'), null, { timeout: 60000 })

// O INÍCIO: o cartão das gravações recentes.
await p.goto(`${APP}/#/`, { waitUntil: 'domcontentloaded' })
const cartaoInicio = p.locator('.home-rec[data-status="processing"]')
await cartaoInicio.first().waitFor({ timeout: 60000 }).catch(() => {})
chk(await cartaoInicio.count() === 1, 'início: o cartão da gravação a compor existe')
const textoInicio = await cartaoInicio.first().innerText().catch(() => '')
chk(/a processar/i.test(textoInicio) && !/falhou/i.test(textoInicio), `início: diz «A processar», não «Falhou» → ${JSON.stringify(textoInicio)}`)
chk(await p.locator('button.home-rec', { hasText: nome }).count() === 0, 'início: o cartão não é um botão (R59)')
await foto('inicio')

await p.goto(`${APP}/#/recordings`, { waitUntil: 'domcontentloaded' })
await p.waitForSelector('.rec-table, .rec-grid', { timeout: 60000 })

// Vista LISTA (omissão): a tabela.
const linha = p.locator('.rec-table tr[data-status="processing"]')
chk(await linha.count() === 1, 'lista: a gravação a compor está na tabela')
const textoLinha = await linha.first().innerText().catch(() => '')
chk(/a processar 40%/i.test(textoLinha), `lista: a linha diz «A processar 40%» → ${JSON.stringify(textoLinha.replace(/\s+/g, ' ').slice(0, 120))}`)
chk(!/falhad/i.test(textoLinha), 'lista: e não diz «Falhada»')
chk(await p.locator('.rec-table tr[data-status="failed"]').count() === 0, 'lista: nenhuma linha se apresenta como falhada')
chk(await linha.locator('button').count() === 0, 'lista: zero botões na linha — nem abrir, nem descarregar, nem partilhar (R59)')
chk(await p.locator('.rec-table tr[data-status="processing"].dx-row-link').count() === 0, 'lista: a linha NÃO se apresenta como clicável')
await linha.first().click()
await p.waitForTimeout(1000)
chk(await p.locator('.rec-panel.is-open').count() === 0, 'lista: carregar na linha não abre o leitor')
const tituloPainel = await p.locator('.rec-panel h2').first().innerText({ timeout: 2000 }).catch(() => '')
chk(tituloPainel === nomePronta, `lista: o painel continua na gravação pronta, não passa para a que está a compor → ${JSON.stringify(tituloPainel)}`)
chk(await p.locator('.rec-table tr[data-status="ready"] button').count() >= 1, 'lista: a gravação pronta ao lado TEM botão (controlo)')
// O menu de contexto (botão direito) é outra porta para as mesmas acções.
await linha.first().click({ button: 'right' })
await p.waitForTimeout(500)
chk(await p.locator('[role=menu]').count() === 0, 'lista: o botão direito não abre o menu de acções')
chk(await p.locator('video').count() === 0, 'lista: não há <video> para um ficheiro que ainda não existe')
await foto('lista')

// Vista GRELHA: os cartões.
await p.locator('.rec-views button').nth(1).click()
await p.waitForSelector('.rec-grid', { timeout: 10000 })
const cartao = p.locator('.rec-card[data-status="processing"]')
chk(await cartao.locator('.rec-card__thumb.is-processing').count() === 1, 'grelha: miniatura marcada como a compor')
chk(await cartao.locator('button').count() === 0, 'grelha: a miniatura NÃO é clicável e há zero acções oferecidas (R59)')
const textoCartao = await cartao.first().innerText().catch(() => '')
chk(/a processar 40%/i.test(textoCartao) && !/falhad/i.test(textoCartao), 'grelha: o cartão diz «A processar 40%», não «Falhada»')
chk(await p.locator('.rec-panel.is-open').count() === 0 && await p.locator('video').count() === 0, 'grelha: nenhum leitor nem <video>')
await cartao.first().click({ button: 'right' })
await p.waitForTimeout(500)
chk(await p.locator('[role=menu]').count() === 0, 'grelha: o botão direito não abre o menu de acções')
await foto('grelha')

// Uma releitura de fundo que FALHA não custa o ecrã: antes, a sondagem relia a
// lista inteira e um erro trocava a biblioteca por «tentar de novo» — e
// desmontava o leitor que estivesse a tocar.
let cortadas = 0
const cortar = (r) => { cortadas++; return r.abort() }
await p.route('**/api/recordings/*/details', cortar)
await p.waitForTimeout(10000)
chk(cortadas >= 1, `a releitura de fundo correu e falhou ${cortadas}×`)
chk(await p.locator('.rec-card').count() === 2 && await cartao.count() === 1, 'com a releitura a falhar, a biblioteca fica como estava')
chk(await p.locator('.dx-alert--danger').count() === 0, 'e não aparece erro nenhum por cima dela')
await p.unroute('**/api/recordings/*/details', cortar)

// O progresso anda e a gravação fica pronta SEM recarregar a página: é o que
// o gravador escreve na base, e a consola relê as gravações que estão a compor.
sql(`UPDATE recordings SET progress_pct = 80, progress_at = now() WHERE id = '${id}'`)
const viu80 = await p.waitForFunction(
  () => /a processar 80%/i.test(document.querySelector('.rec-card[data-status="processing"]')?.textContent ?? ''),
  null, { timeout: 20000 },
).then(() => true, () => false)
chk(viu80, 'o progresso actualiza-se sozinho (40% → 80%)')
sql(`UPDATE recordings SET status = 'ready', size_bytes = 4, progress_pct = NULL, progress_at = NULL WHERE id = '${id}'`)
const pronta = await p.waitForFunction(
  // Dois cartões prontos, cada um com a sua miniatura-botão: o de controlo e
  // ESTE. Só «há um botão» passava com o de controlo sozinho.
  () => !document.querySelector('.rec-card[data-status="processing"]') && document.querySelectorAll('.rec-card[data-status="ready"] button.rec-card__thumb').length === 2,
  null, { timeout: 20000 },
).then(() => true, () => false)
chk(pronta, 'quando a composição acaba, o cartão passa a pronto e a miniatura a botão, sem recarregar')
// Controlo do menu: sobre a MESMA gravação, já pronta, o botão direito abre-o —
// sem isto, «não abre» acima passava também com um selector errado.
await p.locator('.rec-card[data-status="ready"]', { hasText: nome }).first().click({ button: 'right' })
const abriu = await p.locator('[role=menu]').first().waitFor({ timeout: 5000 }).then(() => true, () => false)
chk(abriu, 'e, já pronta, o botão direito abre o menu de acções (controlo)')
await foto('pronta')

// O INÍCIO acompanha da mesma maneira: o cartão passa de «A processar» a botão.
sql(`UPDATE recordings SET status = 'processing', size_bytes = 0, progress_pct = 10, progress_at = now() WHERE id = '${id}'`)
await p.goto(`${APP}/#/`, { waitUntil: 'domcontentloaded' })
const deNovoACompor = await cartaoInicio.first().waitFor({ timeout: 30000 }).then(() => true, () => false)
sql(`UPDATE recordings SET status = 'ready', size_bytes = 4, progress_pct = NULL, progress_at = NULL WHERE id = '${id}'`)
const prontaNoInicio = deNovoACompor && await p.locator('button.home-rec', { hasText: nome }).first().waitFor({ timeout: 20000 }).then(() => true, () => false)
chk(prontaNoInicio, 'início: quando a composição acaba, o cartão passa a botão sem recarregar')
await b.close()

console.log(`\n=== ${falhas === 0 ? 'TODAS PASSARAM' : falhas + ' FALHARAM'} ===`)
process.exit(falhas ? 1 : 0)
