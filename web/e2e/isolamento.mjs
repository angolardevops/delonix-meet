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

// IA local do Estúdio. O `suggestions` leva um corpo VÁLIDO: só 401/403/404 provam
// que a pertença foi decidida antes de o handler validar ou chamar o modelo (um
// 400/429/503 seria o handler a correr para quem não é da org).
console.log('\n--- IA local do Estúdio ---')
await permitido('B lê o estado da IA da própria org', `/api/orgs/${B.orgId}/ai/status`, { token: B.token })
await recusadoNaPorta('A lê o estado da IA da org B', `/api/orgs/${B.orgId}/ai/status`, { token: A.token })
await recusadoNaPorta('A usa a IA do Estúdio da org B', `/api/orgs/${B.orgId}/ai/suggestions`, {
  token: A.token,
  method: 'POST',
  body: {
    task: 'fillers',
    segments: [{ start_ms: 0, end_ms: 4000, text: 'Bom dia, tipo, vamos rever a rede de Luanda e o troço do Kilamba.' }],
  },
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
// Rotas de inquilino que o inventário (`check-isolamento-cobertura.sh`) apontou
// como não exercitadas: papéis/permissões (RBAC), convites e ramais. A A nunca
// as alcança na org B — ler, criar, alterar, apagar, decidir.
console.log('\n--- RBAC, convites e ramais da org B ---')
const fantasma = crypto.randomUUID()
await recusado('A lista os papéis da org B', `/api/orgs/${B.orgId}/roles`, { token: A.token })
await recusado('A cria um papel na org B', `/api/orgs/${B.orgId}/roles`, {
  token: A.token, method: 'POST', body: { name: 'intruso', permissions: [] },
})
await recusado('A altera um papel da org B', `/api/orgs/${B.orgId}/roles/${fantasma}`, {
  token: A.token, method: 'PATCH', body: { name: 'x' },
})
await recusado('A apaga um papel da org B', `/api/orgs/${B.orgId}/roles/${fantasma}`, { token: A.token, method: 'DELETE' })
await recusado('A duplica um papel da org B', `/api/orgs/${B.orgId}/roles/${fantasma}/duplicate`, {
  token: A.token, method: 'POST', body: {},
})
await recusado('A atribui um papel a um membro da org B', `/api/orgs/${B.orgId}/members/${B.userId}/role`, {
  token: A.token, method: 'PUT', body: { role_id: fantasma },
})
await recusado('A lista os ramais da org B', `/api/orgs/${B.orgId}/extensions`, { token: A.token })
await recusado('A cria um ramal na org B', `/api/orgs/${B.orgId}/extensions`, {
  token: A.token, method: 'POST', body: { extension: '9001', display_name: 'intruso' },
})
await recusado('A altera um ramal da org B', `/api/orgs/${B.orgId}/extensions/${fantasma}`, {
  token: A.token, method: 'PATCH', body: { display_name: 'x' },
})
await recusado('A apaga um ramal da org B', `/api/orgs/${B.orgId}/extensions/${fantasma}`, { token: A.token, method: 'DELETE' })
await recusado('A regenera a password de um ramal da org B', `/api/orgs/${B.orgId}/extensions/${fantasma}/regenerate-password`, {
  token: A.token, method: 'POST', body: {},
})
await recusado('A atribui um DID a um ramal da org B', `/api/orgs/${B.orgId}/extensions/${fantasma}/did`, {
  token: A.token, method: 'PUT', body: { did_id: fantasma },
})

// O segredo mais valioso desta família: com ele, qualquer um emite no canal
// de YouTube da empresa. A provar: A não alcança os destinos da B (ler, rodar
// a chave, alterar, apagar — e o de B sobrevive), a chave nunca volta em claro
// nem à própria B (fora da criação/rotação), e A não consegue EMITIR com um
// destino da B referindo-o por id.
console.log('\n--- destinos de directo guardados da org B ---')
await recusado('A lista destinos de emissão da org B', `/api/orgs/${B.orgId}/stream-destinations`, { token: A.token })
const CHAVE_DESTINO_B = `chave-secreta-da-b-${marca}`
const destinoB = await req(`/api/orgs/${B.orgId}/stream-destinations`, {
  token: B.token, method: 'POST',
  body: { kind: 'rtmp', label: 'Destino da B', url: 'rtmp://10.0.0.9/live', stream_key: CHAVE_DESTINO_B },
})
if (destinoB.status === 201 && destinoB.json?.id) {
  ok('B guarda um destino de directo → 201')
  const idB = destinoB.json.id
  const d = `/api/orgs/${B.orgId}/stream-destinations/${idB}`
  const semChave = (j) => !JSON.stringify(j ?? null).includes(CHAVE_DESTINO_B)
  // A chave sai UMA vez, na resposta que a cria (ADR-0003/G1); depois só há
  // `key_prefix` e `has_key` — é o que a lista e o detalhe abaixo provam.
  if (destinoB.json.stream_key === CHAVE_DESTINO_B && destinoB.json.key_prefix && destinoB.json.has_key === true) {
    ok('a chave sai UMA vez, na criação (com key_prefix/has_key)')
  } else nok('a chave sai UMA vez, na criação', JSON.stringify(destinoB.json).slice(0, 160))
  const listaB = await req(`/api/orgs/${B.orgId}/stream-destinations`, { token: B.token })
  const umB = await req(d, { token: B.token })
  if (listaB.status === 200 && umB.status === 200 && semChave(listaB.json) && semChave(umB.json)) {
    ok('nem a própria B volta a ver a chave (lista e detalhe)')
  } else {
    nok('nem a própria B volta a ver a chave', `${listaB.status}/${umB.status}`)
  }

  await recusado('A lê um destino da org B', d, { token: A.token })
  await recusado('A roda a chave de um destino da org B', `${d}/rotate-key`, {
    token: A.token, method: 'POST', body: { stream_key: 'roubada' },
  })
  await recusado('A roda a chave do destino da B pelo caminho da org B', `/api/orgs/${B.orgId}/stream-destinations/${idB}/rotate-key`, {
    token: A.token, method: 'POST', body: { stream_key: 'roubada' },
  })
  await recusado('A cria um destino na org B', `/api/orgs/${B.orgId}/stream-destinations`, {
    token: A.token, method: 'POST',
    body: { kind: 'rtmp', label: 'intruso', url: 'rtmp://127.0.0.1:1/live', stream_key: 'k' },
  })
  await recusado('A altera um destino da org B', d, {
    token: A.token, method: 'PATCH', body: { url: 'rtmp://atacante.exemplo/live' },
  })
  // Pelo caminho da PRÓPRIA org A, com o id da B: o `WHERE org_id` tem de o esconder.
  await recusado('A lê o destino da B pelo caminho da org A', `/api/orgs/${A.orgId}/stream-destinations/${idB}`, { token: A.token })
  await recusado('A apaga o destino da B pelo caminho da org A', `/api/orgs/${A.orgId}/stream-destinations/${idB}`, {
    token: A.token, method: 'DELETE',
  })
  await recusado('A apaga um destino da org B', d, { token: A.token, method: 'DELETE' })
  const depois = await req(d, { token: B.token })
  if (depois.status === 200 && depois.json?.url === 'rtmp://10.0.0.9/live') ok('e o destino da B CONTINUA LÁ, inalterado')
  else nok('e o destino da B CONTINUA LÁ, inalterado', `${depois.status}: ${JSON.stringify(depois.json).slice(0, 120)}`)

  // Emitir com o destino da B a partir de uma sala da A. A recusa chega numa
  // trama de texto depois do upgrade (ver `ws_directo`).
  const recusaDoDirecto = (roomToken, code, destinos) => new Promise((resolve) => {
    const q = new URLSearchParams({ token: roomToken, destinos: JSON.stringify(destinos), codec: 'video/h264' })
    const ws = new WebSocket(`${WS}/api/rooms/${code}/live?${q}`)
    const t = setTimeout(() => { ws.close(); resolve('(sem resposta)') }, 8000)
    ws.on('message', (d) => {
      const m = JSON.parse(d.toString())
      if (m.erro) { clearTimeout(t); ws.close(); resolve(m.erro) }
      else if (m.tipo === 'destinos') { clearTimeout(t); ws.close(); resolve('(aceite)') }
    })
    ws.on('error', () => { clearTimeout(t); resolve('(erro de ligação)') })
  })
  const NAO_E_TEU = /não existe, não é desta organização/
  const salaDirectoA = (await req('/api/rooms', { token: A.token, method: 'POST', body: { name: 'directo da A', topology: 'sfu' } })).json
  const joinDirectoA = await req(`/api/rooms/${salaDirectoA.code}/join`, { token: A.token, method: 'POST' })
  const vDirA = await recusaDoDirecto(joinDirectoA.json?.room_token, salaDirectoA.code, [{ id: idB }])
  if (NAO_E_TEU.test(vDirA)) ok('A NÃO emite com o destino guardado da B (recusado no WebSocket)')
  else nok('A NÃO emite com o destino guardado da B', `o servidor respondeu: ${vDirA}`)
  // Controlo positivo: a B, com o SEU destino, não leva ESSA recusa (pode levar
  // outra — sem ffmpeg no runner, por exemplo — e isso não é o que se mede).
  const salaDirectoB = (await req('/api/rooms', { token: B.token, method: 'POST', body: { name: 'directo da B', topology: 'sfu' } })).json
  const joinDirectoB = await req(`/api/rooms/${salaDirectoB.code}/join`, { token: B.token, method: 'POST' })
  const vDirB = await recusaDoDirecto(joinDirectoB.json?.room_token, salaDirectoB.code, [{ id: idB }])
  if (!NAO_E_TEU.test(vDirB) && !vDirB.startsWith('(erro')) ok(`B emite com o próprio destino guardado (controlo positivo: ${vDirB.slice(0, 60)})`)
  else nok('B emite com o próprio destino guardado (controlo positivo)', vDirB)
  if (!vDirA.includes(CHAVE_DESTINO_B) && !vDirB.includes(CHAVE_DESTINO_B)) ok('nenhuma resposta do directo leva a chave')
  else nok('nenhuma resposta do directo leva a chave', 'a chave apareceu numa trama do WebSocket')
} else {
  // 503 = servidor sem DATA_ENCRYPTION_KEYS. Diz-se, em vez de passar em silêncio.
  nok('B guarda um destino de directo', `devolveu ${destinoB.status}: ${JSON.stringify(destinoB.json).slice(0, 160)}`)
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
await recusado('anónimo vê metadados da sala da B', `/api/rooms/${salaB.code}`, {})
await recusado('anónimo vê o estado do directo da sala da B', `/api/rooms/${salaB.code}/live/status`, {})
// A fila da sala de espera traz nomes, origem e cargo de quem espera: só o
// dono/co-anfitrião a vê. O controlo positivo vem primeiro — sem ele, a recusa
// não prova nada.
await permitido('B (dona) espreita a sala de espera da sua sala', `/api/rooms/${salaB.code}/waiting?room=${salaB.code}`, { token: B.token })
await recusadoNaPorta('A espreita a sala de espera da sala da B', `/api/rooms/${salaB.code}/waiting?room=${salaB.code}`, { token: A.token })

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
  // `/api/meetings/{meeting_id}` tem GET e DELETE, e `/minutes` só tem PUT — um GET
  // devolve 405, que o helper contava como recusa sem provar nada. Foi o
  // CONTROLO POSITIVO abaixo que deu por isso: «B lista as suas reuniões»
  // devolvia 405 também. Sem ele, duas asserções verdes mediam o router, não a
  // autorização.
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


// O LEITOR DE GRAVAÇÕES (migrações 0040–0046) contra uma gravação REAL da org B.
//
// Aqui a gravação FABRICA-SE: B entra na sua sala e carrega um ficheiro. Com um
// id que existe, um 404 prova a recusa e não a ausência — e o controlo
// positivo (B lê os seus detalhes) garante que a rota não está simplesmente
// partida. Um recurso de outra organização é 404, nunca 403: não se confirma
// que existe. O contrato completo (e o colega da mesma organização que só vê o
// que foi publicado) está em `gravacoes-meta.mjs`.
console.log('\n--- leitor de gravações da org B ---')
await req(`/api/rooms/${salaB.code}/join`, { token: B.token, method: 'POST' })
const upB = await fetch(`${API}/api/rooms/${salaB.code}/recordings?name=iso.webm`, {
  method: 'POST', headers: { Authorization: `Bearer ${B.token}` }, body: new Uint8Array([0x1a, 0x45, 0xdf, 0xa3]),
})
const gravB = upB.ok ? (await upB.json()).id : null
if (gravB) {
  await permitido('B lê os detalhes da sua gravação (controlo positivo)', `/api/recordings/${gravB}/details`, { token: B.token })
  const lingua = 'pt'
  await permitido('B envia uma legenda (controlo positivo)', `/api/recordings/${gravB}/captions/${lingua}`, {
    token: B.token, method: 'PUT', body: { vtt: 'WEBVTT\n\n00:00.000 --> 00:01.000\nprivado da B\n', publish: true },
  })
  const capB = (await req(`/api/recordings/${gravB}/chapters`, { token: B.token, method: 'POST', body: { at_secs: 0, title: 'da B' } })).json
  const comB = (await req(`/api/recordings/${gravB}/comments`, { token: B.token, method: 'POST', body: { body: 'da B' } })).json
  const capId = capB?.id ?? inventado
  const comId = comB?.id ?? inventado

  await recusadoNaPorta('A lê os detalhes da gravação da B', `/api/recordings/${gravB}/details`, { token: A.token })
  await recusadoNaPorta('A EDITA a gravação da B', `/api/recordings/${gravB}`, { token: A.token, method: 'PATCH', body: { description: 'forjada' } })
  await recusadoNaPorta('A PUBLICA a gravação da B', `/api/recordings/${gravB}/publish`, { token: A.token, method: 'POST', body: { visibility: 'org' } })
  await recusadoNaPorta('A despublica a gravação da B', `/api/recordings/${gravB}/unpublish`, { token: A.token, method: 'POST' })
  await recusadoNaPorta('A lê a miniatura da B', `/api/recordings/${gravB}/thumbnail`, { token: A.token })
  await recusadoNaPorta('A regista visualizações na gravação da B', `/api/recordings/${gravB}/views`, { token: A.token, method: 'POST' })
  await recusadoNaPorta('A lista os participantes da gravação da B', `/api/recordings/${gravB}/participants`, { token: A.token })
  await recusadoNaPorta('A lista os participantes da sala da B', `/api/rooms/${salaB.code}/participants`, { token: A.token })
  await recusadoNaPorta('A lê a transcrição da B', `/api/recordings/${gravB}/transcript`, { token: A.token })
  await recusadoNaPorta('A lista os comentários da B', `/api/recordings/${gravB}/comments`, { token: A.token })
  await recusadoNaPorta('A comenta a gravação da B', `/api/recordings/${gravB}/comments`, { token: A.token, method: 'POST', body: { body: 'intruso' } })
  await recusadoNaPorta('A lê um comentário da B', `/api/recordings/${gravB}/comments/${comId}`, { token: A.token })
  await recusadoNaPorta('A APAGA um comentário da B', `/api/recordings/${gravB}/comments/${comId}`, { token: A.token, method: 'DELETE' })
  await recusadoNaPorta('A lista os capítulos da B', `/api/recordings/${gravB}/chapters`, { token: A.token })
  await recusadoNaPorta('A cria um capítulo na B', `/api/recordings/${gravB}/chapters`, { token: A.token, method: 'POST', body: { at_secs: 1, title: 'x' } })
  await recusadoNaPorta('A gera capítulos na B', `/api/recordings/${gravB}/chapters/generate`, { token: A.token, method: 'POST' })
  await recusadoNaPorta('A lê um capítulo da B', `/api/recordings/${gravB}/chapters/${capId}`, { token: A.token })
  await recusadoNaPorta('A EDITA um capítulo da B', `/api/recordings/${gravB}/chapters/${capId}`, { token: A.token, method: 'PATCH', body: { title: 'forjado' } })
  await recusadoNaPorta('A APAGA um capítulo da B', `/api/recordings/${gravB}/chapters/${capId}`, { token: A.token, method: 'DELETE' })
  await recusadoNaPorta('A lista as legendas da B', `/api/recordings/${gravB}/captions`, { token: A.token })
  await recusadoNaPorta('A lê uma legenda publicada da B', `/api/recordings/${gravB}/captions/${lingua}`, { token: A.token })
  await recusadoNaPorta('A lê o VTT publicado da B', `/api/recordings/${gravB}/captions/${lingua}/vtt`, { token: A.token })
  await recusadoNaPorta('A SUBSTITUI a legenda da B', `/api/recordings/${gravB}/captions/${lingua}`, {
    token: A.token, method: 'PUT', body: { vtt: 'WEBVTT\n', publish: true },
  })
  await recusadoNaPorta('A gera legendas na B', `/api/recordings/${gravB}/captions/generate`, { token: A.token, method: 'POST', body: {} })
  const pub = await req('/api/recordings?scope=published', { token: A.token })
  if (Array.isArray(pub.json) && !pub.json.some((r) => r.id === gravB)) ok('a gravação da B não aparece nas publicadas da A')
  else nok('a gravação da B não aparece nas publicadas da A', JSON.stringify(pub.json).slice(0, 160))
  // O estado, não só o código: nada do que A tentou ficou escrito.
  const depois = (await req(`/api/recordings/${gravB}/details`, { token: B.token })).json
  const capsB = (await req(`/api/recordings/${gravB}/chapters`, { token: B.token })).json
  const comsB = (await req(`/api/recordings/${gravB}/comments`, { token: B.token })).json
  const n = (p) => (Array.isArray(p) ? p.length : (p?.items?.length ?? -1))
  if (depois?.title === null && depois.share_count === 0 && n(capsB) === 1 && n(comsB) === 1) {
    ok('e a gravação da B ficou exactamente como estava')
  } else {
    nok('e a gravação da B ficou exactamente como estava', JSON.stringify({ depois, capitulos: n(capsB), comentarios: n(comsB) }).slice(0, 700))
  }
} else {
  nok('B carrega uma gravação para o teste do leitor', `devolveu ${upB.status}`)
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

  // -------------------------------------------------------------------------
  // SMS a CONTACTOS (extensão do ADR-0005). O número vem sempre do servidor; o
  // cliente só diz a QUEM. A pergunta de isolamento passa a ter três metades:
  // «A alcança o contacto de B», «um membro sem permissão envia», e «o membro
  // escolhe o número por baixo do `user_id`».
  // A rota é o modem falso de B (USB) — o CI não tem operador configurado.
  // -------------------------------------------------------------------------
  console.log('\n--- SMS a contactos: telefone, política, consentimento, limites ---')
  await req('/api/integrations/sms-agent/v1/devices', { token: gwB.json.token, method: 'PUT', body: { devices: [modemFalso('B')] } })
  const dominioB = B.email.split('@')[1]
  async function membroB(nome, role) {
    const email = `${nome}-${marca}@${dominioB}`
    const r = await req(`/api/orgs/${B.orgId}/members`, {
      token: B.token, method: 'POST',
      body: { email, username: `${nome}-${marca}`, password: PW, role, title: nome },
    })
    if (!(r.status >= 200 && r.status < 300)) throw new Error(`não criei ${email}: ${r.status} ${JSON.stringify(r.json)}`)
    const l = await req('/api/auth/login', { method: 'POST', body: { email, password: PW } })
    return { email, token: l.json?.access_token, userId: r.json.user_id }
  }
  const M = await membroB('marta', 'member') // quem envia
  const N = await membroB('nuno', 'member') // contacto com telefone
  const O = await membroB('olga', 'member') // contacto SEM telefone
  const P = await membroB('paulo', 'member') // contacto que vai ser arquivado

  // Telefone: o próprio e o admin escrevem; mais ninguém.
  const telM = await permitido('M regista o próprio telefone', `/api/orgs/${B.orgId}/members/${M.userId}/phone`, {
    token: M.token, method: 'PUT', body: { phone: '923 100 200' },
  })
  if (telM?.phone === '+244923100200' && telM?.phone_source === 'manual') ok('o número fica normalizado e marcado manual')
  else nok('o número fica normalizado e marcado manual', JSON.stringify(telM))
  await permitido('o admin de B regista o telefone de N', `/api/orgs/${B.orgId}/members/${N.userId}/phone`, {
    token: B.token, method: 'PUT', body: { phone: '+244 923 300 400' },
  })
  await permitido('o admin de B regista o telefone de P', `/api/orgs/${B.orgId}/members/${P.userId}/phone`, {
    token: B.token, method: 'PUT', body: { phone: '923500600' },
  })
  const telOutro = await req(`/api/orgs/${B.orgId}/members/${N.userId}/phone`, {
    token: M.token, method: 'PUT', body: { phone: '923999999' },
  })
  if (telOutro.status === 403) ok('M (membro) muda o telefone de N → 403')
  else nok('M (membro) muda o telefone de N → 403', `${telOutro.status} ${JSON.stringify(telOutro.json)}`)
  await recusadoNaPorta('A muda o telefone de um membro da org B', `/api/orgs/${B.orgId}/members/${N.userId}/phone`, {
    token: A.token, method: 'PUT', body: { phone: '923999999' },
  })
  await recusadoNaPorta('A escreve o telefone de N pelo caminho da SUA org', `/api/orgs/${A.orgId}/members/${N.userId}/phone`, {
    token: A.token, method: 'PUT', body: { phone: '923999999' },
  })
  const telPt = await req(`/api/orgs/${B.orgId}/members/${M.userId}/phone`, {
    token: M.token, method: 'PUT', body: { phone: '+351 912 345 678' },
  })
  if (telPt.status === 422) ok('número fora de Angola não é guardado (422) — o encaminhamento não o serve')
  else nok('número fora de Angola não é guardado (422)', `${telPt.status} ${JSON.stringify(telPt.json)}`)

  // Visibilidade: o colega sabe QUE há número, não QUAL.
  const dirM = (await req(`/api/orgs/${B.orgId}/members`, { token: M.token })).json ?? []
  const nVistoPorM = dirM.find((e) => e.user_id === N.userId)
  const mVistoPorM = dirM.find((e) => e.user_id === M.userId)
  if (nVistoPorM && nVistoPorM.phone === null && nVistoPorM.can_sms === true && mVistoPorM?.phone === '+244923100200') {
    ok('o membro vê can_sms do colega mas não o número; vê o seu')
  } else nok('o membro não vê o número do colega', JSON.stringify({ nVistoPorM, mVistoPorM }))
  const dirB = (await req(`/api/orgs/${B.orgId}/members`, { token: B.token })).json ?? []
  if (dirB.find((e) => e.user_id === N.userId)?.phone === '+244923300400') ok('controlo: o admin vê o número de N')
  else nok('controlo: o admin vê o número de N', JSON.stringify(dirB))

  // Política: por omissão só admins.
  const pol0 = await permitido('M lê a política de SMS da sua org', `/api/orgs/${B.orgId}/sms/policy`, { token: M.token })
  if (pol0?.send_policy === 'admins') ok('a política por omissão é admins')
  else nok('a política por omissão é admins', JSON.stringify(pol0))
  await recusado('A lê a política de SMS da org B', `/api/orgs/${B.orgId}/sms/policy`, { token: A.token })
  await recusado('A abre o envio da org B a membros', `/api/orgs/${B.orgId}/sms/policy`, {
    token: A.token, method: 'PUT', body: { send_policy: 'members' },
  })
  const polM = await req(`/api/orgs/${B.orgId}/sms/policy`, { token: M.token, method: 'PUT', body: { send_policy: 'members' } })
  if (polM.status === 403) ok('M (membro) muda a política → 403')
  else nok('M (membro) muda a política → 403', `${polM.status}`)
  const antesDaPolitica = await req(`/api/orgs/${B.orgId}/sms/messages`, {
    token: M.token, method: 'POST', body: { user_id: N.userId, body: 'ola' },
  })
  if (antesDaPolitica.status === 403) ok('M envia SMS a N com a política admins → 403')
  else nok('M envia SMS a N com a política admins → 403', `${antesDaPolitica.status} ${JSON.stringify(antesDaPolitica.json)}`)
  const polLixo = await req(`/api/orgs/${B.orgId}/sms/policy`, { token: B.token, method: 'PUT', body: { send_policy: 'everyone' } })
  if (polLixo.status === 400) ok('política desconhecida é recusada (400), não ignorada')
  else nok('política desconhecida é recusada (400)', `${polLixo.status}`)
  await permitido('o admin de B abre o envio a membros', `/api/orgs/${B.orgId}/sms/policy`, {
    token: B.token, method: 'PUT', body: { send_policy: 'members' },
  })

  // O modo `to` continua só de admin, com qualquer política.
  const toMembro = await req(`/api/orgs/${B.orgId}/sms/messages`, {
    token: M.token, method: 'POST', body: { to: '923000777', body: 'fraude' },
  })
  if (toMembro.status === 403) ok('M envia para um número escrito (modo to) → 403')
  else nok('M envia para um número escrito (modo to) → 403', `${toMembro.status} ${JSON.stringify(toMembro.json)}`)
  // O número nunca vem do cliente no modo contacto.
  const ambos = await req(`/api/orgs/${B.orgId}/sms/messages`, {
    token: M.token, method: 'POST', body: { user_id: N.userId, to: '923000777', body: 'desvio' },
  })
  if (ambos.status === 400 && /sms\.target_ambiguous/.test(ambos.json?.error ?? '')) ok('user_id + to no mesmo pedido → 400 sms.target_ambiguous')
  else nok('user_id + to no mesmo pedido → 400', `${ambos.status} ${JSON.stringify(ambos.json)}`)

  // Controlo positivo: M envia a N.
  const paraN = await req(`/api/orgs/${B.orgId}/sms/messages`, {
    token: M.token, method: 'POST', body: { user_id: N.userId, body: 'reuniao adiada', route: 'usb' },
  })
  if (paraN.status === 202 && paraN.json?.purpose === 'contact' && paraN.json?.recipient_user_id === N.userId
      && paraN.json?.to === '+244*******00' && /^marta-\w+ \(Delonix Meet\): reuniao adiada$/.test(paraN.json?.body ?? '')) {
    ok('M envia a N pelo user_id (202), com o nome de M no texto e o número mascarado para M')
  } else nok('M envia a N pelo user_id (202)', `${paraN.status} ${JSON.stringify(paraN.json)}`)
  const vistaAdmin = (await req(`/api/orgs/${B.orgId}/sms/messages/${paraN.json?.id}`, { token: B.token })).json
  if (vistaAdmin?.to === '+244923300400') ok('o número resolvido foi o de N (visto pelo admin)')
  else nok('o número resolvido foi o de N', JSON.stringify(vistaAdmin))

  // Destinatário fora do alcance: 404, sem confirmar qual é o caso.
  await recusadoNaPorta('M envia SMS a um membro da org A', `/api/orgs/${B.orgId}/sms/messages`, {
    token: M.token, method: 'POST', body: { user_id: A.userId, body: 'x' },
  })
  await recusadoNaPorta('A envia SMS a N pelo caminho da SUA org', `/api/orgs/${A.orgId}/sms/messages`, {
    token: A.token, method: 'POST', body: { user_id: N.userId, body: 'x' },
  })
  await permitido('o admin de B arquiva P', `/api/orgs/${B.orgId}/members/${P.userId}`, { token: B.token, method: 'DELETE' })
  await recusadoNaPorta('M envia SMS a P ARQUIVADO (tinha telefone)', `/api/orgs/${B.orgId}/sms/messages`, {
    token: M.token, method: 'POST', body: { user_id: P.userId, body: 'x' },
  })
  const semTel = await req(`/api/orgs/${B.orgId}/sms/messages`, {
    token: M.token, method: 'POST', body: { user_id: O.userId, body: 'x' },
  })
  if (semTel.status === 422 && /^sms\.recipient_no_phone/.test(semTel.json?.error ?? '')) ok('contacto sem telefone → 422 sms.recipient_no_phone')
  else nok('contacto sem telefone → 422', `${semTel.status} ${JSON.stringify(semTel.json)}`)

  // Consentimento: N desliga os SMS de contactos.
  const prefN = await permitido('N desliga os SMS de contactos', '/api/users/me/sms-preferences', {
    token: N.token, method: 'PUT', body: { contact_opt_out: true },
  })
  if (prefN?.contact_opt_out === true && prefN?.meeting_opt_out === false && prefN?.phones?.[0]?.phone === '+244923300400') {
    ok('as preferências de N ficam gravadas e trazem o telefone dele')
  } else nok('as preferências de N', JSON.stringify(prefN))
  const optOut = await req(`/api/orgs/${B.orgId}/sms/messages`, {
    token: M.token, method: 'POST', body: { user_id: N.userId, body: 'insisto' },
  })
  if (optOut.status === 409 && /^sms\.recipient_opted_out/.test(optOut.json?.error ?? '')) ok('N com opt-out → 409 sms.recipient_opted_out')
  else nok('N com opt-out → 409', `${optOut.status} ${JSON.stringify(optOut.json)}`)
  const dirDepois = (await req(`/api/orgs/${B.orgId}/members`, { token: M.token })).json ?? []
  if (dirDepois.find((e) => e.user_id === N.userId)?.can_sms === false) ok('o directório deixa de oferecer SMS a N (can_sms false)')
  else nok('o directório deixa de oferecer SMS a N', JSON.stringify(dirDepois.find((e) => e.user_id === N.userId)))

  // O membro vê o estado do que ENVIOU, e só isso.
  const listaM = (await req(`/api/orgs/${B.orgId}/sms/messages`, { token: M.token })).json?.items ?? []
  if (listaM.length === 1 && listaM[0].id === paraN.json?.id && listaM[0].to.includes('*') && ['queued', 'claimed', 'sent', 'failed'].includes(listaM[0].status)) {
    ok(`M lista só a sua mensagem, mascarada, com estado «${listaM[0].status}»`)
  } else nok('M lista só a sua mensagem', JSON.stringify(listaM).slice(0, 300))
  await recusado('M lê uma mensagem do admin de B', `/api/orgs/${B.orgId}/sms/messages/${msgB.json?.id}`, { token: M.token })
  const trilha = JSON.stringify((await req(`/api/orgs/${B.orgId}/audit-events?limit=200`, { token: B.token })).json ?? [])
  if (trilha.includes('sms.contact_queued') && !trilha.includes('923300400') && !trilha.includes('923100200')) {
    ok('a auditoria regista o envio a contacto sem nenhum número em claro')
  } else nok('a auditoria sem números', trilha.slice(0, 300))

  // Limite por utilizador: 5 por minuto. As recusas acima NÃO gastaram quota.
  await req('/api/users/me/sms-preferences', { token: N.token, method: 'PUT', body: { contact_opt_out: false } })
  const estados = []
  for (let i = 0; i < 5; i++) {
    const r = await req(`/api/orgs/${B.orgId}/sms/messages`, {
      token: M.token, method: 'POST', body: { user_id: N.userId, body: `rajada ${i}`, route: 'usb' },
    })
    estados.push(r.status)
  }
  if (estados.slice(0, 4).every((s) => s === 202) && estados[4] === 429) {
    ok('limite por utilizador: 4 aceites depois do primeiro, o 6.º → 429 (as recusas não contaram)')
  } else nok('limite por utilizador', JSON.stringify(estados))
  const colega = await req(`/api/orgs/${B.orgId}/sms/messages`, {
    token: N.token, method: 'POST', body: { user_id: M.userId, body: 'e eu?', route: 'usb' },
  })
  if (colega.status === 202) ok('controlo: o limite de M não trava N (202)')
  else nok('controlo: o limite de M não trava N', `${colega.status} ${JSON.stringify(colega.json)}`)

  // SMS de reunião: a permissão vê-se ANTES de criar a reunião.
  await permitido('o admin de B fecha outra vez o envio a admins', `/api/orgs/${B.orgId}/sms/policy`, {
    token: B.token, method: 'PUT', body: { send_policy: 'admins' },
  })
  const inicio = new Date(Date.now() + 3 * 3600_000).toISOString()
  const reuniaoM = await req('/api/meetings', {
    token: M.token, method: 'POST',
    body: { title: 'sem permissao', starts_at: inicio, duration_min: 30, invitee_ids: [N.userId], sms_invite: true },
  })
  const reunioesM = (await req('/api/meetings', { token: M.token })).json ?? []
  if (reuniaoM.status === 403 && !reunioesM.some((m) => m.title === 'sem permissao')) {
    ok('M agenda com sms_invite sem permissão → 403, e a reunião não é criada')
  } else nok('agendar com SMS sem permissão', `${reuniaoM.status} ${JSON.stringify(reuniaoM.json).slice(0, 200)}`)
  const recorrente = await req('/api/meetings', {
    token: B.token, method: 'POST',
    body: { title: 'semanal', starts_at: inicio, duration_min: 30, invitee_ids: [N.userId], sms_reminder_min: 15, recurrence_freq: 'weekly' },
  })
  if (recorrente.status === 422) ok('lembrete por SMS numa série recorrente → 422 (não se promete o que não se envia)')
  else nok('lembrete em série recorrente → 422', `${recorrente.status}`)
  await req('/api/integrations/sms-agent/v1/devices', { token: gwB.json.token, method: 'PUT', body: { devices: [modemFalso('B')] } })
  const reuniaoB = await req('/api/meetings', {
    token: B.token, method: 'POST',
    body: { title: 'com SMS', starts_at: inicio, duration_min: 30, invitee_ids: [N.userId, O.userId, A.userId], sms_invite: true, sms_reminder_min: 30 },
  })
  const saltos = Object.fromEntries((reuniaoB.json?.sms?.invite?.skipped ?? []).map((s) => [s.user_id, s.reason]))
  if (reuniaoB.status === 200 && reuniaoB.json?.sms?.invite?.queued === 1 && reuniaoB.json.sms.reminder_min === 30
      && saltos[O.userId] === 'sms.recipient_no_phone' && saltos[A.userId] === 'sms.recipient_not_member') {
    ok('o admin agenda com convite por SMS: 1 enfileirado, O sem telefone e o de outra org saltados com código')
  } else nok('agendar com convite por SMS', `${reuniaoB.status} ${JSON.stringify(reuniaoB.json?.sms)}`)
  const convites = ((await req(`/api/orgs/${B.orgId}/sms/messages`, { token: B.token })).json?.items ?? [])
    .filter((m) => m.purpose === 'meeting_invite' && m.meeting_id === reuniaoB.json?.id)
  if (convites.length === 1 && convites[0].recipient_user_id === N.userId && convites[0].to === '+244923300400') {
    ok('o convite foi para N, pelo número de N')
  } else nok('o convite foi para N', JSON.stringify(convites))

  // Revogar corta o agente.
  await permitido('B revoga o seu gateway', `/api/orgs/${B.orgId}/sms/gateways/${gwB.json.id}`, {
    token: B.token, method: 'DELETE',
  })
  await recusado('o token revogado de B deixa de servir', '/api/integrations/sms-agent/v1/claim', { token: gwB.json.token, method: 'POST' })
}

console.log('\n--- papéis, permissões, utilizadores e convites (ADR-0008) ---')
{
  const X = '00000000-0000-4000-8000-000000000000'
  const papeisB = await req(`/api/orgs/${B.orgId}/roles`, { token: B.token })
  const membroB = (papeisB.json?.items ?? []).find((r) => r.key === 'member')?.id ?? X
  const papelB = await req(`/api/orgs/${B.orgId}/roles`, {
    token: B.token, method: 'POST', body: { name: 'Só da B', capabilities: { 'admin.view_audit': 'allow' } },
  })
  const rB = papelB.json?.id ?? X
  if (papelB.status !== 201) nok('B cria um papel para o teste', `devolveu ${papelB.status}`)
  const depB = await req(`/api/orgs/${B.orgId}/departments`, { token: B.token, method: 'POST', body: { name: 'Dep B' } })
  const dB = depB.json?.id ?? X
  const convB = await req(`/api/orgs/${B.orgId}/invitations`, {
    token: B.token, method: 'POST', body: { email: `convidado-${Date.now()}@${B.email.split('@')[1]}`, role_id: membroB, delivery: 'code' },
  })
  const iB = convB.json?.id ?? X
  const regraB = await req(`/api/orgs/${B.orgId}/sod-rules`, {
    token: B.token, method: 'POST', body: { name: 'Regra B', capabilities: ['admin.view_audit', 'admin.manage_roles'] },
  })
  const sB = regraB.json?.id ?? X
  const a = { token: A.token }
  await recusado('A lista os papéis da org B', `/api/orgs/${B.orgId}/roles`, a)
  await recusado('A cria um papel na org B', `/api/orgs/${B.orgId}/roles`, { ...a, method: 'POST', body: { name: 'intruso' } })
  await recusado('A lê um papel da org B', `/api/orgs/${B.orgId}/roles/${rB}`, a)
  await recusado('A altera um papel da org B', `/api/orgs/${B.orgId}/roles/${rB}`, { ...a, method: 'PATCH', body: { name: 'x' } })
  await recusado('A apaga um papel da org B', `/api/orgs/${B.orgId}/roles/${rB}`, { ...a, method: 'DELETE' })
  await recusado('A duplica um papel da org B', `/api/orgs/${B.orgId}/roles/${rB}/duplicate`, { ...a, method: 'POST', body: {} })
  await recusado('A lê a coluna da matriz da org B', `/api/orgs/${B.orgId}/roles/${rB}/capabilities`, a)
  await recusado('A reescreve a matriz da org B', `/api/orgs/${B.orgId}/roles/${rB}/capabilities`, { ...a, method: 'PUT', body: { values: {} } })
  await recusado('A exporta a matriz da org B', `/api/orgs/${B.orgId}/permission-matrix`, a)
  await recusado('A simula um utilizador da org B', `/api/orgs/${B.orgId}/authorization/evaluations`, { ...a, method: 'POST', body: { user_id: B.userId } })
  await recusado('A lê as «minhas» capacidades na org B', `/api/orgs/${B.orgId}/members/me/capabilities`, a)
  await recusado('A atribui papel a alguém da org B', `/api/orgs/${B.orgId}/members/${B.userId}/role`, { ...a, method: 'PUT', body: { role_id: membroB } })
  await recusado('A lista regras SoD da org B', `/api/orgs/${B.orgId}/sod-rules`, a)
  await recusado('A cria regra SoD na org B', `/api/orgs/${B.orgId}/sod-rules`, { ...a, method: 'POST', body: { name: 'z', capabilities: ['admin.view_audit', 'admin.manage_roles'] } })
  await recusado('A lê uma regra SoD da org B', `/api/orgs/${B.orgId}/sod-rules/${sB}`, a)
  await recusado('A altera uma regra SoD da org B', `/api/orgs/${B.orgId}/sod-rules/${sB}`, { ...a, method: 'PATCH', body: { name: 'w' } })
  await recusado('A apaga uma regra SoD da org B', `/api/orgs/${B.orgId}/sod-rules/${sB}`, { ...a, method: 'DELETE' })
  await recusado('A aceita risco na org B', `/api/orgs/${B.orgId}/sod-rules/${sB}/risk-acceptances`, { ...a, method: 'POST', body: { user_id: B.userId, justification: 'justificação longa' } })
  await recusado('A lê violações SoD da org B', `/api/orgs/${B.orgId}/sod-violations`, a)
  await recusado('A lista conflitos de papel da org B', `/api/orgs/${B.orgId}/role-conflicts`, a)
  await recusado('A resolve um conflito da org B', `/api/orgs/${B.orgId}/role-conflicts/${X}/resolve`, { ...a, method: 'POST', body: { decision: 'keep_current' } })
  await recusado('A lista aprovações da org B', `/api/orgs/${B.orgId}/approval-requests`, a)
  await recusado('A lê uma aprovação da org B', `/api/orgs/${B.orgId}/approval-requests/${X}`, a)
  await recusado('A aprova na org B', `/api/orgs/${B.orgId}/approval-requests/${X}/approve`, { ...a, method: 'POST', body: {} })
  await recusado('A recusa na org B', `/api/orgs/${B.orgId}/approval-requests/${X}/reject`, { ...a, method: 'POST', body: { reason: 'x' } })
  await recusado('A lista utilizadores da org B', `/api/orgs/${B.orgId}/users`, a)
  await recusado('A suspende em massa na org B', `/api/orgs/${B.orgId}/users/bulk-actions`, { ...a, method: 'POST', body: { action: 'suspend', user_ids: [B.userId] } })
  await recusado('A importa CSV na org B', `/api/orgs/${B.orgId}/users/imports`, { ...a, method: 'POST', body: { csv: 'email\nx@y.local' } })
  await recusado('A lista convites da org B', `/api/orgs/${B.orgId}/invitations`, a)
  await recusado('A convida para a org B', `/api/orgs/${B.orgId}/invitations`, { ...a, method: 'POST', body: { email: 'x@y.local', role_id: membroB } })
  await recusado('A lê um convite da org B', `/api/orgs/${B.orgId}/invitations/${iB}`, a)
  await recusado('A revoga um convite da org B', `/api/orgs/${B.orgId}/invitations/${iB}`, { ...a, method: 'DELETE' })
  await recusado('A reenvia um convite da org B', `/api/orgs/${B.orgId}/invitations/${iB}/resend`, { ...a, method: 'POST', body: {} })
  await recusado('A lista departamentos da org B', `/api/orgs/${B.orgId}/departments`, a)
  await recusado('A cria departamento na org B', `/api/orgs/${B.orgId}/departments`, { ...a, method: 'POST', body: { name: 'x' } })
  await recusado('A lê um departamento da org B', `/api/orgs/${B.orgId}/departments/${dB}`, a)
  await recusado('A renomeia um departamento da org B', `/api/orgs/${B.orgId}/departments/${dB}`, { ...a, method: 'PATCH', body: { name: 'y' } })
  await recusado('A apaga um departamento da org B', `/api/orgs/${B.orgId}/departments/${dB}`, { ...a, method: 'DELETE' })
  await recusado('A lê os lugares da org B', `/api/orgs/${B.orgId}/seats`, a)
  await recusado('A liberta lugares na org B', `/api/orgs/${B.orgId}/seats/release`, { ...a, method: 'POST', body: { inactive_days: 60 } })
  await recusado('A lê o aprovisionamento da org B', `/api/orgs/${B.orgId}/provisioning`, a)
  await recusado('A lê as regras de entrada da org B', `/api/orgs/${B.orgId}/entry-rules`, a)
  await recusado('A reescreve as regras de entrada da org B', `/api/orgs/${B.orgId}/entry-rules`, { ...a, method: 'PUT', body: { create_account_on_first_login: false, approved_domains: [], suspend_on_odoo_exit: true, external_guest_ttl_hours: 24 } })
  await recusado('A (admin de org) fixa o tecto de lugares da org B', `/api/operator/v1/organizations/${B.orgId}/seats`, { ...a, method: 'PUT', body: { max_seats: 1 } })
  const codigo = convB.json?.token
  if (codigo) {
    await recusado('A aceita o convite da B com o código (correio de outra pessoa)', '/api/invitations/accept', { ...a, method: 'POST', body: { token: codigo } })
    const aindaPendente = await req(`/api/orgs/${B.orgId}/invitations/${iB}`, { token: B.token })
    if (aindaPendente.json?.status === 'pending') ok('e o convite da B CONTINUA pendente')
    else nok('e o convite da B CONTINUA pendente', JSON.stringify(aindaPendente.json))
  } else nok('B cria um convite para o teste', `devolveu ${convB.status}`)
  const papelAinda = await req(`/api/orgs/${B.orgId}/roles/${rB}`, { token: B.token })
  if (papelAinda.json?.name === 'Só da B') ok('e o papel da B CONTINUA igual')
  else nok('e o papel da B CONTINUA igual', JSON.stringify(papelAinda.json))
  // Controlo positivo: B lê o seu directório.
  await permitido('controlo: B lê o seu directório', `/api/orgs/${B.orgId}/users`, { token: B.token })
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
