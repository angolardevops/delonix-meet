#!/usr/bin/env node
// MFA por TOTP, ponta a ponta contra um servidor a sério.
//
// O código gerador está implementado AQUI, em JavaScript, de propósito: é uma
// segunda implementação independente do RFC 6238. Se concordar com a de Rust,
// isso é evidência a sério de que o algoritmo está certo — muito melhor do que
// a de Rust concordar consigo própria.
import crypto from 'node:crypto'

const API = process.env.API ?? 'http://127.0.0.1:8180'
const PW = 'UmaPasswordForte123!'
let passou = 0, falhou = 0

const ok = (n) => { passou++; console.log(`  ✓ ${n}`) }
const nok = (n, d) => { falhou++; console.log(`  ✗ ${n}\n      ${d}`) }

async function req(path, { token, method = 'GET', body } = {}) {
  const r = await fetch(`${API}${path}`, {
    method,
    headers: { ...(token ? { Authorization: `Bearer ${token}` } : {}), ...(body ? { 'Content-Type': 'application/json' } : {}) },
    ...(body ? { body: JSON.stringify(body) } : {}),
  })
  return { status: r.status, json: await r.json().catch(() => null) }
}

// --- TOTP em JavaScript (RFC 6238) ---
const ALFABETO = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567'
function base32Decode(s) {
  let bits = 0, buffer = 0
  const out = []
  for (const c of s.replace(/[=\s]/g, '').toUpperCase()) {
    const v = ALFABETO.indexOf(c)
    if (v < 0) throw new Error(`caractere base32 inválido: ${c}`)
    buffer = (buffer << 5) | v
    bits += 5
    if (bits >= 8) { bits -= 8; out.push((buffer >> bits) & 0xff) }
  }
  return Buffer.from(out)
}
function totp(secretB32, agoraSegundos = Math.floor(Date.now() / 1000), passo = 30, digitos = 6) {
  const contador = Buffer.alloc(8)
  contador.writeBigUInt64BE(BigInt(Math.floor(agoraSegundos / passo)))
  const mac = crypto.createHmac('sha1', base32Decode(secretB32)).update(contador).digest()
  const off = mac[19] & 0x0f
  const bin = ((mac[off] & 0x7f) << 24) | (mac[off + 1] << 16) | (mac[off + 2] << 8) | mac[off + 3]
  return String(bin % 10 ** digitos).padStart(digitos, '0')
}

// Vectores do RFC 6238 — se estes falharem, o gerador de teste está errado e
// tudo o que vem a seguir não prova nada.
{
  const semente = 'GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ' // "12345678901234567890" em base32
  const casos = [[59, '287082'], [1111111109, '081804'], [1234567890, '005924']]
  const maus = casos.filter(([t, e]) => totp(semente, t) !== e)
  if (maus.length) { console.error('o gerador de teste não bate com o RFC 6238:', maus); process.exit(1) }
  console.log('  · gerador de teste validado contra os vectores do RFC 6238\n')
}

const marca = Math.random().toString(36).slice(2, 8)
const email = `mfa${marca}@mfa${marca}.local`
await req('/api/auth/register', { method: 'POST', body: { org_name: `MFA ${marca}`, email, username: `mfa${marca}`, password: PW } })
const tok = (await req('/api/auth/login', { method: 'POST', body: { email, password: PW } })).json.access_token

console.log('--- inscrição ---')
let e = await req('/api/users/me/mfa', { token: tok })
e.json?.enabled === false && e.json?.pending === false ? ok('conta nova não tem MFA') : nok('estado inicial', JSON.stringify(e.json))

const insc = await req('/api/users/me/mfa/enrol', { token: tok, method: 'POST' })
insc.json?.secret ? ok('inscrição devolve segredo e URI otpauth') : nok('inscrever', JSON.stringify(insc.json))
const segredo = insc.json.secret
insc.json.otpauth_uri?.includes('otpauth://totp/') ? ok('URI otpauth bem formado') : nok('URI', insc.json.otpauth_uri)

e = await req('/api/users/me/mfa', { token: tok })
e.json?.pending === true && e.json?.enabled === false ? ok('fica PENDENTE até ser confirmado') : nok('pendente', JSON.stringify(e.json))

console.log('\n--- login enquanto pendente ---')
let l = await req('/api/auth/login', { method: 'POST', body: { email, password: PW } })
l.json?.access_token ? ok('inscrição por confirmar NÃO tranca a conta') : nok('login pendente', JSON.stringify(l.json))

console.log('\n--- activação ---')
const mau = await req('/api/users/me/mfa/activate', { token: tok, method: 'POST', body: { code: '000000' } })
mau.status === 401 ? ok('código errado não activa → 401') : nok('activar com código errado', `HTTP ${mau.status}`)

const act = await req('/api/users/me/mfa/activate', { token: tok, method: 'POST', body: { code: totp(segredo) } })
const backup = act.json?.backup_codes
Array.isArray(backup) && backup.length === 10 ? ok('activação devolve 10 códigos de recuperação') : nok('activar', JSON.stringify(act.json))

console.log('\n--- login COM segundo factor ---')
l = await req('/api/auth/login', { method: 'POST', body: { email, password: PW } })
l.json?.mfa_required === true && l.json?.mfa_token && !l.json?.access_token
  ? ok('a password sozinha já NÃO produz sessão — devolve desafio')
  : nok('desafio', JSON.stringify(l.json).slice(0, 200))
const desafio = l.json.mfa_token

const semCodigo = await req('/api/users/me', { token: desafio })
semCodigo.status === 401 ? ok('o token de desafio NÃO abre a API → 401') : nok('desafio como access token', `HTTP ${semCodigo.status}`)

const errado = await req('/api/auth/mfa', { method: 'POST', body: { mfa_token: desafio, code: '000000' } })
errado.status === 401 ? ok('código errado no login → 401') : nok('código errado', `HTTP ${errado.status}`)

// A ACTIVAÇÃO consumiu o código daquela janela. Reutilizá-lo para entrar é um
// replay, e tem de ser recusado — mesmo sendo uma operação diferente. Não é
// óbvio, e é por isso que se testa explicitamente.
const codigoDaActivacao = totp(segredo)
const replayEntreOperacoes = await req('/api/auth/mfa', { method: 'POST', body: { mfa_token: desafio, code: codigoDaActivacao } })
replayEntreOperacoes.status === 401
  ? ok('o código usado para ACTIVAR não serve para entrar (anti-replay entre operações)')
  : nok('replay entre operações', `HTTP ${replayEntreOperacoes.status} — o código da activação foi reaceite`)

// Espera pela janela seguinte. É o único ponto do teste que tem de esperar em
// tempo real: o TOTP é uma função do relógio e não há como o adiantar daqui.
const esperaMs = (30 - (Math.floor(Date.now() / 1000) % 30)) * 1000 + 1500
console.log(`  · a aguardar ${Math.round(esperaMs / 1000)}s pela janela TOTP seguinte`)
await new Promise((r) => setTimeout(r, esperaMs))

const l1b = await req('/api/auth/login', { method: 'POST', body: { email, password: PW } })
const codigo = totp(segredo)
const fim = await req('/api/auth/mfa', { method: 'POST', body: { mfa_token: l1b.json.mfa_token, code: codigo } })
fim.json?.access_token ? ok('código da janela seguinte → sessão emitida') : nok('login com MFA', JSON.stringify(fim.json).slice(0, 200))

console.log('\n--- anti-replay ---')
const l2 = await req('/api/auth/login', { method: 'POST', body: { email, password: PW } })
const replay = await req('/api/auth/mfa', { method: 'POST', body: { mfa_token: l2.json.mfa_token, code: codigo } })
replay.status !== 200
  ? ok(`o MESMO código não serve segunda vez → ${replay.status}`)
  : nok('anti-replay', 'o código foi aceite outra vez dentro da mesma janela de 30 s')

console.log('\n--- códigos de recuperação ---')
const l3 = await req('/api/auth/login', { method: 'POST', body: { email, password: PW } })
const rec = await req('/api/auth/mfa', { method: 'POST', body: { mfa_token: l3.json.mfa_token, code: backup[0] } })
rec.json?.access_token ? ok('código de recuperação entra') : nok('recuperação', JSON.stringify(rec.json).slice(0, 160))

const l4 = await req('/api/auth/login', { method: 'POST', body: { email, password: PW } })
const rec2 = await req('/api/auth/mfa', { method: 'POST', body: { mfa_token: l4.json.mfa_token, code: backup[0] } })
rec2.status === 401 ? ok('o mesmo código de recuperação NÃO serve duas vezes → 401') : nok('recuperação repetida', `HTTP ${rec2.status}`)

const tok2 = rec.json.access_token
e = await req('/api/users/me/mfa', { token: tok2 })
e.json?.backup_codes_left === 9 ? ok('sobram 9 códigos de recuperação') : nok('contagem', JSON.stringify(e.json))

console.log('\n--- desactivação ---')
const dMau = await req('/api/users/me/mfa/disable', { token: tok2, method: 'POST', body: { code: '000000' } })
dMau.status === 401 ? ok('não se desactiva sem código válido → 401') : nok('desactivar sem código', `HTTP ${dMau.status}`)

const dOk = await req('/api/users/me/mfa/disable', { token: tok2, method: 'POST', body: { code: backup[1] } })
dOk.json?.ok ? ok('desactiva com código de recuperação') : nok('desactivar', JSON.stringify(dOk.json))

l = await req('/api/auth/login', { method: 'POST', body: { email, password: PW } })
l.json?.access_token ? ok('depois de desactivar, a password volta a bastar') : nok('login pós-desactivação', JSON.stringify(l.json))

console.log(`\n=== ${passou} passaram, ${falhou} falharam ===`)
process.exit(falhou ? 1 : 0)
