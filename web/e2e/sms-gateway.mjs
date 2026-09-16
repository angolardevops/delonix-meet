#!/usr/bin/env node
// Gateway de SMS (ADR-0005) ponta a ponta, contra um servidor e um Postgres A SÉRIO.
//
// O que prova:
//   1. Operador por SMPP: uma mensagem para um número Unitel sai pelo worker,
//      chega ao SMSC (falso, aberto por este script) com o destino e a origem
//      certos, e fica `sent` com o id do SMSC. Uma mensagem longa em UCS-2 chega
//      em DUAS partes.
//   2. Credencial recusada: a Africell está configurada com a password errada, e
//      a mensagem fica `failed` com o command_status do bind — não fica `sent`.
//   3. Sem rota: Movicel não está configurada e não há USB → 422 com a razão.
//   4. Idempotência: a mesma `Idempotency-Key` devolve a mesma mensagem.
//   5. SMS de reunião: agendar com `sms_invite` e `sms_reminder_min` põe no SMSC
//      o convite E o lembrete (este pelo varrimento do worker, sem outro pedido),
//      para o número de cada convidado resolvido no servidor, em GSM-7 num só
//      segmento, com a hora em WAT e o link do domínio da org. Quem desligou os
//      SMS de reunião e quem só tem rota Movicel (por contratar) ficam de fora,
//      com o código — e não chega nada ao SMSC para eles.
//   6. (Opcional, `AGENT_BIN`) o agente VERDADEIRO lê o USB desta máquina e o
//      inventário aparece na consola; um telefone sem modem exposto não pode ser
//      escolhido (422 com a razão do agente).
//
// Não prova: interoperação com o SMSC real de nenhum operador, nem envio por um
// modem físico. Isso só com contrato e com hardware.
//
// O servidor tem de arrancar com (a password tem de bater com SMSC_PASSWORD):
//   SMS_UNITEL_SMPP='smpp://delonix:p%40ss-unitel@127.0.0.1:12775?source_addr=DELONIX'
//   SMS_AFRICELL_SMPP='smpp://delonix:errada@127.0.0.1:12775?source_addr=DELONIX'
//   (SMS_MOVICEL_SMPP ausente)
//
// Uso:  API=http://127.0.0.1:8180 [AGENT_BIN=sms-gateway/target/release/delonix-sms-gateway] node web/e2e/sms-gateway.mjs
import net from 'node:net'
import { spawn } from 'node:child_process'

const API = process.env.API ?? 'http://127.0.0.1:8180'
const SMSC_PORT = Number(process.env.SMSC_PORT ?? 12775)
const SMSC_PASSWORD = 'p@ss-unitel'
const PW = 'UmaPasswordForte123!'

let passou = 0
let falhou = 0
const ok = (n) => (passou++, console.log(`  ✓ ${n}`))
const nok = (n, d) => (falhou++, console.log(`  ✗ ${n}\n      ${d}`))
const esperar = (ms) => new Promise((r) => setTimeout(r, ms))

async function req(path, { token, method = 'GET', body, headers = {} } = {}) {
  const r = await fetch(`${API}${path}`, {
    method,
    headers: {
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
      ...(body ? { 'Content-Type': 'application/json' } : {}),
      ...headers,
    },
    ...(body ? { body: JSON.stringify(body) } : {}),
  })
  let json = null
  try {
    json = await r.json()
  } catch {
    /* sem corpo */
  }
  return { status: r.status, json }
}

// ---------- SMSC falso: bind_transmitter, submit_sm, unbind ----------
const submits = []
const smsc = net.createServer((sock) => {
  let buf = Buffer.alloc(0)
  sock.on('data', (chunk) => {
    buf = Buffer.concat([buf, chunk])
    while (buf.length >= 16 && buf.length >= buf.readUInt32BE(0)) {
      const len = buf.readUInt32BE(0)
      const id = buf.readUInt32BE(4)
      const seq = buf.readUInt32BE(12)
      const body = buf.subarray(16, len)
      buf = buf.subarray(len)
      let status = 0
      let rbody = Buffer.alloc(0)
      if (id === 0x02) {
        const [, password] = body.toString('latin1').split('\0')
        status = password === SMSC_PASSWORD ? 0 : 0x0e
        rbody = Buffer.from('FAKE\0')
      } else if (id === 0x04) {
        submits.push(Buffer.from(body))
        rbody = Buffer.from(`fake-${submits.length}\0`)
      }
      const head = Buffer.alloc(16)
      head.writeUInt32BE(16 + rbody.length, 0)
      head.writeUInt32BE((id | 0x80000000) >>> 0, 4)
      head.writeUInt32BE(status, 8)
      head.writeUInt32BE(seq, 12)
      sock.write(Buffer.concat([head, rbody]))
      if (id === 0x06 || status) sock.end()
    }
  })
  sock.on('error', () => {})
})
await new Promise((r) => smsc.listen(SMSC_PORT, '127.0.0.1', r))

/** Campos C-string do início de um submit_sm: service_type, ton, npi, origem, ton, npi, destino. */
function submitHeader(body) {
  let i = body.indexOf(0) + 1
  const srcTon = body[i]
  i += 2
  const srcEnd = body.indexOf(0, i)
  const source = body.subarray(i, srcEnd).toString('latin1')
  i = srcEnd + 3
  const destEnd = body.indexOf(0, i)
  const dest = body.subarray(i, destEnd).toString('latin1')
  // esm, protocol, priority, schedule(""), validity(""), registered, replace, data_coding, default_msg, sm_length
  const dataCoding = body[destEnd + 8]
  const smLength = body[destEnd + 10]
  const sm = body.subarray(destEnd + 11, destEnd + 11 + smLength)
  // Septetos GSM por empacotar: nas letras, dígitos e na pontuação dos modelos
  // coincidem com o ASCII.
  const text = dataCoding === 0 ? sm.toString('latin1') : null
  return { srcTon, source, dest, dataCoding, text }
}

async function ateEstado(orgId, token, id, estados, ms = 15000) {
  const fim = Date.now() + ms
  let m = null
  while (Date.now() < fim) {
    m = (await req(`/api/orgs/${orgId}/sms/messages/${id}`, { token })).json
    if (estados.includes(m?.status)) return m
    await esperar(300)
  }
  return m
}

const marca = Math.random().toString(36).slice(2, 8)
const email = `admin@sms${marca}.local`
await req('/api/auth/register', {
  method: 'POST',
  body: { org_name: `Org SMS ${marca}`, email, username: `sms-${marca}`, password: PW },
})
const login = await req('/api/auth/login', { method: 'POST', body: { email, password: PW } })
const token = login.json?.access_token
const orgId = (await req('/api/orgs', { token })).json?.[0]?.id
if (!token || !orgId) {
  console.error('não consegui criar a org de teste', login)
  process.exit(1)
}
console.log(`\n=== Gateway de SMS ponta a ponta (org ${orgId}) ===\n`)

// ---------- operadores anunciados ----------
const rota = (await req(`/api/orgs/${orgId}/sms/route`, { token })).json
const conf = Object.fromEntries((rota?.operators ?? []).map((o) => [o.operator, o.configured]))
if (conf.unitel === true && conf.africell === true && conf.movicel === false) {
  ok('a consola vê Unitel e Africell configuradas e Movicel por contratar')
} else nok('operadores anunciados', `${JSON.stringify(rota)} — o servidor arrancou com as variáveis do cabeçalho?`)
if (JSON.stringify(rota).includes('ss-unitel') || JSON.stringify(rota).includes('errada')) {
  nok('a password SMPP não sai na resposta', JSON.stringify(rota))
} else ok('a password SMPP não sai na resposta')

// ---------- 1. Unitel por SMPP ----------
const curta = await req(`/api/orgs/${orgId}/sms/messages`, {
  token, method: 'POST', body: { to: '+244 923 111 222', body: 'Delonix Meet: a sua reuniao comeca as 10h.' },
})
if (curta.status === 202 && curta.json?.route === 'operator' && curta.json?.operator === 'unitel') {
  ok('auto → Unitel por operador (202)')
} else nok('auto → Unitel por operador (202)', `${curta.status} ${JSON.stringify(curta.json)}`)
const curtaFinal = await ateEstado(orgId, token, curta.json?.id, ['sent', 'failed'])
if (curtaFinal?.status === 'sent' && /^fake-\d+$/.test(curtaFinal.provider_ref ?? '')) {
  ok(`o worker entregou ao SMSC (provider_ref ${curtaFinal.provider_ref})`)
} else nok('o worker entregou ao SMSC', JSON.stringify(curtaFinal))
const h = submits[0] ? submitHeader(submits[0]) : {}
if (h.dest === '244923111222' && h.source === 'DELONIX' && h.srcTon === 5) {
  ok('o submit_sm leva destino internacional e origem alfanumérica DELONIX')
} else nok('conteúdo do submit_sm', JSON.stringify(h))

const antes = submits.length
const longa = await req(`/api/orgs/${orgId}/sms/messages`, {
  token, method: 'POST', body: { to: '944000111', body: 'Reunião de coordenação: ' + 'ç'.repeat(60) },
})
if (longa.json?.encoding === 'ucs2' && longa.json?.segments === 2) ok('texto com «ç» vai em UCS-2 e em 2 partes')
else nok('texto com «ç» vai em UCS-2 e em 2 partes', JSON.stringify(longa.json))
const longaFinal = await ateEstado(orgId, token, longa.json?.id, ['sent', 'failed'])
if (longaFinal?.status === 'sent' && submits.length - antes === 2 && longaFinal.provider_ref?.split(',').length === 2) {
  ok('as duas partes chegaram ao SMSC, cada uma com o seu id')
} else nok('as duas partes chegaram ao SMSC', `${submits.length - antes} submits, ${JSON.stringify(longaFinal)}`)

// ---------- 2. Africell com password errada ----------
const af = await req(`/api/orgs/${orgId}/sms/messages`, {
  token, method: 'POST', body: { to: '951000111', body: 'teste' },
})
const afFinal = await ateEstado(orgId, token, af.json?.id, ['sent', 'failed'])
if (afFinal?.status === 'failed' && /bind_transmitter.*0x0000000E/.test(afFinal.error ?? '')) {
  ok('bind recusado → failed com o command_status (não finge sent)')
} else nok('bind recusado → failed', JSON.stringify(afFinal))

// ---------- 3. sem rota ----------
const mov = await req(`/api/orgs/${orgId}/sms/messages`, {
  token, method: 'POST', body: { to: '912000111', body: 'teste' },
})
if (mov.status === 422 && /Movicel/.test(mov.json?.error ?? '') && /USB/.test(mov.json?.error ?? '')) {
  ok(`Movicel sem contrato e sem USB → 422 «${mov.json.error}»`)
} else nok('sem rota → 422', `${mov.status} ${JSON.stringify(mov.json)}`)
const estrangeiro = await req(`/api/orgs/${orgId}/sms/messages`, {
  token, method: 'POST', body: { to: '+351912345678', body: 'teste' },
})
if (estrangeiro.status === 422) ok('número fora de Angola → 422')
else nok('número fora de Angola → 422', `${estrangeiro.status} ${JSON.stringify(estrangeiro.json)}`)

// ---------- 4. idempotência ----------
const chave = `e2e-${marca}`
const i1 = await req(`/api/orgs/${orgId}/sms/messages`, {
  token, method: 'POST', body: { to: '923000999', body: 'uma vez' }, headers: { 'Idempotency-Key': chave },
})
const i2 = await req(`/api/orgs/${orgId}/sms/messages`, {
  token, method: 'POST', body: { to: '923000999', body: 'uma vez' }, headers: { 'Idempotency-Key': chave },
})
const lista = (await req(`/api/orgs/${orgId}/sms/messages?page_size=100`, { token })).json?.items ?? []
if (i1.status === 202 && i2.status === 202 && i1.json?.id === i2.json?.id && lista.filter((m) => m.body === 'uma vez').length === 1) {
  ok('a mesma Idempotency-Key devolve a mesma mensagem e não cria outra')
} else nok('idempotência', `${i1.json?.id} vs ${i2.json?.id}`)
const grande = await req(`/api/orgs/${orgId}/sms/messages?page_size=101`, { token })
if (grande.status === 400) ok('page_size acima de 100 é recusado, não cortado em silêncio')
else nok('page_size > 100', `${grande.status}`)

// ---------- 5. SMS de reunião: convite e lembrete ----------
{
  const dominio = email.split('@')[1]
  await req(`/api/orgs/${orgId}/settings`, { token, method: 'POST', body: { domain: 'meet.exemplo.ao', retention_days: 0 } })
  const membro = async (nome, telefone) => {
    const r = await req(`/api/orgs/${orgId}/employees`, {
      token, method: 'POST', body: { email: `${nome}-${marca}@${dominio}`, username: `${nome}-${marca}`, password: PW },
    })
    const t = await req(`/api/orgs/${orgId}/employees/${r.json?.user_id}/phone`, { token, method: 'PUT', body: { phone: telefone } })
    if (t.status !== 200) nok(`telefone de ${nome}`, `${t.status} ${JSON.stringify(t.json)}`)
    return { userId: r.json?.user_id, email: `${nome}-${marca}@${dominio}` }
  }
  const ines = await membro('ines', '923 700 800') // Unitel, recebe
  const jose = await membro('jose', '923 700 900') // Unitel, desligou SMS de reunião
  const kiala = await membro('kiala', '912 700 000') // Movicel: sem rota
  const loginJose = await req('/api/auth/login', { method: 'POST', body: { email: jose.email, password: PW } })
  await req('/api/users/me/sms-preferences', {
    token: loginJose.json?.access_token, method: 'PUT', body: { meeting_opt_out: true },
  })

  const antesReuniao = submits.length
  // Começa daqui a 4 min com lembrete 5 min antes: o lembrete já está vencido e
  // o próximo varrimento (≤ 20 s) tem de o enviar sem mais nenhum pedido.
  const inicio = new Date(Date.now() + 4 * 60_000)
  const titulo = 'Revisão das acções do trimestre'
  const r = await req('/api/meetings', {
    token, method: 'POST',
    body: {
      title: titulo, starts_at: inicio.toISOString(), duration_min: 30,
      invitee_ids: [ines.userId, jose.userId, kiala.userId], sms_invite: true, sms_reminder_min: 5,
    },
  })
  const saltos = Object.fromEntries((r.json?.sms?.invite?.skipped ?? []).map((x) => [x.user_id, x.reason]))
  if (r.status === 200 && r.json?.sms?.invite?.queued === 1 && saltos[jose.userId] === 'sms.recipient_opted_out' && saltos[kiala.userId] === 'sms.no_route') {
    ok('agendar com sms_invite: 1 convite em fila; opt-out e sem rota saltados com código')
  } else nok('agendar com sms_invite', `${r.status} ${JSON.stringify(r.json?.sms ?? r.json)}`)

  const fim = Date.now() + 60_000
  let paraInes = []
  while (Date.now() < fim) {
    paraInes = submits.slice(antesReuniao).map(submitHeader).filter((h) => h.dest === '244923700800')
    if (paraInes.length >= 2) break
    await esperar(500)
  }
  const hora = new Intl.DateTimeFormat('pt-PT', { timeZone: 'Africa/Luanda', hour: '2-digit', minute: '2-digit', hour12: false }).format(inicio)
  const convite = paraInes.find((h) => /convite/.test(h.text ?? ''))
  const lembrete = paraInes.find((h) => /comeca/.test(h.text ?? ''))
  const link = 'https://meet.exemplo.ao/#/calendar'
  if (convite && convite.dataCoding === 0 && convite.text.includes(`as ${hora} WAT`) && convite.text.endsWith(link)
      && convite.text.includes('Revisao das accoes')) {
    ok(`o convite chegou ao SMSC em GSM-7: «${convite.text}»`)
  } else nok('o convite chegou ao SMSC', JSON.stringify(paraInes))
  if (lembrete && lembrete.dataCoding === 0 && lembrete.text.includes(`as ${hora} WAT`) && lembrete.text.endsWith(link)) {
    ok(`o lembrete chegou ao SMSC pelo varrimento, sem outro pedido: «${lembrete.text}»`)
  } else nok('o lembrete chegou ao SMSC pelo varrimento', JSON.stringify(paraInes))
  const outros = submits.slice(antesReuniao).map(submitHeader).filter((h) => ['244923700900', '244912700000'].includes(h.dest))
  if (outros.length === 0) ok('nada chegou ao SMSC para quem desligou nem para quem não tem rota')
  else nok('nada para opt-out / sem rota', JSON.stringify(outros))

  const daReuniao = ((await req(`/api/orgs/${orgId}/sms/messages?page_size=100`, { token })).json?.items ?? [])
    .filter((m) => m.meeting_id === r.json?.id)
  const finais = []
  for (const m of daReuniao) finais.push(await ateEstado(orgId, token, m.id, ['sent', 'failed']))
  const tipos = finais.map((m) => `${m?.purpose}:${m?.status}:${m?.segments}`).sort()
  if (JSON.stringify(tipos) === JSON.stringify(['meeting_invite:sent:1', 'meeting_reminder:sent:1'])
      && finais.every((m) => m.recipient_user_id === ines.userId)) {
    ok('convite e lembrete ficam sent, 1 segmento cada, ligados à reunião e à Inês')
  } else nok('estado das mensagens da reunião', JSON.stringify(tipos))

  // Um segundo varrimento não lembra outra vez.
  await esperar(25_000)
  const repetidos = submits.slice(antesReuniao).map(submitHeader).filter((h) => h.dest === '244923700800')
  if (repetidos.length === 2) ok('o varrimento seguinte não repete o lembrete')
  else nok('o lembrete não se repete', `${repetidos.length} submits para a Inês`)
}

// ---------- 6. agente verdadeiro contra o USB desta máquina ----------
if (process.env.AGENT_BIN) {
  const gw = await req(`/api/orgs/${orgId}/sms/gateways`, { token, method: 'POST', body: { name: 'e2e' } })
  const agente = spawn(process.env.AGENT_BIN, ['--server', API, '--token', gw.json.token], {
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  let saida = ''
  agente.stdout.on('data', (d) => (saida += d))
  agente.stderr.on('data', (d) => (saida += d))
  let devs = []
  for (let t = 0; t < 40 && devs.length === 0; t++) {
    await esperar(500)
    devs = (await req(`/api/orgs/${orgId}/sms/devices`, { token })).json ?? []
  }
  if (saida.includes(gw.json.token)) nok('o agente não escreve o token no log', 'o token apareceu na saída')
  else ok('o agente não escreve o token no log')
  const gws = (await req(`/api/orgs/${orgId}/sms/gateways`, { token })).json ?? []
  if (devs.length > 0 && gws.find((g) => g.id === gw.json.id)?.online) {
    ok(`o agente real reportou ${devs.length} dispositivo(s) USB e o gateway está online`)
    for (const d of devs) console.log(`      · ${d.vendor_id}:${d.product_id} ${d.product ?? ''} → ${d.kind}${d.capable ? ' (capaz)' : ''}`)
  } else nok('o agente real reportou o inventário', `${devs.length} dispositivos; saída:\n${saida.slice(0, 800)}`)
  // O caso do pedido: um telefone Android ligado sem modem exposto. Sem telefone, qualquer incapaz serve.
  const semModem = devs.find((d) => d.kind.startsWith('android')) ?? devs.find((d) => !d.capable)
  if (semModem) {
    const r = await req(`/api/orgs/${orgId}/sms/route`, { token, method: 'PUT', body: { device_id: semModem.id } })
    if (r.status === 422 && r.json?.error) ok(`escolher «${semModem.product}» é recusado (422): «${r.json.error.slice(0, 90)}…»`)
    else nok('dispositivo incapaz não pode ser escolhido', `${r.status} ${JSON.stringify(r.json)}`)
  }
  agente.kill('SIGTERM')
} else {
  console.log('  · AGENT_BIN não definido — o agente real não foi exercitado')
}

smsc.close()
console.log(`\n=== ${passou} passaram, ${falhou} falharam ===`)
process.exit(falhou ? 1 : 0)
