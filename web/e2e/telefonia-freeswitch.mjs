#!/usr/bin/env node
// Telefonia (ADR-0009) contra um FreeSWITCH REAL e um servidor e Postgres a sério.
//
// O que prova, medido:
//   1. mod_xml_curl (directory, purpose=gateways): os troncos criados na API
//      aparecem como gateways dlx-<id> depois de «Reiniciar registo».
//   2. `sofia xmlstatus gateway` lido pelo SipControl e mostrado nos troncos.
//   3. originate com origination_uuid e failover real entre dois gateways
//      (o primeiro responde 503, o segundo atende), latência medida.
//   4. mod_json_cdr: os CDRs de cada tentativa chegam, com custo ao preço em
//      vigor; o reenvio do mesmo ficheiro é idempotente.
//   5. mod_xml_curl (dialplan): uma chamada que ENTRA no FreeSWITCH pelo perfil
//      do PBX é encaminhada pelo plano servido pelo servidor.
//   6. limit_execute: com o tronco a 1 canal ocupado, a segunda chamada é
//      recusada; a emergência passa na mesma (sem limite).
//
// Topologia (tudo em 127.0.0.1, num só FreeSWITCH, ver .fs-telecom/conf):
//   perfil pbx 5160 (contexto delonix-outbound, via xml_curl) · external 5180
//   (gateways dlx-*) · carrier 5190 (atende; `…000` ocupado; `…777` chamada
//   longa) · carrier-down 5191 (503 a tudo).
//
// Não prova: operadoras reais, TLS/SRTP, NAT, nem o Kamailio.
//
// Uso:  API=http://127.0.0.1:8430 ESL=127.0.0.1:8121 ESL_PASSWORD=… VOICE_SECRET=… \
//       FS_CONTAINER=fs-telecom node web/e2e/telefonia-freeswitch.mjs
import net from 'node:net'
import { execFileSync } from 'node:child_process'

const API = process.env.API ?? 'http://127.0.0.1:8430'
const [ESL_HOST, ESL_PORT] = (process.env.ESL ?? '127.0.0.1:8121').split(':')
const ESL_PASSWORD = process.env.ESL_PASSWORD
const VOICE_SECRET = process.env.VOICE_SECRET
const FS = process.env.FS_CONTAINER ?? 'fs-telecom'
const PW = 'UmaPasswordForte123!'
if (!ESL_PASSWORD || !VOICE_SECRET) {
  console.error('ESL_PASSWORD e VOICE_SECRET são obrigatórios')
  process.exit(2)
}

let passou = 0
let falhou = 0
const medidas = {}
const ok = (n) => {
  passou++
  console.log(`  ✓ ${n}`)
}
const nok = (n, d) => {
  falhou++
  console.log(`  ✗ ${n}\n      ${d}`)
}
const check = (n, cond, d) => (cond ? ok(n) : nok(n, typeof d === 'string' ? d : JSON.stringify(d)))
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

async function req(path, { token, method = 'GET', body, headers = {} } = {}) {
  const r = await fetch(`${API}${path}`, {
    method,
    headers: {
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
      ...(body ? { 'Content-Type': 'application/json' } : {}),
      ...headers,
    },
    ...(body ? { body: typeof body === 'string' ? body : JSON.stringify(body) } : {}),
  })
  let json = null
  try {
    json = await r.json()
  } catch {}
  return { status: r.status, json }
}

/** `api <cmd>` pelo Event Socket (uma ligação por comando). */
function esl(cmd) {
  return new Promise((resolve, reject) => {
    const s = net.connect(Number(ESL_PORT), ESL_HOST)
    let buf = ''
    let stage = 'auth'
    const t = setTimeout(() => {
      s.destroy()
      reject(new Error(`ESL sem resposta a ${cmd}`))
    }, 20000)
    s.on('data', (d) => {
      buf += d.toString()
      for (;;) {
        const end = buf.indexOf('\n\n')
        if (end < 0) return
        const head = buf.slice(0, end)
        const len = Number((head.match(/Content-Length: (\d+)/) || [])[1] ?? 0)
        if (buf.length < end + 2 + len) return
        const body = buf.slice(end + 2, end + 2 + len)
        buf = buf.slice(end + 2 + len)
        if (stage === 'auth' && head.includes('auth/request')) {
          s.write(`auth ${ESL_PASSWORD}\n\n`)
          stage = 'authed'
        } else if (stage === 'authed' && head.includes('command/reply')) {
          if (!head.includes('+OK')) return reject(new Error('ESL recusou a password'))
          s.write(`api ${cmd}\n\n`)
          stage = 'api'
        } else if (stage === 'api' && head.includes('api/response')) {
          clearTimeout(t)
          s.end()
          resolve(body)
        }
      }
    })
    s.on('error', reject)
  })
}

const basic = 'Basic ' + Buffer.from(`freeswitch:${VOICE_SECRET}`).toString('base64')
const marca = Math.random().toString(36).slice(2, 8)
const DOMAIN = `pbx-${marca}.test`
/**
 * Uma chamada pelo plano de marcação da organização. Entrava pelo perfil do PBX
 * (5160) com o domínio da org no Request-URI, e o servidor ia buscar a org a
 * esse domínio; desde a R292 a organização que paga é só a variável que a
 * plataforma põe no canal — o host do pedido é escrito por quem liga. Nasce
 * num canal `loopback` com essa variável, como em scripts/troncos-prova.sh
 * (sem fixar o codec, o loopback oferecia L16, que operadora nenhuma aceita).
 * `org` só existe mais abaixo: a função é chamada depois.
 */
const viaPlano = (n) => `loopback/${n}/delonix-outbound`
const VARS_PLANO = () => `delonix_org_id=${org},delonix_cdr_skip=true,absolute_codec_string=PCMA,export_vars=absolute_codec_string`
console.log(`\n=== Telefonia contra FreeSWITCH real (marca ${marca}) ===\n`)

// ---- org, troncos, plano ----
const email = `admin@tel${marca}.local`
await req('/api/auth/register', { method: 'POST', body: { org_name: `Tel ${marca}`, email, username: `tel-${marca}`, password: PW } })
const login = await req('/api/auth/login', { method: 'POST', body: { email, password: PW } })
const token = login.json.access_token
const org = (await req('/api/orgs', { token })).json[0].id
const T = (p) => `/api/orgs/${org}/telephony${p}`

const down = await req(T('/trunks'), {
  token, method: 'POST',
  body: { name: `Em baixo ${marca}`, short_code: 'DWN', host: '127.0.0.1', port: 5191, transport: 'udp', srtp: 'off', register: false, max_channels: 5, price_per_min: { amount: '9.40', currency: 'AOA' } },
})
const up = await req(T('/trunks'), {
  token, method: 'POST',
  body: { name: `Teste ${marca}`, short_code: 'TST', host: '127.0.0.1', port: 5190, transport: 'udp', srtp: 'off', register: false, max_channels: 1, price_per_min: { amount: '8.90', currency: 'AOA' } },
})
check('cria dois troncos', down.status === 201 && up.status === 201, [down, up])
const A = down.json.id
const B = up.json.id
const plan = await req(T('/dial-plan'), {
  token, method: 'PUT',
  body: { rules: [
    { pattern: '9XXXXXXXX', description: 'Móvel nacional', action: 'external', trunk_id: A, fallback_trunk_id: B, record: true },
    { pattern: '112,113,115', description: 'Emergência', action: 'external', trunk_id: B, emergency: true },
  ] },
})
check('grava o plano', plan.status === 200, plan)
const sip = await req(T('/sip-settings'), { token, method: 'PUT', body: { domain: DOMAIN, transport: 'udp', srtp: 'off', codecs: ['PCMA'] } })
check('grava o domínio SIP da org (decide a org das chamadas que entram)', sip.status === 200, sip)

// ---- 1. gateways por xml_curl ----
const restart = await req(T('/sip-registration/restart'), { token, method: 'POST' })
check('reiniciar registo → 202', restart.status === 202, restart)
let gws = ''
for (let i = 0; i < 20; i++) {
  gws = await esl('sofia status gateway')
  if (gws.includes(`dlx-${A}`) && gws.includes(`dlx-${B}`)) break
  await sleep(500)
}
check('mod_xml_curl carregou os gateways dlx-<id>', gws.includes(`dlx-${A}`) && gws.includes(`dlx-${B}`), gws)

// ---- 2. xmlstatus ----
await sleep(1500)
const xml = await esl(`sofia xmlstatus gateway dlx-${B}`)
medidas.xmlstatus = xml.replace(/\s+/g, ' ').slice(0, 400)
check('sofia xmlstatus gateway devolve <gateway> com <state>', /<gateway>/.test(xml) && /<state>NOREG<\/state>/.test(xml), xml)
const tr = await req(T(`/trunks/${B}`), { token })
medidas.trunk_status = tr.json?.status
check('o tronco mostra o registo medido (not_required) e canais 0', tr.json?.status?.registration === 'not_required' && tr.json?.status?.channels_in_use === 0, tr.json?.status)
const reg = await req(T('/sip-registration'), { token })
medidas.media = reg.json?.media
check('sip-registration lê a versão do FreeSWITCH real', /^1\.11/.test(reg.json?.media?.version ?? ''), reg.json)

// ---- 3. originate com failover ----
const call = await req(T('/test-calls'), { token, method: 'POST', body: { number: '923447108' } })
check('teste rápido aceite (202)', call.status === 202, call)
let done
for (let i = 0; i < 100; i++) {
  done = (await req(T(`/test-calls/${call.json.id}`), { token })).json
  if (done.finished_at) break
  await sleep(200)
}
medidas.test_call = { status: done.status, answer_latency_ms: done.answer_latency_ms, billsec: done.billsec, hangup_cause: done.hangup_cause }
check('atendida pelo segundo tronco depois do 503 do primeiro', done.status === 'answered' && done.answer_latency_ms > 0, done)

// ---- 4. CDRs ----
let cdrs = []
for (let i = 0; i < 30; i++) {
  cdrs = (await req(`${T('/call-records')}?page_size=20`, { token })).json?.items ?? []
  if (cdrs.length >= 2) break
  await sleep(500)
}
const failedA = cdrs.find((c) => c.trunk_id === A)
const answeredB = cdrs.find((c) => c.trunk_id === B && c.outcome === 'answered')
medidas.cdrs = cdrs.map((c) => ({ trunk: c.trunk_name, outcome: c.outcome, cause: c.hangup_cause, billsec: c.billsec, cost: c.cost, recorded: c.recorded }))
check('CDR da tentativa falhada no tronco A', failedA && failedA.outcome === 'failed' && failedA.cost?.amount === '0.0000', cdrs)
check('CDR atendido no tronco B com custo 8.90 Kz/min', answeredB && answeredB.cost?.amount === '8.9000', cdrs)
const files = execFileSync('docker', ['exec', FS, 'ls', '/usr/local/freeswitch/log/json_cdr']).toString().split('\n').filter(Boolean)
const callFile = files.find((f) => f.startsWith(call.json.id))
check('origination_uuid: o 1.º canal tem o id da chamada (ficheiro do CDR)', !!callFile, files)
if (callFile) {
  const raw = execFileSync('docker', ['exec', FS, 'cat', `/usr/local/freeswitch/log/json_cdr/${callFile}`]).toString()
  const again = await req('/internal/v1/telephony/call-records', { method: 'POST', body: raw, headers: { Authorization: basic, 'Content-Type': 'application/json' } })
  check('reenvio do mesmo CDR → 200 duplicate', again.status === 200 && again.json?.duplicate === true, again)
  const n = (await req(`${T('/call-records')}?page_size=100`, { token })).json.items.length
  check('o reenvio não duplica linhas', n === cdrs.length, { n, antes: cdrs.length })
}

// ---- 5. chamada que segue o plano do xml_curl ----
const before = (await req(`${T('/call-records')}?page_size=100`, { token })).json.items.length
const pbxCall = await esl(`originate {${VARS_PLANO()},originate_timeout=15}${viaPlano('923447222')} &park()`)
check('a chamada é encaminhada pelo plano (xml_curl) e atendida', pbxCall.startsWith('+OK'), pbxCall)
await sleep(8000)
const after = (await req(`${T('/call-records')}?page_size=100`, { token })).json.items
const viaPlan = after.slice(0, after.length - before)
medidas.via_pbx = viaPlan.map((c) => ({ trunk: c.trunk_name, outcome: c.outcome, cost: c.cost, recorded: c.recorded }))
check('as pernas B do plano chegam como CDRs (A falha, B atende, gravado)', viaPlan.some((c) => c.trunk_id === B && c.outcome === 'answered' && c.recorded) && viaPlan.some((c) => c.trunk_id === A), viaPlan)
await esl('hupall NORMAL_CLEARING')

// ---- 6. limit_execute ----
const long = esl(`originate {${VARS_PLANO()},originate_timeout=15}${viaPlano('923447777')} &park()`)
await sleep(2500)
const usage = (await esl(`limit_usage hash delonix_trunk ${B}`)).trim()
const trB = (await req(T(`/trunks/${B}`), { token })).json
medidas.limit = { limit_usage: usage, api_channels_in_use: trB.status.channels_in_use, max: trB.max_channels }
check('limit_usage conta o canal ocupado e a API mostra-o', usage === '1' && trB.status.channels_in_use === 1, medidas.limit)
const second = await esl(`originate {${VARS_PLANO()},originate_timeout=10}${viaPlano('923447333')} &park()`)
medidas.limit.second_call = second.trim()
check('com o tronco B cheio (1/1) a segunda chamada é recusada', second.startsWith('-ERR'), second)
const em = await esl(`originate {${VARS_PLANO()},originate_timeout=10}${viaPlano('112')} &park()`)
medidas.limit.emergency = em.trim()
check('a emergência passa com o tronco cheio (sem limit_execute)', em.startsWith('+OK'), em)
await long
await esl('hupall NORMAL_CLEARING')

console.log(`\nMedidas:\n${JSON.stringify(medidas, null, 2)}`)
console.log(`\n${passou} passaram, ${falhou} falharam`)
console.log(`GATEWAYS: dlx-${A} (em baixo) dlx-${B} (atende)`)
process.exit(falhou ? 1 : 0)
