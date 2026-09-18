#!/usr/bin/env node
// S7 — `org::add_employee` não captura uma conta que já é de OUTRA organização.
//
// A regra «tornar-me colega de alguém» estava escrita em `room_access` (fechada
// em S3) e TAMBÉM em `add_employee`, que liga uma conta EXISTENTE por email. O
// domínio da org é a primeira barreira, mas uma org LEGADA com `email_domain`
// vazio salta-a — e o `email_domain` não é editável pela API, por isso este é o
// único caminho que chega à guarda nova. Ataca-se a base de dados DIRECTAMENTE
// para o reproduzir (como a auditoria): esvazia-se o domínio da org do atacante
// e tenta-se puxar o admin de outra org.
//
// Medido a 2026-09-16, ANTES da correcção: a vítima entrava na org do atacante
// com role=admin. DEPOIS: 409 e a vítima não fica membro.
//
// Uso:  PG=<contentor> node web/e2e/captura-empregado.mjs
import { sql as sqlPg } from './pg.mjs'

const API = process.env.API ?? 'http://127.0.0.1:8180'
const PW = 'UmaPasswordForte123!'
// A base é `delonix_meet` no CI; `PGDB=` permite apontar a um banco descartável
// quando se corre localmente contra um servidor que não usa o banco de dev.
const sql = (q) => sqlPg(q, { db: process.env.PGDB ?? 'delonix_meet' })
const j = async (p, o = {}) => {
  const r = await fetch(`${API}${p}`, {
    method: o.method ?? 'GET',
    headers: {
      ...(o.token ? { Authorization: `Bearer ${o.token}` } : {}),
      ...(o.body ? { 'Content-Type': 'application/json' } : {}),
    },
    ...(o.body ? { body: JSON.stringify(o.body) } : {}),
  })
  return { s: r.status, j: await r.json().catch(() => null) }
}
async function novaOrg(sufixo) {
  const email = `admin@${sufixo}.local`
  await j('/api/auth/register', { method: 'POST', body: { org_name: sufixo, email, username: sufixo, password: PW } })
  const l = await j('/api/auth/login', { method: 'POST', body: { email, password: PW } })
  const o = await j('/api/orgs', { token: l.j.access_token })
  return { email, token: l.j.access_token, orgId: o.j?.[0]?.id, userId: l.j.user?.id }
}

let falhas = 0
const chk = (c, n, d = '') => { console.log(`  ${c ? '✓' : '✗'} ${n}${c ? '' : '\n      ' + d}`); if (!c) falhas++ }

const m = 's7' + Math.random().toString(36).slice(2, 7)
const A = await novaOrg(`${m}a`)
const V = await novaOrg(`${m}b`)

// Torna a org do atacante LEGADA (email_domain vazio) — o que a API não deixa
// fazer, mas que existe em dados anteriores à migração 0010.
sql(`UPDATE organizations SET email_domain='' WHERE id='${A.orgId}'`)
chk(sql(`SELECT COALESCE(email_domain,'∅') FROM organizations WHERE id='${A.orgId}'`) === '', 'org do atacante ficou com email_domain vazio (legada)')

// Controlo positivo: numa org legada, adicionar uma conta NOVA continua a funcionar.
const novo = await j(`/api/orgs/${A.orgId}/members`, {
  token: A.token, method: 'POST',
  body: { email: `${m}-novo@qualquer.local`, username: `${m}-novo`, password: PW, role: 'member' },
})
chk(novo.s >= 200 && novo.s < 300, 'controlo positivo: a org legada ainda adiciona uma conta NOVA', `devolveu ${novo.s}`)

// Ataque: puxar o admin da org B (conta que já pertence a outra org).
const antes = sql(`SELECT count(*) FROM org_members WHERE user_id='${V.userId}' AND org_id='${A.orgId}'`)
const r = await j(`/api/orgs/${A.orgId}/members`, { token: A.token, method: 'POST', body: { email: V.email, role: 'admin' } })
chk(r.s === 409, 'add_employee recusa a conta de outra organização', `devolveu ${r.s}: ${JSON.stringify(r.j).slice(0, 140)}`)

const depois = sql(`SELECT count(*) FROM org_members WHERE user_id='${V.userId}' AND org_id='${A.orgId}'`)
chk(antes === '0' && depois === '0', 'e a vítima NÃO ficou membro da org do atacante', `antes=${antes} depois=${depois}`)

// A conta da vítima não foi tocada.
chk(sql(`SELECT count(*) FROM org_members WHERE user_id='${V.userId}' AND org_id='${V.orgId}'`) === '1', 'a vítima continua membro só da sua própria org')

console.log(`\n=== ${falhas === 0 ? 'OK' : falhas + ' FALHA(S)'} ===`)
process.exit(falhas ? 1 : 0)
