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
