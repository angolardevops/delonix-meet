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
  // 405 é o ROUTER a dizer que o método não existe naquele caminho — não prova
  // nada sobre autorização. Com a reorganização das rotas (POST→PUT/PATCH) um
  // caso com o método antigo passava a verde por engano; agora falha.
  if (status === 405) {
    nok(nome, `405: o método ${opts?.method ?? 'GET'} não existe em ${path} — o caso mede o router, não a regra`)
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
await recusado('A lista empregados da org B', `/api/orgs/${B.orgId}/members`, { token: A.token })
await recusado('A lista filiais da org B', `/api/orgs/${B.orgId}/branches`, { token: A.token })
await recusado('A lista grupos da org B', `/api/orgs/${B.orgId}/groups`, { token: A.token })
await recusado('A lista salas presenciais da org B', `/api/orgs/${B.orgId}/meeting-rooms`, { token: A.token })
await recusado('A lista webhooks da org B', `/api/orgs/${B.orgId}/webhooks`, { token: A.token })
await recusado('A lista chaves de API da org B', `/api/orgs/${B.orgId}/api-keys`, { token: A.token })
await recusado('A lê a config Odoo da org B', `/api/orgs/${B.orgId}/integrations/odoo`, { token: A.token })
await recusado('A lista DIDs de voz da org B', `/api/orgs/${B.orgId}/voice/dids`, { token: A.token })
await recusado('A lista CDR de voz da org B', `/api/orgs/${B.orgId}/voice/call-records`, { token: A.token })

console.log('\n--- escrita cross-tenant ---')
await recusado('A lê a org B', `/api/orgs/${B.orgId}`, { token: A.token })
await recusado('A altera definições da org B', `/api/orgs/${B.orgId}`, {
  token: A.token, method: 'PATCH', body: { hide_org_creation: true },
})
await recusado('A lê a análise de quarentena da org B', `/api/orgs/${B.orgId}/analytics/quarantine`, { token: A.token })
await recusado('A cria uma sala de voz na org B', `/api/orgs/${B.orgId}/voice/rooms`, {
  token: A.token, method: 'POST', body: { room_code: salaB.code },
})
await recusado('A roda o token Odoo da org B', `/api/orgs/${B.orgId}/integrations/odoo/rotate-token`, {
  token: A.token, method: 'POST', body: {},
})
await recusado('A remove um empregado da org B', `/api/orgs/${B.orgId}/members/${B.userId}`, {
  token: A.token, method: 'DELETE',
})

// As seis rotas com escopo de organização que este teste NÃO cobria (R95).
// Encontradas a comparar o inventário do que EXISTE (`grep` às rotas do
// `main.rs`) com o inventário do que se TESTA — o mesmo método que apanhou os
// testes ponta-a-ponta que nunca corriam (R72).
console.log('\n--- as seis que faltavam ---')
await recusado('A lê a trilha de auditoria da org B', `/api/orgs/${B.orgId}/audit-events`, { token: A.token })
await recusado('A verifica a cadeia de auditoria da org B', `/api/orgs/${B.orgId}/audit-events/verification`, {
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

// Armazenamento usado e quota (G3): volume e quota de outra empresa.
await recusado('A lê o armazenamento da org B', `/api/orgs/${B.orgId}/storage-usage`, { token: A.token })

// Destinos de emissão guardados (G1): a chave RTMP é credencial de terceiros.
await recusado('A lista destinos de emissão da org B', `/api/orgs/${B.orgId}/stream-destinations`, { token: A.token })
const destinoB = await req(`/api/orgs/${B.orgId}/stream-destinations`, {
  token: B.token, method: 'POST',
  body: { kind: 'rtmp', label: 'Destino da B', url: 'rtmp://10.0.0.9/live', stream_key: 'chave-da-b' },
})
if (destinoB.status === 201 && destinoB.json?.id) {
  const d = `/api/orgs/${B.orgId}/stream-destinations/${destinoB.json.id}`
  await recusado('A lê um destino da org B', d, { token: A.token })
  await recusado('A roda a chave de um destino da org B', `/api/orgs/${B.orgId}/stream-destinations/${destinoB.json.id}/rotate-key`, {
    token: A.token, method: 'POST', body: { stream_key: 'roubada' },
  })
  await recusado('A apaga um destino da org B', d, { token: A.token, method: 'DELETE' })
  const ainda = await req(d, { token: B.token })
  if (ainda.status === 200 && ainda.json?.key_prefix === 'chav') ok('e o destino da B CONTINUA LÁ, com a chave dela')
  else nok('e o destino da B CONTINUA LÁ, com a chave dela', `devolveu ${ainda.status}: ${JSON.stringify(ainda.json).slice(0, 120)}`)
} else {
  nok('B cria um destino de emissão para o teste', `devolveu ${destinoB.status}: ${JSON.stringify(destinoB.json).slice(0, 120)}`)
}

const hookB = await req(`/api/orgs/${B.orgId}/webhooks`, {
  token: B.token, method: 'POST',
  body: { kind: 'generic', url: 'https://example.com/hook', secret: 's3cr3t-de-teste' },
})
if (hookB.status >= 200 && hookB.status < 300 && hookB.json?.id) {
  await recusado('A apaga um webhook da org B', `/api/orgs/${B.orgId}/webhooks/${hookB.json.id}`, {
    token: A.token, method: 'DELETE',
  })
  // Registo de entregas e reenvio (G7): o payload traz dados de reuniões da B.
  await recusado('A lê um webhook da org B', `/api/orgs/${B.orgId}/webhooks/${hookB.json.id}`, { token: A.token })
  await recusado('A lista as entregas de um webhook da org B', `/api/orgs/${B.orgId}/webhooks/${hookB.json.id}/deliveries`, { token: A.token })
  // Uma entrega A SÉRIO da B: uma reunião dispara `meeting.created`, e a linha
  // fica registada mesmo que o envio a example.com falhe. Sem ela, um 404 só
  // provaria que a linha não existe, não que a org é recusada.
  await req('/api/meetings', {
    token: B.token, method: 'POST',
    body: { title: 'Dispara o webhook da B', kind: 'video', starts_at: new Date(Date.now() + 3600e3).toISOString(), duration_min: 30, invitee_ids: [] },
  })
  let entregaB = null
  for (let i = 0; i < 50 && !entregaB; i++) {
    const l = await req(`/api/orgs/${B.orgId}/webhooks/${hookB.json.id}/deliveries`, { token: B.token })
    entregaB = l.json?.items?.[0]?.id ?? null
    if (!entregaB) await new Promise((r) => setTimeout(r, 100))
  }
  if (entregaB) ok('B vê a entrega do seu webhook')
  else nok('B vê a entrega do seu webhook', 'nenhuma entrega registada em 5 s')
  for (const entregaId of [entregaB, '00000000-0000-4000-8000-000000000000'].filter(Boolean)) {
    await recusado('A lê uma entrega de webhook da org B', `/api/orgs/${B.orgId}/webhooks/${hookB.json.id}/deliveries/${entregaId}`, { token: A.token })
    await recusado('A reenvia uma entrega de webhook da org B', `/api/orgs/${B.orgId}/webhooks/${hookB.json.id}/deliveries/${entregaId}/redeliver`, {
      token: A.token, method: 'POST',
    })
  }
  const entregas = await req(`/api/orgs/${B.orgId}/webhooks/${hookB.json.id}/deliveries`, { token: B.token })
  if (Array.isArray(entregas.json?.items) && entregas.json.items.every((d) => d.redelivery_of == null)) ok('e nenhum reenvio da A chegou a criar entrega na B')
  else nok('e nenhum reenvio da A chegou a criar entrega na B', `devolveu ${entregas.status}: ${JSON.stringify(entregas.json).slice(0, 160)}`)
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
await recusado('A lê o chat da sala da B', `/api/rooms/${salaB.code}/messages`, { token: A.token })
await recusado('A lista gravações da sala da B', `/api/rooms/${salaB.code}/recordings`, { token: A.token })
await recusado('A lê notas da sala da B', `/api/rooms/${salaB.code}/minutes`, { token: A.token })
await recusado('A reporta QoS na sala da B', `/api/rooms/${salaB.code}/quality-samples`, {
  token: A.token, method: 'POST', body: { rtt_ms: 1, loss_pct: 0, up_kbps: 1 },
})
await recusado('A espreita a sala de espera da B (não admite)', `/api/rooms/${salaB.code}/waiting`, { token: A.token })
await recusado('anónimo vê metadados da sala da B', `/api/rooms/${salaB.code}`, {})
await recusado('anónimo vê o estado do directo da sala da B', `/api/rooms/${salaB.code}/live/status`, {})

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
  // Um 405 (método inexistente) já não conta como recusa — ver `recusado`.
  await recusado('A lê a reunião da org B', `/api/meetings/${m}`, { token: A.token })
  await recusado('A APAGA a reunião da org B', `/api/meetings/${m}`, {
    token: A.token, method: 'DELETE',
  })
  await recusado('A escreve a ACTA da reunião da B', `/api/meetings/${m}/minutes`, {
    token: A.token, method: 'PUT', body: { markdown: 'acta forjada' },
  })
  await recusado('A lê a agenda da reunião da B', `/api/meetings/${m}/agenda-items`, { token: A.token })
  await recusado('A lê os convidados da reunião da B', `/api/meetings/${m}/invitees`, { token: A.token })
  await recusado('A lê o plano de acção da reunião da B', `/api/meetings/${m}/action-plan`, { token: A.token })
  await recusado('A descarrega o ICS da reunião da B', `/api/meetings/${m}/calendar.ics`, { token: A.token })
  await recusado('A responde ao convite da reunião da B', `/api/meetings/${m}/invitees/me`, {
    token: A.token, method: 'PUT', body: { status: 'accepted' },
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
  await recusado('A descarrega o PNG do quadro da B', `/api/whiteboards/${q}/image`, { token: A.token })
  // URL assinado (G11): quem não vê o quadro não o emite, e uma assinatura
  // inventada não abre o PNG — nem com a sessão de A.
  await recusado('A emite um URL assinado do quadro da B', `/api/whiteboards/${q}/signed-url`, {
    token: A.token, method: 'POST',
  })
  await recusado('A forja um URL assinado do quadro da B', `/api/whiteboards/${q}/image?exp=${Math.floor(Date.now() / 1000) + 600}&sig=${'0'.repeat(64)}`, {
    token: A.token,
  })
  await recusado('A lê os metadados do quadro da B', `/api/whiteboards/${q}`, { token: A.token })
  await recusado('A PARTILHA o quadro da B por link', `/api/whiteboards/${q}/public-link`, {
    token: A.token, method: 'PUT', body: { public: true },
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
await recusado('A convida gente para a sala da B', `/api/rooms/${salaB.code}/invitations`, {
  token: A.token, method: 'POST', body: { user_ids: [] },
})
await recusado('A lê a acta da sala da B', `/api/rooms/${salaB.code}/minutes`, { token: A.token })
await recusado('A escreve a acta da sala da B', `/api/rooms/${salaB.code}/minutes`, {
  token: A.token, method: 'PUT', body: { markdown: 'acta forjada' },
})
await recusado('A reporta tempos de chamada na sala da B', `/api/rooms/${salaB.code}/join-timings`, {
  token: A.token, method: 'POST', body: { join_ms: 1 },
})
await recusado('A partilha uma gravação alheia com alguém', `/api/recordings/${inventado}/shares`, {
  token: A.token, method: 'POST', body: { user_id: inventado },
})
await recusado('A revoga a partilha de uma gravação alheia', `/api/recordings/${inventado}/shares/${inventado}`, {
  token: A.token, method: 'DELETE',
})

// As gravações não se podem FABRICAR sem uma chamada a sério, por isso o que
// aqui se prova é a forma da recusa com um id que a org A inventa. É menos do
// que o resto deste ficheiro e está dito: um `404` aqui não distingue «não é
// tua» de «não existe». O caminho por id fica coberto pela sala
// (`/api/rooms/{code}/recordings`, acima), que usa um id REAL da org B.
await recusado('A descarrega uma gravação por id inventado', `/api/recordings/${inventado}/content`, {
  token: A.token,
})
await recusado('A cria link de partilha de uma gravação alheia', `/api/recordings/${inventado}/public-link`, {
  token: A.token, method: 'PUT', body: {},
})
await recusado('A mexe num item de acção por id inventado', `/api/meetings/${inventado}/action-plan/items/${inventado}`, {
  token: A.token, method: 'PATCH', body: { status: 'done' },
})
await recusado('A lê uma sala de voz da org B', `/api/orgs/${B.orgId}/voice/rooms/${inventado}`, { token: A.token })
await recusado('A lê os participantes de uma sala de voz da org B', `/api/orgs/${B.orgId}/voice/rooms/${inventado}/participants`, { token: A.token })
await recusado('A encerra uma sala de voz da org B', `/api/orgs/${B.orgId}/voice/rooms/${inventado}/close`, {
  token: A.token, method: 'POST', body: {},
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
await recusadoNaPorta('admin de org recém-registado LÊ o armazenamento da plataforma', '/api/operator/v1/storage', {
  token: A.token,
})
await recusadoNaPorta('admin de org recém-registado ESCREVE o armazenamento da plataforma', '/api/operator/v1/storage', {
  token: A.token, method: 'PUT',
  body: { storage_type: 'webdav', webdav_url: 'http://169.254.169.254/', webdav_user: 'x' },
})
await recusadoNaPorta('admin de org recém-registado dispara o teste de ligação (PROPFIND)', '/api/operator/v1/storage/test', {
  token: A.token, method: 'POST', body: {},
})

console.log('\n--- S2: a sincronização Odoo não captura contas de outra organização ---')
// A org A emite o seu token de integração `dlxo_` e lista no seu «directório»
// o endereço do administrador da org B. Antes: a conta de B era reescrita
// (nome, `odoo_managed`) e entrava na org A como admin. (Até ao R142 usava-se a
// chave `dlx_`, que o extractor do Odoo aceitava; já não aceita.)
const chaveA = await req(`/api/orgs/${A.orgId}/integrations/odoo/rotate-token`, {
  token: A.token, method: 'POST', body: {},
})
if (chaveA.status >= 200 && chaveA.status < 300 && chaveA.json?.token) {
  const novoEmail = `novo-${marca}@alfa${marca}.local`
  const prov = await req('/api/integrations/odoo/v1/provision', {
    token: chaveA.json.token, method: 'POST',
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

  const empA = await req(`/api/orgs/${A.orgId}/members`, { token: A.token })
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
  nok('A emite o token de integração Odoo para o teste S2', `devolveu ${chaveA.status}`)
}

console.log('\n--- S3: um membro ARQUIVADO perde o acesso da organização ---')
// Dois colaboradores da org A: C (membro) e D (admin). Cada acesso é provado
// ANTES de arquivar (controlo positivo) e recusado DEPOIS. Sem o «antes», um
// «depois» recusado podia ser só o recurso a não existir.
const dominioA = A.email.split('@')[1]
async function colaborador(nome, role) {
  const email = `${nome}-${marca}@${dominioA}`
  const r = await req(`/api/orgs/${A.orgId}/members`, {
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

console.log('\n--- G4–G6: metadados, capítulos e comentários de uma gravação REAL da A ---')
// Aqui o id é verdadeiro (a gravação acabou de ser carregada), por isso um
// `404` para a B quer mesmo dizer «não é tua» — o controlo positivo é a A.
if (gravacaoA) {
  const capA = await permitido('A cria um capítulo na sua gravação', `/api/recordings/${gravacaoA}/chapters`, {
    token: A.token, method: 'POST', body: { at_secs: 0, title: 'abertura' },
  })
  const comA = await permitido('A comenta a sua gravação', `/api/recordings/${gravacaoA}/comments`, {
    token: A.token, method: 'POST', body: { body: 'comentário privado da A' },
  })
  await permitido('A lê os metadados da sua gravação', `/api/recordings/${gravacaoA}`, { token: A.token })
  await recusado('B lê os metadados da gravação da A', `/api/recordings/${gravacaoA}`, { token: B.token })
  await recusado('B muda a categoria da gravação da A', `/api/recordings/${gravacaoA}`, {
    token: B.token, method: 'PATCH', body: { category: 'other' },
  })
  await recusado('B lê os capítulos da gravação da A', `/api/recordings/${gravacaoA}/chapters`, { token: B.token })
  await recusado('B cria um capítulo na gravação da A', `/api/recordings/${gravacaoA}/chapters`, {
    token: B.token, method: 'POST', body: { at_secs: 1, title: 'forjado' },
  })
  await recusado('B lê os comentários da gravação da A', `/api/recordings/${gravacaoA}/comments`, { token: B.token })
  await recusado('B comenta a gravação da A', `/api/recordings/${gravacaoA}/comments`, {
    token: B.token, method: 'POST', body: { body: 'forjado' },
  })
  if (capA?.id) {
    await recusado('B lê um capítulo da A', `/api/recordings/${gravacaoA}/chapters/${capA.id}`, { token: B.token })
    await recusado('B apaga um capítulo da A', `/api/recordings/${gravacaoA}/chapters/${capA.id}`, {
      token: B.token, method: 'DELETE',
    })
  }
  if (comA?.id) {
    await recusado('B lê um comentário da A', `/api/recordings/${gravacaoA}/comments/${comA.id}`, { token: B.token })
    await recusado('B edita um comentário da A', `/api/recordings/${gravacaoA}/comments/${comA.id}`, {
      token: B.token, method: 'PATCH', body: { body: 'forjado' },
    })
    await recusado('B apaga um comentário da A', `/api/recordings/${gravacaoA}/comments/${comA.id}`, {
      token: B.token, method: 'DELETE',
    })
    const ainda = await req(`/api/recordings/${gravacaoA}/comments/${comA.id}`, { token: A.token })
    if (ainda.status === 200 && ainda.json?.body === 'comentário privado da A') ok('o comentário da A continua intacto')
    else nok('o comentário da A continua intacto', `devolveu ${ainda.status}: ${JSON.stringify(ainda.json).slice(0, 160)}`)
  }
  const pesquisaB = await req('/api/recordings?q=s3', { token: B.token })
  if (pesquisaB.status === 200 && !(pesquisaB.json?.items ?? []).some((r) => r.id === gravacaoA)) {
    ok('a pesquisa da B não devolve a gravação da A')
  } else {
    nok('a pesquisa da B não devolve a gravação da A', `devolveu ${pesquisaB.status}: ${JSON.stringify(pesquisaB.json).slice(0, 160)}`)
  }
}

const pesquisaPorA = async (token) => {
  const r = await req(`/api/users?q=${encodeURIComponent(`admin-alfa${marca}`)}`, { token })
  return Array.isArray(r.json) && r.json.some((u) => u.id === A.userId)
}

// ANTES
await permitido('C (membro activo) lê o chat da sala da A', `/api/rooms/${salaA.code}/messages`, { token: C.token })
if (await pesquisaPorA(C.token)) ok('C (membro activo) encontra o admin da A na pesquisa')
else nok('C (membro activo) encontra o admin da A na pesquisa', 'não encontrou — o controlo positivo falhou')
if (gravacaoA) {
  await permitido('D (admin activo) descarrega a gravação da A', `/api/recordings/${gravacaoA}/content?dl=1`, { token: D.token })
  await permitido('D (admin activo) lê os comentários da gravação da A', `/api/recordings/${gravacaoA}/comments`, { token: D.token })
}
// Chave de API `dlx_` da A, própria destes casos. A `chaveA` de cima passou a
// ser o token Odoo (`dlxo_`, R142), que a v1 de reuniões não aceita — sem esta,
// o controlo dava 401 e a recusa do caso ARQUIVADO passava por engano.
const chaveApiA = await req(`/api/orgs/${A.orgId}/api-keys`, {
  token: A.token, method: 'POST', body: { name: 's3-arquivo' },
})
await permitido('controlo: a chave da A cria reunião com C como anfitriã', '/api/v1/meetings', {
  token: chaveApiA.json?.key, method: 'POST',
  body: { title: 's3 antes', starts_at: new Date(Date.now() + 7200_000).toISOString(), host_email: C.email },
})

// ARQUIVAR
await permitido('A arquiva C', `/api/orgs/${A.orgId}/members/${C.userId}`, { token: A.token, method: 'DELETE' })
await permitido('A arquiva D', `/api/orgs/${A.orgId}/members/${D.userId}`, { token: A.token, method: 'DELETE' })

// DEPOIS
await recusado('C ARQUIVADA lê o chat da sala da A', `/api/rooms/${salaA.code}/messages`, { token: C.token })
if (!(await pesquisaPorA(C.token))) ok('C ARQUIVADA já não encontra o admin da A na pesquisa')
else nok('C ARQUIVADA já não encontra o admin da A na pesquisa', 'o directório da ex-organização continua visível')
if (gravacaoA) {
  await recusado('D ARQUIVADO descarrega a gravação da A', `/api/recordings/${gravacaoA}/content?dl=1`, { token: D.token })
  await recusado('D ARQUIVADO lê os comentários da gravação da A', `/api/recordings/${gravacaoA}/comments`, { token: D.token })
}
await recusado('a chave da A cria reunião com C ARQUIVADA como anfitriã', '/api/v1/meetings', {
  token: chaveApiA.json?.key, method: 'POST',
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
  await permitido('o agente de B reporta um modem', '/api/integrations/sms-agent/v1/devices', {
    token: gwB.json.token, method: 'PUT', body: { devices: [modemFalso('B')] },
  })
  await permitido('o agente de A reporta um modem', '/api/integrations/sms-agent/v1/devices', {
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
  const claimA = await req('/api/integrations/sms-agent/v1/claim', { token: gwA.json.token, method: 'POST' })
  if (claimA.status === 200 && (claimA.json?.messages ?? []).every((m) => m.id !== msgB.json?.id)) {
    ok('o gateway de A não reclama a mensagem de B')
  } else nok('o gateway de A não reclama a mensagem de B', `${claimA.status} ${JSON.stringify(claimA.json)}`)
  await recusado('o gateway de A reporta resultado da mensagem de B', `/api/integrations/sms-agent/v1/messages/${msgB.json?.id}/result`, {
    token: gwA.json.token, method: 'POST', body: { ok: true },
  })

  // Credenciais que NÃO são de gateway.
  await recusado('sessão de B na superfície do agente', '/api/integrations/sms-agent/v1/claim', { token: B.token, method: 'POST' })
  await recusado('chave dlx_ na superfície do agente', '/api/integrations/sms-agent/v1/claim', { token: chaveA.json?.key, method: 'POST' })
  await recusado('anónimo na superfície do agente', '/api/integrations/sms-agent/v1/claim', { method: 'POST' })
  await recusado('token dlxg_ inventado', '/api/integrations/sms-agent/v1/claim', { token: `dlxg_${'0'.repeat(64)}`, method: 'POST' })

  // Controlo positivo: o agente de B reclama e confirma a SUA mensagem.
  const claimB = await req('/api/integrations/sms-agent/v1/claim', { token: gwB.json.token, method: 'POST' })
  const reclamada = (claimB.json?.messages ?? []).find((m) => m.id === msgB.json?.id)
  if (reclamada?.pdus?.length === 1 && reclamada.device_key === modemFalso('B').device_key) {
    ok('controlo: o gateway de B reclama a sua mensagem, com o PDU feito')
  } else nok('controlo: o gateway de B reclama a sua mensagem, com o PDU feito', JSON.stringify(claimB.json))
  const res = await req(`/api/integrations/sms-agent/v1/messages/${msgB.json?.id}/result`, {
    token: gwB.json.token, method: 'POST', body: { ok: true, provider_ref: '42' },
  })
  const final = (await req(`/api/orgs/${B.orgId}/sms/messages/${msgB.json?.id}`, { token: B.token })).json
  if (res.status === 204 && final?.status === 'sent') ok('controlo: o resultado de B fica gravado (sent)')
  else nok('controlo: o resultado de B fica gravado (sent)', `${res.status} ${JSON.stringify(final)}`)

  // Revogar corta o agente.
  await permitido('B revoga o seu gateway', `/api/orgs/${B.orgId}/sms/gateways/${gwB.json.id}`, {
    token: B.token, method: 'DELETE',
  })
  await recusado('o token revogado de B deixa de servir', '/api/integrations/sms-agent/v1/claim', { token: gwB.json.token, method: 'POST' })
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
