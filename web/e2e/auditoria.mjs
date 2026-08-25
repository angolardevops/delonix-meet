#!/usr/bin/env node
// Auditoria imutável: a cadeia de hash detecta adulteração, e os gatilhos
// recusam UPDATE e DELETE.
//
// O adversário que interessa aqui é alguém COM PRIVILÉGIOS — um administrador a
// apagar o que fez. Por isso o teste ataca a tabela DIRECTAMENTE, com SQL, e
// não pela API: um teste que só usa a API prova que a API não deixa, não que os
// dados estão protegidos.
import { execFileSync } from 'node:child_process'
const API=process.env.API??'http://127.0.0.1:8180', PW='UmaPasswordForte123!'
const PG=process.env.PG??'wt-audit-postgres-1'
const j=(u,o={})=>fetch(u,{...o,headers:{...(o.token?{Authorization:`Bearer ${o.token}`}:{}),...(o.body?{'Content-Type':'application/json'}:{})}}).then(async r=>({s:r.status,j:await r.json().catch(()=>null)}))
// Primeira linha só: o psql imprime também a etiqueta do comando ("INSERT 0 1")
// a seguir ao valor de um RETURNING, e isso entrava no UUID.
const sql=(q)=>execFileSync('docker',['exec',PG,'psql','-U','delonix','-d','delonix_meet','-tAc',q],{stdio:['ignore','pipe','pipe']}).toString().trim().split('\n')[0].trim()
const sqlErro=(q)=>{ try{ sql(q); return null }catch(e){ return (e.stderr?.toString()||e.message) } }
let falhas=0; const chk=(c,n)=>{console.log(`  ${c?'✓':'✗'} ${n}`); if(!c) falhas++}

const m=Math.random().toString(36).slice(2,7), email=`au${m}@au${m}.local`
await j(`${API}/api/auth/register`,{method:'POST',body:JSON.stringify({org_name:`AU ${m}`,email,username:`au${m}`,password:PW})})
const tok=(await j(`${API}/api/auth/login`,{method:'POST',body:JSON.stringify({email,password:PW})})).j.access_token
const org=(await j(`${API}/api/orgs`,{token:tok})).j[0].id

// Gera eventos reais (cada login escreve na trilha).
for(let i=0;i<4;i++) await j(`${API}/api/auth/login`,{method:'POST',body:JSON.stringify({email,password:PW})})
await new Promise(r=>setTimeout(r,600))

console.log('--- cadeia intacta ---')
let v=(await j(`${API}/api/orgs/${org}/audit/verify`,{token:tok})).j
chk(v?.intact===true, `cadeia intacta com ${v?.entries} registos`)
chk(v?.broken_at_seq===null, 'sem quebras assinaladas')

console.log('\n--- os gatilhos recusam edição e remoção ---')
const alvo=sql(`SELECT id FROM audit_logs WHERE audit_chain_key(org_id)='${org}' ORDER BY seq LIMIT 1`)
const eU=sqlErro(`UPDATE audit_logs SET action='apagado' WHERE id=${alvo}`)
chk(/append-only/.test(eU||''), `UPDATE directo é recusado${eU?'':' (NÃO FOI!)'}`)
const eD=sqlErro(`DELETE FROM audit_logs WHERE id=${alvo}`)
chk(/append-only/.test(eD||''), `DELETE directo é recusado${eD?'':' (NÃO FOI!)'}`)

console.log('\n--- e se alguém tiver poder para remover os gatilhos? ---')
// É o teste que interessa: os gatilhos são a primeira barreira, a CADEIA é a
// que sobrevive a um adversário com privilégios de esquema.
sql('ALTER TABLE audit_logs DISABLE TRIGGER trg_audit_append_only')
// Guarda o valor ORIGINAL: repor um valor adivinhado deixaria a cadeia
// partida e o teste seguinte mediria a minha distracção, não o produto.
const acaoOriginal = sql(`SELECT action FROM audit_logs WHERE id=${alvo}`)
sql(`UPDATE audit_logs SET action='acção-inocente' WHERE id=${alvo}`)
v=(await j(`${API}/api/orgs/${org}/audit/verify`,{token:tok})).j
chk(v?.intact===false, 'ALTERAR uma linha é DETECTADO')
chk(/ALTERADO/i.test(v?.detail||''), `e o relatório diz o quê: "${v?.detail}"`)
chk(typeof v?.broken_at_seq==='number', `e onde: registo nº ${v?.broken_at_seq}`)

// Repõe e testa a remoção de uma linha do meio.
sql(`UPDATE audit_logs SET action='${acaoOriginal}' WHERE id=${alvo}`)
v=(await j(`${API}/api/orgs/${org}/audit/verify`,{token:tok})).j
chk(v?.intact===true, 'reposto o conteúdo original, a cadeia volta a fechar')

const meio=sql(`SELECT id FROM audit_logs WHERE audit_chain_key(org_id)='${org}' ORDER BY seq OFFSET 1 LIMIT 1`)
sql(`DELETE FROM audit_logs WHERE id=${meio}`)
v=(await j(`${API}/api/orgs/${org}/audit/verify`,{token:tok})).j
chk(v?.intact===false, 'APAGAR uma linha do meio é DETECTADO')
chk(/apagou|Falta/i.test(v?.detail||''), `e diz que faltam registos: "${v?.detail}"`)
sql('ALTER TABLE audit_logs ENABLE TRIGGER trg_audit_append_only')

console.log('\n--- a trilha sobrevive à conta que a gerou ---')
const m2=Math.random().toString(36).slice(2,7), e2=`sv${m2}@au${m}.local`
// Utilizador na MESMA org (o servidor impõe um domínio por org).
const uid=sql(`INSERT INTO users (email, username, password_hash) VALUES ('${e2}','saiu${m2}','x') RETURNING id`)
sql(`INSERT INTO org_members (org_id, user_id, role) VALUES ('${org}','${uid}','member')`)
sql(`INSERT INTO audit_logs (org_id, actor_id, actor_name, action, target) VALUES ('${org}','${uid}','saiu${m2}','org.settings_changed','teste')`)
sql(`DELETE FROM users WHERE id='${uid}'`)
const lista=(await j(`${API}/api/orgs/${org}/audit?limit=200`,{token:tok})).j
const sobrevivente=Array.isArray(lista)&&lista.find(x=>x.action==='org.settings_changed')
chk(!!sobrevivente, 'o evento de uma conta APAGADA continua na trilha')
chk(sobrevivente?.actor===`saiu${m2}`, `e mantém o nome que o actor tinha então: "${sobrevivente?.actor}"`)

console.log(`\n=== ${falhas===0?'TODAS PASSARAM':falhas+' FALHARAM'} ===`)
process.exit(falhas?1:0)
