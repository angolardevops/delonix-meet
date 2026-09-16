#!/usr/bin/env node
// Testes de ISOLAMENTO cross-tenant e de PERMISSÕES NEGATIVAS.
//
// É o critério de saída do §5.4 do mandato: «nenhum endpoint cross-tenant;
// testes automáticos de isolamento; testes de permissões negativas».
//
// Correm contra um servidor A SÉRIO, com Postgres a sério. Um teste de
// isolamento contra um duplo prova o duplo, não o produto — e isolamento é
// precisamente a garantia que se paga para ter.
//
// Duas organizações independentes (domínios de email diferentes, porque o
// servidor impõe uma org por domínio) e, para cada recurso, verifica-se que a
// org A não alcança o que é da org B. O que se procura NÃO é um erro bonito: é
// que a resposta não traga dados de outro inquilino.
//
// Uso:  node e2e/isolamento.mjs
import WebSocket from 'ws'

const API = process.env.API ?? 'http://127.0.0.1:8180'
const WS = (process.env.API ?? 'http://127.0.0.1:8180').replace(/^http/, 'ws')
const PW = 'UmaPasswordForte123!'

let passou = 0
let falhou = 0
const falhas = []

function ok(nome) {
  passou++
  console.log(`  ✓ ${nome}`)
}
function nok(nome, detalhe) {
  falhou++
  falhas.push({ nome, detalhe })
  console.log(`  ✗ ${nome}\n      ${detalhe}`)
}

async function req(path, { token, method = 'GET', body } = {}) {
  const r = await fetch(`${API}${path}`, {
    method,
    headers: {
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
      ...(body ? { 'Content-Type': 'application/json' } : {}),
    },
    ...(body ? { body: JSON.stringify(body) } : {}),
  })
  let json = null
  try {
    json = await r.json()
  } catch {
    /* resposta sem corpo */
  }
  return { status: r.status, json }
}

/** Cria uma organização nova com o seu administrador. */
async function novaOrg(sufixo) {
  const email = `admin@${sufixo}.local`
  const reg = await req('/api/auth/register', {
    method: 'POST',
    body: { org_name: `Org ${sufixo}`, email, username: `admin-${sufixo}`, password: PW },
  })
  const login = await req('/api/auth/login', { method: 'POST', body: { email, password: PW } })
  const token = login.json?.access_token ?? reg.json?.access_token
  if (!token) throw new Error(`não consegui autenticar ${email}: ${JSON.stringify(login.json)}`)
  const orgs = await req('/api/orgs', { token })
  return { email, token, orgId: orgs.json?.[0]?.id, userId: login.json?.user?.id }
}

/**
 * Um pedido que TEM de ser recusado. Aceita-se 401/403/404 — o código exacto é
 * decisão de desenho (404 esconde a existência do recurso, o que é defensável);
 * o que NÃO se aceita é 2xx.
 */
async function recusado(nome, path, opts) {
  const { status, json } = await req(path, opts)
  if (status >= 200 && status < 300) {
    nok(nome, `devolveu ${status} com ${JSON.stringify(json).slice(0, 160)}`)
    return
  }
  ok(`${nome} → ${status}`)
}

/**
 * Recusa ANTES de o handler correr: só 401/403/404. Um `400` não conta — quer
 * dizer que o pedido passou a autorização e o handler o rejeitou por outra
 * razão (no `/platform/storage/test`, o 400 era o servidor a TENTAR o pedido
 * ao URL do atacante e a falhar a ligação).
 */
async function recusadoNaPorta(nome, path, opts) {
  const { status, json } = await req(path, opts)
  if ([401, 403, 404].includes(status)) {
    ok(`${nome} → ${status}`)
    return
  }
  nok(nome, `devolveu ${status} com ${JSON.stringify(json).slice(0, 160)} — só 401/403/404 provam que o handler não correu`)
}

/** Um pedido que tem de ser ACEITE (prova que o teste não passa por acidente). */
async function permitido(nome, path, opts) {
  const { status, json } = await req(path, opts)
  if (status >= 200 && status < 300) {
    ok(`${nome} → ${status}`)
    return json
  }
  nok(nome, `devia ser permitido mas devolveu ${status}: ${JSON.stringify(json).slice(0, 160)}`)
  return null
}

const marca = Math.random().toString(36).slice(2, 8)
console.log(`\n=== Isolamento cross-tenant (marca ${marca}) ===\n`)

const A = await novaOrg(`alfa${marca}`)
const B = await novaOrg(`beta${marca}`)
console.log(`org A = ${A.orgId}\norg B = ${B.orgId}\n`)

// Recursos da org B, criados por B.
const salaB = (await req('/api/rooms', {
  token: B.token,
  method: 'POST',
  body: { name: 'sala privada da B', topology: 'sfu' },
})).json

console.log('--- controlo positivo: B alcança o que é seu ---')
await permitido('B lê a própria org', `/api/orgs/${B.orgId}/stats`, { token: B.token })
await permitido('B lê a própria sala', `/api/rooms/${salaB.code}`, { token: B.token })

console.log('\n--- org A contra recursos da org B ---')
await recusado('A lê stats da org B', `/api/orgs/${B.orgId}/stats`, { token: A.token })
await recusado('A lista empregados da org B', `/api/orgs/${B.orgId}/employees`, { token: A.token })
await recusado('A lista filiais da org B', `/api/orgs/${B.orgId}/branches`, { token: A.token })
await recusado('A lista grupos da org B', `/api/orgs/${B.orgId}/groups`, { token: A.token })
await recusado('A lista salas presenciais da org B', `/api/orgs/${B.orgId}/meeting-rooms`, { token: A.token })
await recusado('A lista webhooks da org B', `/api/orgs/${B.orgId}/webhooks`, { token: A.token })
await recusado('A lista chaves de API da org B', `/api/orgs/${B.orgId}/api-keys`, { token: A.token })
await recusado('A lê a config Odoo da org B', `/api/orgs/${B.orgId}/integration/odoo`, { token: A.token })
await recusado('A lista DIDs de voz da org B', `/api/orgs/${B.orgId}/voice/dids`, { token: A.token })
await recusado('A lista CDR de voz da org B', `/api/orgs/${B.orgId}/voice/cdr`, { token: A.token })

console.log('\n--- escrita cross-tenant ---')
await recusado('A altera definições da org B', `/api/orgs/${B.orgId}/settings`, {
  token: A.token, method: 'POST', body: { hide_org_creation: true },
})
await recusado('A roda o token Odoo da org B', `/api/orgs/${B.orgId}/integration/odoo/token`, {
  token: A.token, method: 'POST', body: {},
})
await recusado('A remove um empregado da org B', `/api/orgs/${B.orgId}/employees/${B.userId}`, {
  token: A.token, method: 'DELETE',
})

// As seis rotas com escopo de organização que este teste NÃO cobria (R95).
// Encontradas a comparar o inventário do que EXISTE (`grep` às rotas do
// `main.rs`) com o inventário do que se TESTA — o mesmo método que apanhou os
// testes ponta-a-ponta que nunca corriam (R72).
console.log('\n--- as seis que faltavam ---')
await recusado('A lê a trilha de auditoria da org B', `/api/orgs/${B.orgId}/audit`, { token: A.token })
await recusado('A verifica a cadeia de auditoria da org B', `/api/orgs/${B.orgId}/audit/verify`, {
  token: A.token,
})
await recusado('A lê a configuração de SSO da org B', `/api/orgs/${B.orgId}/sso`, { token: A.token })
await recusado('A lê a facturação de voz da org B', `/api/orgs/${B.orgId}/voice/billing`, {
  token: A.token,
})

// Os dois DELETE precisam de um recurso REAL. Com um UUID ao acaso, um `404`
// contaria como recusa e não provaria autorização nenhuma — só que o recurso
// não existe. B cria, A tenta apagar, e a asserção que interessa é a última: o
// recurso de B tem de CONTINUAR LÁ.
console.log('\n--- destruição cross-tenant: o recurso tem de sobreviver ---')
const chaveB = await req(`/api/orgs/${B.orgId}/api-keys`, {
  token: B.token, method: 'POST', body: { name: 'chave-de-teste' },
})
if (chaveB.status >= 200 && chaveB.status < 300 && chaveB.json?.id) {
  await recusado('A apaga uma chave de API da org B', `/api/orgs/${B.orgId}/api-keys/${chaveB.json.id}`, {
    token: A.token, method: 'DELETE',
  })
  const depois = await req(`/api/orgs/${B.orgId}/api-keys`, { token: B.token })
  const sobreviveu = Array.isArray(depois.json) && depois.json.some((k) => k.id === chaveB.json.id)
  if (sobreviveu) ok('e a chave da B CONTINUA LÁ')
  else nok('e a chave da B CONTINUA LÁ', 'desapareceu — a recusa foi só no código de estado')
} else {
  nok('B cria uma chave de API para o teste', `devolveu ${chaveB.status}`)
}

const hookB = await req(`/api/orgs/${B.orgId}/webhooks`, {
  token: B.token, method: 'POST',
  body: { kind: 'generic', url: 'https://example.com/hook', secret: 's3cr3t-de-teste' },
})
if (hookB.status >= 200 && hookB.status < 300 && hookB.json?.id) {
  await recusado('A apaga um webhook da org B', `/api/orgs/${B.orgId}/webhooks/${hookB.json.id}`, {
    token: A.token, method: 'DELETE',
  })
  const depois = await req(`/api/orgs/${B.orgId}/webhooks`, { token: B.token })
  const sobreviveu = Array.isArray(depois.json) && depois.json.some((h) => h.id === hookB.json.id)
  if (sobreviveu) ok('e o webhook da B CONTINUA LÁ')
  else nok('e o webhook da B CONTINUA LÁ', 'desapareceu — a recusa foi só no código de estado')
} else {
  // O guarda de SSRF pode recusar o URL; se assim for, diz-se, em vez de o
  // teste passar em silêncio por não ter criado nada.
  nok('B cria um webhook para o teste', `devolveu ${hookB.status}: ${JSON.stringify(hookB.json).slice(0, 120)}`)
}

console.log('\n--- salas da org B: o código é uma CAPABILITY, não um passe ---')
//
// Aqui a expectativa ingénua ("A tem de levar 403") está ERRADA, e é preciso
// dizer porquê: o código da sala é uma capability à maneira do Meet — quem o
// conhece pode ver os metadados e PEDIR para entrar. Foi verificado no fio, não
// suposto: o dono recebe `joined` (media directa), a org A recebe `waiting`.
//
// A invariante que interessa não é «A é recusado», é **A nunca obtém acesso
// DIRECTO à media de outra organização**. É essa que se testa.
await permitido('A vê metadados da sala da B (capability por código)', `/api/rooms/${salaB.code}`, { token: A.token })

const joinA = await permitido('A pede para entrar na sala da B', `/api/rooms/${salaB.code}/join`, {
  token: A.token, method: 'POST',
})
const joinB = await permitido('B (dono) entra na sua sala', `/api/rooms/${salaB.code}/join`, {
  token: B.token, method: 'POST',
})

/** Liga o WS com um room token e devolve o primeiro veredicto do servidor. */
async function veredictoWs(roomToken, code) {
  return new Promise((resolve) => {
    const ws = new WebSocket(`${WS}/ws?token=${encodeURIComponent(roomToken)}&room=${code}`)
    const t = setTimeout(() => { ws.close(); resolve('sem-resposta') }, 8000)
    ws.on('message', (d) => {
      const m = JSON.parse(d.toString())
      if (['joined', 'waiting', 'denied', 'error'].includes(m.type)) {
        clearTimeout(t); ws.close(); resolve(m.type)
      }
    })
    ws.on('error', () => { clearTimeout(t); resolve('erro') })
  })
}

const vA = await veredictoWs(joinA.room_token, salaB.code)
if (vA === 'waiting') ok('A cai na SALA DE ESPERA da sala da B (sem media)')
else nok('A cai na sala de espera da sala da B', `o servidor respondeu "${vA}" — se for "joined", é acesso DIRECTO à media de outra organização`)

const vB = await veredictoWs(joinB.room_token, salaB.code)
if (vB === 'joined') ok('B (dono) entra directo na sua própria sala (controlo positivo)')
else nok('B entra directo na sua sala', `respondeu "${vB}" — se o dono não entra, o teste acima não prova nada`)

console.log('\n--- o que o código NÃO abre ---')
await recusado('A lê o chat da sala da B', `/api/rooms/${salaB.code}/chat`, { token: A.token })
await recusado('A lista gravações da sala da B', `/api/rooms/${salaB.code}/recordings`, { token: A.token })
await recusado('A lê notas da sala da B', `/api/rooms/${salaB.code}/notes`, { token: A.token })
await recusado('A reporta QoS na sala da B', `/api/rooms/${salaB.code}/qos`, {
  token: A.token, method: 'POST', body: { rtt_ms: 1, loss_pct: 0, up_kbps: 1 },
})
await recusado('anónimo vê metadados da sala da B', `/api/rooms/${salaB.code}`, {})

// REUNIÕES, GRAVAÇÕES E QUADROS da org B (R96).
//
// O mesmo inventário-contra-inventário que deu as seis rotas de organização
// (R95), agora aplicado aos recursos POR ID. Existiam 32 rotas não-públicas de
// sala/reunião/gravação/quadro e o teste tocava em 8. As restantes nunca
// tinham sido pedidas com o token do inquilino errado.
//
// A regra que decide o que é grave: uma sala é uma CAPABILITY (quem sabe o
// código vê os metadados e pede para entrar — está no topo deste ficheiro). Um
// recurso por ID não é: a acta de uma reunião, o ficheiro de uma gravação e o
// PNG de um quadro não têm código para partilhar. O `id` é opaco e não
// autoriza nada.
// Um id que a org A inventa. Serve onde o recurso não se pode FABRICAR sem uma
// chamada a sério (gravações) — e onde é usado está dito que um 404 não
// distingue «não é tua» de «não existe».
const inventado = '00000000-0000-4000-8000-000000000000'

console.log('\n--- reuniões, gravações e quadros da org B ---')

const reuniaoB = await req('/api/meetings', {
  token: B.token, method: 'POST',
  body: {
    title: 'reunião privada da B',
    kind: 'video',
    starts_at: new Date(Date.now() + 3600_000).toISOString(),
  },
})
if (reuniaoB.status >= 200 && reuniaoB.status < 300 && reuniaoB.json?.id) {
  const m = reuniaoB.json.id
  // `/api/meetings/{id}` só tem DELETE e `/minutes` só tem POST — um GET
  // devolve 405, que o helper contava como recusa sem provar nada. Foi o
  // CONTROLO POSITIVO abaixo que deu por isso: «B lê a sua própria reunião»
  // devolvia 405 também. Sem ele, duas asserções verdes mediam o router, não a
  // autorização.
  await recusado('A APAGA a reunião da org B', `/api/meetings/${m}`, {
    token: A.token, method: 'DELETE',
  })
  await recusado('A escreve a ACTA da reunião da B', `/api/meetings/${m}/minutes`, {
    token: A.token, method: 'POST', body: { markdown: 'acta forjada' },
  })
  await recusado('A lê a agenda da reunião da B', `/api/meetings/${m}/agenda`, { token: A.token })
  await recusado('A lê os convidados da reunião da B', `/api/meetings/${m}/invitees`, { token: A.token })
  await recusado('A lê o plano de acção da reunião da B', `/api/meetings/${m}/action-plan`, { token: A.token })
  await recusado('A descarrega o ICS da reunião da B', `/api/meetings/${m}/ics`, { token: A.token })
  await recusado('A responde ao convite da reunião da B', `/api/meetings/${m}/respond`, {
    token: A.token, method: 'POST', body: { status: 'accepted' },
  })
  await recusado('A ARRANCA a reunião da B', `/api/meetings/${m}/start`, {
    token: A.token, method: 'POST', body: {},
  })
  // Controlo positivo: sem ele, um `404` em tudo podia ser a reunião não
  // existir, e as oito asserções acima passavam a medir nada.
  await permitido('B lista as suas reuniões e a dela lá está (controlo positivo)', '/api/meetings', {
    token: B.token,
  })
  // E o controlo que fecha o buraco de cima: a reunião TEM de continuar a
  // existir depois de a org A tentar apagá-la.
  const listaB = await req('/api/meetings', { token: B.token })
  const viva = Array.isArray(listaB.json?.meetings ?? listaB.json)
    && (listaB.json.meetings ?? listaB.json).some((x) => x.id === m)
  if (viva) ok('e a reunião da B CONTINUA LÁ depois de A tentar apagá-la')
  else nok('e a reunião da B CONTINUA LÁ', 'desapareceu — a recusa foi só no código de estado')

  // Fecha o ciclo do plano de acção: criar um item no plano de outra empresa.
  await recusado('A cria um item no plano de acção da B', `/api/meetings/${m}/action-plan/items`, {
    token: A.token, method: 'POST', body: { text: 'tarefa forjada' },
  })
} else {
  nok('B cria uma reunião para o teste', `devolveu ${reuniaoB.status}: ${JSON.stringify(reuniaoB.json).slice(0, 140)}`)
}

// PNG mínimo de 1×1 — o handler descodifica e valida, por isso tem de ser real.
const PNG_1x1 =
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=='
const quadroB = await req('/api/whiteboards', {
  token: B.token, method: 'POST', body: { title: 'quadro da B', png_base64: PNG_1x1 },
})
if (quadroB.status >= 200 && quadroB.status < 300 && quadroB.json?.id) {
  const q = quadroB.json.id
  await recusado('A descarrega o PNG do quadro da B', `/api/whiteboards/${q}/png`, { token: A.token })
  await recusado('A PARTILHA o quadro da B por link', `/api/whiteboards/${q}/share`, {
    token: A.token, method: 'POST', body: { public: true },
  })
  await recusado('A apaga o quadro da B', `/api/whiteboards/${q}`, {
    token: A.token, method: 'DELETE',
  })
  const listaB = await req('/api/whiteboards', { token: B.token })
  const vive = Array.isArray(listaB.json) && listaB.json.some((w) => w.id === q)
  if (vive) ok('e o quadro da B CONTINUA LÁ')
  else nok('e o quadro da B CONTINUA LÁ', 'desapareceu — a recusa foi só no código de estado')
} else {
  nok('B cria um quadro para o teste', `devolveu ${quadroB.status}: ${JSON.stringify(quadroB.json).slice(0, 140)}`)
}


// Recursos ligados à SALA da org B, com o código real dela. O código é uma
// capability para VER metadados e PEDIR entrada — não para escrever.
console.log('\n--- o que o código da sala NÃO autoriza a escrever ---')
await recusado('A convida gente para a sala da B', `/api/rooms/${salaB.code}/invite`, {
  token: A.token, method: 'POST', body: { user_ids: [] },
})
await recusado('A lê a acta da sala da B', `/api/rooms/${salaB.code}/minutes`, { token: A.token })
await recusado('A escreve a acta da sala da B', `/api/rooms/${salaB.code}/minutes`, {
  token: A.token, method: 'POST', body: { markdown: 'acta forjada' },
})
await recusado('A reporta tempos de chamada na sala da B', `/api/rooms/${salaB.code}/timings`, {
  token: A.token, method: 'POST', body: { join_ms: 1 },
})
await recusado('A partilha uma gravação alheia com alguém', `/api/recordings/${inventado}/share`, {
  token: A.token, method: 'POST', body: { user_id: inventado },
})
await recusado('A revoga a partilha de uma gravação alheia', `/api/recordings/${inventado}/share/${inventado}`, {
  token: A.token, method: 'DELETE',
})

// As gravações não se podem FABRICAR sem uma chamada a sério, por isso o que
// aqui se prova é a forma da recusa com um id que a org A inventa. É menos do
// que o resto deste ficheiro e está dito: um `404` aqui não distingue «não é
// tua» de «não existe». O caminho por id fica coberto pela sala
// (`/api/rooms/{code}/recordings`, acima), que usa um id REAL da org B.
await recusado('A descarrega uma gravação por id inventado', `/api/recordings/${inventado}`, {
  token: A.token,
})
await recusado('A cria link de partilha de uma gravação alheia', `/api/recordings/${inventado}/link`, {
  token: A.token, method: 'POST', body: {},
})
await recusado('A mexe num item de acção por id inventado', `/api/action-items/${inventado}`, {
  token: A.token, method: 'PATCH', body: { done: true },
})

// ─────────────────────────────────────────────────────────────────────────────
// S1–S3 da auditoria de 2026-09-16 (docs/auditoria-2026-09-16-backend.md).
//
// As três tinham a mesma forma: uma regra de acesso escrita em mais de um sítio,
// e a cópia que decidia estava errada. Cada caso abaixo corre o caminho de
// ataque inteiro e verifica o ESTADO depois — não só o código de estado.
// ─────────────────────────────────────────────────────────────────────────────

console.log('\n--- S1: registar-se não faz de ninguém administrador da plataforma ---')
// O `register` cria sempre o autor como admin da sua org nova. O armazenamento
// da plataforma decidia «admin da plataforma» como «admin de QUALQUER org» — ou
// seja, qualquer pessoa na Internet. Controlo positivo: não há aqui, e está
// dito. O administrador da plataforma é declarado na configuração do servidor
// (`PLATFORM_ADMIN_USER_IDS`) e o utilizador deste teste só nasce depois do
// arranque; o caminho positivo é provado em `storage::tests`.
await recusadoNaPorta('admin de org recém-registado LÊ o armazenamento da plataforma', '/api/v1/platform/storage', {
  token: A.token,
})
await recusadoNaPorta('admin de org recém-registado ESCREVE o armazenamento da plataforma', '/api/v1/platform/storage', {
  token: A.token, method: 'PUT',
  body: { storage_type: 'webdav', webdav_url: 'http://169.254.169.254/', webdav_user: 'x' },
})
await recusadoNaPorta('admin de org recém-registado dispara o teste de ligação (PROPFIND)', '/api/v1/platform/storage/test', {
  token: A.token, method: 'POST', body: {},
})

console.log('\n--- S2: a sincronização Odoo não captura contas de outra organização ---')
// A org A emite uma chave `dlx_` (o extractor do Odoo aceita-a) e lista no seu
// «directório» o endereço do administrador da org B. Antes: a conta de B era
// reescrita (nome, `odoo_managed`) e entrava na org A como admin.
const chaveA = await req(`/api/orgs/${A.orgId}/api-keys`, {
  token: A.token, method: 'POST', body: { name: 's2-provision' },
})
if (chaveA.status >= 200 && chaveA.status < 300 && chaveA.json?.key) {
  const novoEmail = `novo-${marca}@alfa${marca}.local`
  const prov = await req('/api/v1/integration/odoo/provision', {
    token: chaveA.json.key, method: 'POST',
    body: {
      company: `Org alfa${marca}`,
      admin_email: A.email,
      users: [
        { odoo_uid: 91, name: 'CAPTURADO', email: B.email, is_admin: true },
        { odoo_uid: 92, name: 'Novo da A', email: novoEmail },
      ],
    },
  })
  if (prov.status >= 200 && prov.status < 300) ok(`a sincronização corre para o resto do lote → ${prov.status}`)
  else nok('a sincronização corre para o resto do lote', `devolveu ${prov.status}: ${JSON.stringify(prov.json).slice(0, 160)}`)

  const empA = await req(`/api/orgs/${A.orgId}/employees`, { token: A.token })
  const emails = Array.isArray(empA.json) ? empA.json.map((e) => e.email) : []
  if (!emails.includes(B.email)) ok('o administrador da org B NÃO entrou na org A')
  else nok('o administrador da org B NÃO entrou na org A', `está na lista de empregados da A: ${JSON.stringify(emails)}`)
  if (emails.includes(novoEmail)) ok('controlo positivo: a conta NOVA foi criada na org A')
  else nok('controlo positivo: a conta NOVA foi criada na org A', `não está: ${JSON.stringify(emails)} — sem isto a recusa acima pode ser o endpoint partido`)

  const euB = await req('/api/users/me', { token: B.token })
  if (euB.json?.username && euB.json.username !== 'CAPTURADO') ok('a conta de B não foi reescrita')
  else nok('a conta de B não foi reescrita', `username = ${JSON.stringify(euB.json?.username)}`)
  const loginB = await req('/api/auth/login', { method: 'POST', body: { email: B.email, password: PW } })
  if (loginB.status === 200) ok('B continua a entrar com a sua password local')
  else nok('B continua a entrar com a sua password local', `login devolveu ${loginB.status}`)
} else {
  nok('A cria uma chave de API para o teste S2', `devolveu ${chaveA.status}`)
}

console.log('\n--- S3: um membro ARQUIVADO perde o acesso da organização ---')
// Dois colaboradores da org A: C (membro) e D (admin). Cada acesso é provado
// ANTES de arquivar (controlo positivo) e recusado DEPOIS. Sem o «antes», um
// «depois» recusado podia ser só o recurso a não existir.
const dominioA = A.email.split('@')[1]
async function colaborador(nome, role) {
  const email = `${nome}-${marca}@${dominioA}`
  const r = await req(`/api/orgs/${A.orgId}/employees`, {
    token: A.token, method: 'POST',
    body: { email, username: `${nome}-${marca}`, password: PW, role, title: nome },
  })
  if (!(r.status >= 200 && r.status < 300)) throw new Error(`não criei ${email}: ${r.status} ${JSON.stringify(r.json)}`)
  const l = await req('/api/auth/login', { method: 'POST', body: { email, password: PW } })
  return { email, token: l.json?.access_token, userId: r.json.user_id, username: `${nome}-${marca}` }
}
const C = await colaborador('carla', 'member')
const D = await colaborador('dario', 'admin')

const salaA = (await req('/api/rooms', { token: A.token, method: 'POST', body: { name: 'sala da A', topology: 'sfu' } })).json
await req(`/api/rooms/${salaA.code}/join`, { token: A.token, method: 'POST' })
const upload = await fetch(`${API}/api/rooms/${salaA.code}/recordings?name=s3.webm`, {
  method: 'POST', headers: { Authorization: `Bearer ${A.token}` }, body: new Uint8Array([0x1a, 0x45, 0xdf, 0xa3]),
})
const gravacaoA = upload.ok ? (await upload.json()).id : null
if (!gravacaoA) nok('A carrega uma gravação para o teste S3', `devolveu ${upload.status}`)

const pesquisaPorA = async (token) => {
  const r = await req(`/api/users/search?q=${encodeURIComponent(`admin-alfa${marca}`)}`, { token })
  return Array.isArray(r.json) && r.json.some((u) => u.id === A.userId)
}

// ANTES
await permitido('C (membro activo) lê o chat da sala da A', `/api/rooms/${salaA.code}/chat`, { token: C.token })
if (await pesquisaPorA(C.token)) ok('C (membro activo) encontra o admin da A na pesquisa')
else nok('C (membro activo) encontra o admin da A na pesquisa', 'não encontrou — o controlo positivo falhou')
if (gravacaoA) {
  await permitido('D (admin activo) descarrega a gravação da A', `/api/recordings/${gravacaoA}?dl=1`, { token: D.token })
}
await permitido('controlo: a chave da A cria reunião com C como anfitriã', '/api/v1/meetings', {
  token: chaveA.json?.key, method: 'POST',
  body: { title: 's3 antes', starts_at: new Date(Date.now() + 7200_000).toISOString(), host_email: C.email },
})

// ARQUIVAR
await permitido('A arquiva C', `/api/orgs/${A.orgId}/employees/${C.userId}`, { token: A.token, method: 'DELETE' })
await permitido('A arquiva D', `/api/orgs/${A.orgId}/employees/${D.userId}`, { token: A.token, method: 'DELETE' })

// DEPOIS
await recusado('C ARQUIVADA lê o chat da sala da A', `/api/rooms/${salaA.code}/chat`, { token: C.token })
if (!(await pesquisaPorA(C.token))) ok('C ARQUIVADA já não encontra o admin da A na pesquisa')
else nok('C ARQUIVADA já não encontra o admin da A na pesquisa', 'o directório da ex-organização continua visível')
if (gravacaoA) {
  await recusado('D ARQUIVADO descarrega a gravação da A', `/api/recordings/${gravacaoA}?dl=1`, { token: D.token })
}
await recusado('a chave da A cria reunião com C ARQUIVADA como anfitriã', '/api/v1/meetings', {
  token: chaveA.json?.key, method: 'POST',
  body: { title: 's3 depois', starts_at: new Date(Date.now() + 9000_000).toISOString(), host_email: C.email },
})

// ---------------------------------------------------------------------------
// Gateway de SMS (ADR-0005). Um SMS custa dinheiro a quem o envia: a pergunta
// não é só «A lê o que é de B», é também «o gateway de A consegue gastar o
// telefone de B, ou mentir sobre o resultado de uma mensagem de B».
// Tudo com recursos REAIS de B, e com o controlo positivo no fim: o agente de B
// reclama e confirma a sua própria mensagem.
// ---------------------------------------------------------------------------
console.log('\n--- gateway de SMS ---')
const modemFalso = (sufixo) => ({
  device_key: `1e0e:9001:TESTE-${sufixo}`,
  vendor_id: '1e0e',
  product_id: '9001',
  manufacturer: 'SIMCOM',
  product: 'SIM7600',
  serial: `TESTE-${sufixo}`,
  kind: 'modem',
  transport: 'at_serial',
  port: '/dev/ttyUSB2',
  capable: true,
  reason: null,
  operator_name: 'UNITEL',
  signal_percent: 70,
})
const gwB = await req(`/api/orgs/${B.orgId}/sms/gateways`, { token: B.token, method: 'POST', body: { name: 'gw-B' } })
const gwA = await req(`/api/orgs/${A.orgId}/sms/gateways`, { token: A.token, method: 'POST', body: { name: 'gw-A' } })
if (gwB.status !== 201 || !gwB.json?.token?.startsWith('dlxg_') || gwA.status !== 201) {
  nok('B e A criam gateways (201 com token dlxg_)', `B=${gwB.status} A=${gwA.status}`)
} else {
  ok('B e A criam gateways (201 com token dlxg_)')
  await permitido('o agente de B reporta um modem', '/api/sms/agent/devices', {
    token: gwB.json.token, method: 'PUT', body: { devices: [modemFalso('B')] },
  })
  await permitido('o agente de A reporta um modem', '/api/sms/agent/devices', {
    token: gwA.json.token, method: 'PUT', body: { devices: [modemFalso('A')] },
  })
  const devB = (await req(`/api/orgs/${B.orgId}/sms/devices`, { token: B.token })).json?.[0]
  await permitido('B escolhe o seu modem como ponto de envio', `/api/orgs/${B.orgId}/sms/route`, {
    token: B.token, method: 'PUT', body: { device_id: devB?.id },
  })
  const msgB = await req(`/api/orgs/${B.orgId}/sms/messages`, {
    token: B.token, method: 'POST', body: { to: '923 000 001', body: 'teste de isolamento', route: 'usb' },
  })
  if (msgB.status === 202 && msgB.json?.route === 'usb' && msgB.json?.to === '+244923000001') {
    ok('B põe um SMS em fila pela rota USB (202)')
  } else nok('B põe um SMS em fila pela rota USB (202)', `${msgB.status} ${JSON.stringify(msgB.json)}`)

  // Leitura cross-tenant — as rotas da consola.
  await recusado('A lista os gateways de SMS da org B', `/api/orgs/${B.orgId}/sms/gateways`, { token: A.token })
  await recusado('A lista os dispositivos USB da org B', `/api/orgs/${B.orgId}/sms/devices`, { token: A.token })
  await recusado('A lê a rota de SMS da org B', `/api/orgs/${B.orgId}/sms/route`, { token: A.token })
  await recusado('A lista as mensagens SMS da org B', `/api/orgs/${B.orgId}/sms/messages`, { token: A.token })
  await recusado('A lê uma mensagem SMS da org B', `/api/orgs/${B.orgId}/sms/messages/${msgB.json?.id}`, {
    token: A.token,
  })
  await recusado('A lê a mensagem de B pelo caminho da SUA org', `/api/orgs/${A.orgId}/sms/messages/${msgB.json?.id}`, {
    token: A.token,
  })

  // Escrita cross-tenant.
  await recusado('A envia SMS pela org B', `/api/orgs/${B.orgId}/sms/messages`, {
    token: A.token, method: 'POST', body: { to: '923000002', body: 'fraude' },
  })
  await recusado('A muda a rota da org B', `/api/orgs/${B.orgId}/sms/route`, {
    token: A.token, method: 'PUT', body: { device_id: null },
  })
  await recusado('A selecciona o telefone de B como rota da SUA org', `/api/orgs/${A.orgId}/sms/route`, {
    token: A.token, method: 'PUT', body: { device_id: devB?.id },
  })
  await recusado('A revoga o gateway da org B', `/api/orgs/${B.orgId}/sms/gateways/${gwB.json.id}`, {
    token: A.token, method: 'DELETE',
  })
  const aindaLa = (await req(`/api/orgs/${B.orgId}/sms/gateways`, { token: B.token })).json ?? []
  if (aindaLa.some((g) => g.id === gwB.json.id)) ok('o gateway de B sobreviveu à tentativa de A')
  else nok('o gateway de B sobreviveu à tentativa de A', JSON.stringify(aindaLa))
  const rotaB = (await req(`/api/orgs/${B.orgId}/sms/route`, { token: B.token })).json
  if (rotaB?.device_id && rotaB.device_id === devB?.id) ok('a rota de B continua no telefone de B')
  else nok('a rota de B continua no telefone de B', JSON.stringify(rotaB))

  // O agente de A contra a fila de B.
  const claimA = await req('/api/sms/agent/claim', { token: gwA.json.token, method: 'POST' })
  if (claimA.status === 200 && (claimA.json?.messages ?? []).every((m) => m.id !== msgB.json?.id)) {
    ok('o gateway de A não reclama a mensagem de B')
  } else nok('o gateway de A não reclama a mensagem de B', `${claimA.status} ${JSON.stringify(claimA.json)}`)
  await recusado('o gateway de A reporta resultado da mensagem de B', `/api/sms/agent/messages/${msgB.json?.id}/result`, {
    token: gwA.json.token, method: 'POST', body: { ok: true },
  })

  // Credenciais que NÃO são de gateway.
  await recusado('sessão de B na superfície do agente', '/api/sms/agent/claim', { token: B.token, method: 'POST' })
  await recusado('chave dlx_ na superfície do agente', '/api/sms/agent/claim', { token: chaveA.json?.key, method: 'POST' })
  await recusado('anónimo na superfície do agente', '/api/sms/agent/claim', { method: 'POST' })
  await recusado('token dlxg_ inventado', '/api/sms/agent/claim', { token: `dlxg_${'0'.repeat(64)}`, method: 'POST' })

  // Controlo positivo: o agente de B reclama e confirma a SUA mensagem.
  const claimB = await req('/api/sms/agent/claim', { token: gwB.json.token, method: 'POST' })
  const reclamada = (claimB.json?.messages ?? []).find((m) => m.id === msgB.json?.id)
  if (reclamada?.pdus?.length === 1 && reclamada.device_key === modemFalso('B').device_key) {
    ok('controlo: o gateway de B reclama a sua mensagem, com o PDU feito')
  } else nok('controlo: o gateway de B reclama a sua mensagem, com o PDU feito', JSON.stringify(claimB.json))
  const res = await req(`/api/sms/agent/messages/${msgB.json?.id}/result`, {
    token: gwB.json.token, method: 'POST', body: { ok: true, provider_ref: '42' },
  })
  const final = (await req(`/api/orgs/${B.orgId}/sms/messages/${msgB.json?.id}`, { token: B.token })).json
  if (res.status === 204 && final?.status === 'sent') ok('controlo: o resultado de B fica gravado (sent)')
  else nok('controlo: o resultado de B fica gravado (sent)', `${res.status} ${JSON.stringify(final)}`)

  // Revogar corta o agente.
  await permitido('B revoga o seu gateway', `/api/orgs/${B.orgId}/sms/gateways/${gwB.json.id}`, {
    token: B.token, method: 'DELETE',
  })
  await recusado('o token revogado de B deixa de servir', '/api/sms/agent/claim', { token: gwB.json.token, method: 'POST' })
}

// ─────────────────────────────────────────────────────────────────────────────
// CONVIDADO SEM CONTA (`POST /api/rooms/{code}/guest-join`, server/src/guests.rs).
//
// É a única rota PÚBLICA que dá acesso a uma reunião, por isso a metade que
// interessa é a negativa: o token que o convidado recebe à porta não abre mais
// nada da API, não passa a sala de espera sozinho, e a sala pode recusá-lo.
//
// Cada pedido leva um `X-Forwarded-For` de TESTE diferente (198.18.0.0/15, rede
// de benchmark). O servidor só confia nesse cabeçalho vindo de um proxy local —
// que é o caso de quem corre isto contra 127.0.0.1 — e assim o travão por IP
// deste ficheiro não consome a quota do IP real do runner, que o `convidado.mjs`
// usa a seguir.
// ─────────────────────────────────────────────────────────────────────────────
console.log('\n--- convidado sem conta: a porta, e o que ela NÃO abre ---')
let ipSeq = 0
const ipDeTeste = () => `198.18.${Math.floor(Math.random() * 250)}.${(ipSeq++ % 250) + 1}`

async function guestJoin(code, body, ip = ipDeTeste()) {
  const r = await fetch(`${API}/api/rooms/${code}/guest-join`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', 'X-Forwarded-For': ip },
    body: JSON.stringify(body),
  })
  let json = null
  try { json = await r.json() } catch { /* sem corpo */ }
  return { status: r.status, json, retryAfter: r.headers.get('retry-after') }
}

const salaG = (await req('/api/rooms', {
  token: B.token, method: 'POST', body: { name: 'sala da B com externos', topology: 'sfu' },
})).json
if (salaG?.allow_guests === true) ok('uma sala nova aceita convidados por omissão (allow_guests = true)')
else nok('uma sala nova aceita convidados por omissão', JSON.stringify(salaG))

const gj = await guestJoin(salaG.code, { display_name: 'Visitante Externo' })
if (gj.status === 200 && gj.json?.room_token && gj.json?.ice_servers?.iceServers) {
  ok('controlo positivo: um anónimo com nome recebe token de sala e ICE → 200')
} else {
  nok('controlo positivo: um anónimo recebe token de sala', `${gj.status} ${JSON.stringify(gj.json).slice(0, 160)}`)
}
const gTok = gj.json?.room_token
if (gj.json?.room && gj.json.room.owner_id === undefined) ok('a vista do convidado não traz o dono da sala')
else nok('a vista do convidado não traz o dono da sala', JSON.stringify(gj.json?.room))
const payload = gTok ? JSON.parse(Buffer.from(gTok.split('.')[1], 'base64url').toString()) : {}
if (payload.origin === 'guest' && payload.wait === true && !payload.owner && !payload.adm && payload.sub !== B.userId) {
  ok('o token é de CONVIDADO: origin=guest, wait, sem owner/adm, sub gerado')
} else nok('o token é de CONVIDADO', JSON.stringify(payload))
if (payload.exp - payload.iat <= 600) ok(`o token é efémero (${payload.exp - payload.iat}s)`)
else nok('o token é efémero', `${payload.exp - payload.iat}s`)

// O token do convidado contra a API autenticada. Nenhuma destas pode passar da
// porta (só 401/403/404): um 400 queria dizer que o handler correu.
if (gTok) {
  const G = { token: gTok }
  await recusadoNaPorta('convidado lê o CHAT guardado da sala', `/api/rooms/${salaG.code}/chat`, G)
  await recusadoNaPorta('convidado lista as GRAVAÇÕES da sala', `/api/rooms/${salaG.code}/recordings`, G)
  await recusadoNaPorta('convidado lê as NOTAS da sala', `/api/rooms/${salaG.code}/notes`, G)
  await recusadoNaPorta('convidado escreve a ACTA da sala', `/api/rooms/${salaG.code}/minutes`, {
    ...G, method: 'POST', body: { minutes: 'forjada', transcript: '' },
  })
  await recusadoNaPorta('convidado lê a biblioteca de gravações', '/api/recordings', G)
  await recusadoNaPorta('convidado lista QUADROS guardados', '/api/whiteboards', G)
  await recusadoNaPorta('convidado lista reuniões', '/api/meetings', G)
  await recusadoNaPorta('convidado CONVIDA gente para a sala', `/api/rooms/${salaG.code}/invite`, {
    ...G, method: 'POST', body: { targets: [B.userId] },
  })
  await recusadoNaPorta('convidado pede um token de MEMBRO (/join)', `/api/rooms/${salaG.code}/join`, { ...G, method: 'POST' })
  await recusadoNaPorta('convidado lê o próprio «perfil»', '/api/users/me', G)
  await recusadoNaPorta('convidado lê as orgs', '/api/orgs', G)
  await recusadoNaPorta('convidado lê a auditoria da org B', `/api/orgs/${B.orgId}/audit`, G)
  await recusadoNaPorta('convidado muda a política de convidados da sala', `/api/rooms/${salaG.code}`, {
    ...G, method: 'PATCH', body: { allow_guests: false },
  })
  await recusadoNaPorta('convidado reporta QoS', `/api/rooms/${salaG.code}/qos`, {
    ...G, method: 'POST', body: { rtt_ms: 1, loss_pct: 0, up_kbps: 1 },
  })
}

// Sinalização: o convidado cai na SALA DE ESPERA; o dono entra directo e VÊ-O
// como convidado.
async function abrirWs(roomToken) {
  const ws = new WebSocket(`${WS}/ws?token=${encodeURIComponent(roomToken)}`)
  const msgs = []
  ws.on('message', (d) => { try { msgs.push(JSON.parse(d.toString())) } catch { /* binário */ } })
  await new Promise((res, rej) => { ws.on('open', res); ws.on('error', rej) })
  const esperar = (pred, ms = 8000) => new Promise((resolve) => {
    const t0 = Date.now()
    const tick = () => {
      const m = msgs.find(pred)
      if (m) return resolve(m)
      if (Date.now() - t0 > ms) return resolve(null)
      setTimeout(tick, 50)
    }
    tick()
  })
  return { ws, msgs, esperar }
}
if (gTok) {
  const conv = await abrirWs(gTok)
  const esp = await conv.esperar((m) => ['waiting', 'joined'].includes(m.type))
  if (esp?.type === 'waiting') ok('o convidado cai na SALA DE ESPERA (sem media)')
  else nok('o convidado cai na sala de espera', `primeiro veredicto: ${JSON.stringify(esp)} — "joined" é entrada directa de um anónimo`)

  const joinDono = await req(`/api/rooms/${salaG.code}/join`, { token: B.token, method: 'POST' })
  const dono = await abrirWs(joinDono.json.room_token)
  const vistoPeloDono = await dono.esperar((m) => m.type === 'waiting-join')
  if (vistoPeloDono?.peer?.is_guest === true && vistoPeloDono.peer.username === 'Visitante Externo') {
    ok('o anfitrião vê-o na espera MARCADO como convidado')
  } else nok('o anfitrião vê-o marcado como convidado', JSON.stringify(vistoPeloDono))
  // Sem ninguém a admitir, o convidado continua à porta.
  await new Promise((r) => setTimeout(r, 1500))
  if (!conv.msgs.some((m) => m.type === 'joined')) ok('sem admissão, o convidado NÃO passa a espera sozinho')
  else nok('sem admissão, o convidado não passa a espera', 'recebeu "joined"')
  conv.ws.close(); dono.ws.close()

  // O directo (emissão para fora) recusa um token de convidado ANTES do upgrade.
  const directo = await new Promise((resolve) => {
    const ws = new WebSocket(`${WS}/api/rooms/${salaG.code}/broadcast?token=${encodeURIComponent(gTok)}&codec=h264&destinos=[]`)
    ws.on('unexpected-response', (_q, res) => resolve(res.statusCode))
    ws.on('open', () => { ws.close(); resolve('aberto') })
    ws.on('error', () => resolve('erro'))
  })
  if (directo === 403) ok('convidado NÃO abre o directo da sala → 403')
  else nok('convidado não abre o directo da sala', `respondeu ${directo}`)
}

// A sala RECUSA convidados.
await recusado('A (outra org) desliga os convidados na sala da B', `/api/rooms/${salaG.code}`, {
  token: A.token, method: 'PATCH', body: { allow_guests: false },
})
const fechar = await req(`/api/rooms/${salaG.code}`, { token: B.token, method: 'PATCH', body: { allow_guests: false } })
if (fechar.status === 200 && fechar.json?.allow_guests === false) ok('o dono desliga os convidados na sua sala → 200')
else nok('o dono desliga os convidados', `${fechar.status} ${JSON.stringify(fechar.json)}`)
const fechada = await guestJoin(salaG.code, { display_name: 'Visitante Externo' })
if (fechada.status === 403 && !fechada.json?.room_token) ok('sala sem convidados → 403, sem token')
else nok('sala sem convidados → 403', `${fechada.status} ${JSON.stringify(fechada.json).slice(0, 120)}`)
const reaberta = await req(`/api/rooms/${salaG.code}`, { token: B.token, method: 'PATCH', body: { allow_guests: true } })
const denovo = await guestJoin(salaG.code, { display_name: 'Visitante Externo' })
if (reaberta.status === 200 && denovo.status === 200) ok('controlo: religada, a mesma entrada volta a dar 200')
else nok('controlo: religada, volta a dar 200', `${reaberta.status}/${denovo.status}`)
const campoInventado = await req(`/api/rooms/${salaG.code}`, { token: B.token, method: 'PATCH', body: { allow_guests: true, name: 'x' } })
if (campoInventado.status >= 400 && campoInventado.status < 500) ok(`PATCH com campo que o servidor não conhece é recusado → ${campoInventado.status}`)
else nok('PATCH com campo desconhecido é recusado', `${campoInventado.status}`)
const criadaFechada = (await req('/api/rooms', {
  token: B.token, method: 'POST', body: { name: 'só internos', topology: 'sfu', allow_guests: false },
})).json
const naCriada = await guestJoin(criadaFechada.code, { display_name: 'Visitante' })
if (criadaFechada.allow_guests === false && naCriada.status === 403) ok('criada com allow_guests=false → 403 ao convidado')
else nok('criada com allow_guests=false → 403', `${criadaFechada.allow_guests} / ${naCriada.status}`)

// Forma do pedido.
const inexistente = await guestJoin('zzz-zzzz-zzz', { display_name: 'Visitante' })
if (inexistente.status === 404) ok('código que não existe → 404')
else nok('código que não existe → 404', `${inexistente.status}`)
for (const [nome, corpo] of [
  ['nome vazio', { display_name: '   ' }],
  ['nome com 61 caracteres', { display_name: 'a'.repeat(61) }],
  ['nome com quebra de linha', { display_name: 'Ana\nAdmin' }],
  ['nome com inversão de direcção (U+202E)', { display_name: '‮nimda' }],
]) {
  const r = await guestJoin(salaG.code, corpo)
  if (r.status === 400 && !r.json?.room_token) ok(`${nome} → 400`)
  else nok(`${nome} → 400`, `${r.status} ${JSON.stringify(r.json).slice(0, 100)}`)
}
const extra = await guestJoin(salaG.code, { display_name: 'Ana', owner: true })
if (extra.status >= 400 && extra.status < 500 && !extra.json?.room_token) ok(`campo a mais no corpo (owner) é recusado → ${extra.status}`)
else nok('campo a mais no corpo é recusado', `${extra.status}`)

// Travão por IP: o mesmo IP esgota a janela e leva 429 com Retry-After; outro
// IP continua a entrar.
{
  const ip = ipDeTeste()
  let travado = null
  for (let i = 0; i < 1100 && !travado; i++) {
    const r = await guestJoin(salaG.code, { display_name: 'Rajada' }, ip)
    if (r.status === 429) travado = r
    else if (r.status !== 200) { nok('travão por IP', `pedido ${i} devolveu ${r.status}`); break }
  }
  if (travado && Number(travado.retryAfter) > 0) ok(`travão por IP → 429 com Retry-After: ${travado.retryAfter}`)
  else nok('travão por IP → 429 com Retry-After', JSON.stringify(travado))
}
// Travão por sala: muitos IPs diferentes contra UMA sala. Numa sala nova, para
// o travão de cima (que já gastou quota da salaG) não contaminar a contagem.
{
  const salaR = (await req('/api/rooms', { token: B.token, method: 'POST', body: { name: 'rajada', topology: 'sfu' } })).json
  let travado = null
  let n = 0
  for (; n < 1100 && !travado; n++) {
    const r = await guestJoin(salaR.code, { display_name: 'Rajada' })
    if (r.status === 429) travado = r
    else if (r.status !== 200) { nok('travão por sala', `pedido ${n} devolveu ${r.status}`); break }
  }
  if (travado && Number(travado.retryAfter) > 0) ok(`travão por SALA com IPs sempre diferentes → 429 ao fim de ${n} (Retry-After: ${travado.retryAfter})`)
  else nok('travão por sala → 429', JSON.stringify(travado))
  const outraSala = await guestJoin(salaG.code, { display_name: 'Outro' })
  if (outraSala.status === 200) ok('controlo: outra sala continua a aceitar')
  else nok('controlo: outra sala continua a aceitar', `${outraSala.status}`)
}

// Auditoria: a entrada fica na trilha da org do DONO, com a sala e o nome.
{
  const trilha = await req(`/api/orgs/${B.orgId}/audit?limit=500`, { token: B.token })
  const linha = (trilha.json ?? []).find((e) => e.action === 'room.guest_join' && e.target === salaG.code
    && /Visitante Externo/.test(e.actor))
  if (linha && /convidado/.test(linha.actor)) {
    ok(`auditoria: room.guest_join na org B com a sala e o nome («${linha.actor}»)`)
  } else nok('auditoria: room.guest_join na org B', JSON.stringify((trilha.json ?? []).slice(0, 3)))
  const trilhaA = await req(`/api/orgs/${A.orgId}/audit?limit=500`, { token: A.token })
  if (!(trilhaA.json ?? []).some((e) => e.action === 'room.guest_join' && e.target === salaG.code)) {
    ok('e NÃO aparece na trilha de outra org')
  } else nok('e não aparece na trilha de outra org', 'apareceu na org A')
}

console.log('\n--- sem autenticação nenhuma ---')
await recusado('anónimo lê stats da org B', `/api/orgs/${B.orgId}/stats`, {})
await recusado('anónimo lista as suas orgs', '/api/orgs', {})
await recusado('anónimo lista gravações', '/api/recordings', {})
await recusado('anónimo lê o próprio perfil', '/api/users/me', {})

console.log('\n--- token adulterado ---')
const [h, p] = A.token.split('.')
await recusado('assinatura trocada', '/api/users/me', { token: `${h}.${p}.assinaturaFalsa` })
await recusado('token vazio', '/api/users/me', { token: '' })
await recusado('lixo por token', '/api/users/me', { token: 'nao-e-um-jwt' })

console.log(`\n=== ${passou} passaram, ${falhou} falharam ===`)
if (falhou) {
  console.log('\nFALHAS:')
  for (const f of falhas) console.log(` • ${f.nome}\n   ${f.detalhe}`)
}
process.exit(falhou ? 1 : 0)
