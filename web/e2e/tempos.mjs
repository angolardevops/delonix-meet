#!/usr/bin/env node
// Tempos de entrada, medidos numa entrada A SÉRIO: dois Chromium, o SFU Rust,
// e o percurso completo desde o clique até haver media.
//
// Estes números não vêm do `getStats()` — vêm de marcos no cliente. Um teste
// contra duplos provaria a aritmética; o que interessa provar é que os marcos
// são disparados nos sítios certos de uma chamada verdadeira.
import { chromium } from '@playwright/test'
const API='http://127.0.0.1:8180', APP=process.env.APP||'http://localhost:5177', PW='UmaPasswordForte123!'
const j=(u,o={})=>fetch(u,{...o,headers:{...(o.token?{Authorization:`Bearer ${o.token}`}:{}),...(o.body?{'Content-Type':'application/json'}:{})}}).then(async r=>({s:r.status,j:await r.json().catch(()=>null)}))
const sleep=ms=>new Promise(r=>setTimeout(r,ms))
const PG=process.env.PG??'wt-tempo-postgres-1'
const { execFileSync }=await import('node:child_process')
const sql=(q)=>execFileSync('docker',['exec',PG,'psql','-U','delonix','-d','delonix_meet','-tAc',q],{stdio:['ignore','pipe','pipe']}).toString().trim().split('\n')[0].trim()
let falhas=0; const chk=(c,n)=>{console.log(`  ${c?'✓':'✗'} ${n}`); if(!c) falhas++}

const m=Math.random().toString(36).slice(2,7), email=`tp${m}@tp${m}.local`
await j(`${API}/api/auth/register`,{method:'POST',body:JSON.stringify({org_name:`TP ${m}`,email,username:`tp${m}`,password:PW})})
const tok=(await j(`${API}/api/auth/login`,{method:'POST',body:JSON.stringify({email,password:PW})})).j.access_token
const sala=(await j(`${API}/api/rooms`,{token:tok,method:'POST',body:JSON.stringify({name:'tempos',topology:'sfu'})})).j
const jr=(await j(`${API}/api/rooms/${sala.code}/join`,{token:tok,method:'POST'})).j

const b=await chromium.launch({args:['--use-fake-ui-for-media-stream','--use-fake-device-for-media-stream']})
const url=`${APP}/e2e/harness.html?token=${encodeURIComponent(jr.room_token)}&code=${sala.code}&access=${encodeURIComponent(tok)}`
const ps=[]
for(let i=0;i<2;i++){const c=await b.newContext();const p=await c.newPage();await p.goto(url,{waitUntil:'domcontentloaded',timeout:120000});ps.push(p)}
for(let k=0;k<30;k++){ await sleep(1500); const v=await Promise.all(ps.map(p=>p.evaluate(()=>window.__dlx.tempos?.resumo?.()??null))); if(v.every(x=>x&&x.join_ms!==null)) break }
const t = await ps[0].evaluate(()=>window.__dlx.tempos.resumo())
console.log('  tempos medidos:', JSON.stringify(t))
await b.close()

chk(typeof t.join_ms==='number' && t.join_ms>0, `join_ms medido: ${t.join_ms} ms`)
chk(typeof t.ws_ms==='number' && t.ws_ms>0 && t.ws_ms<=t.join_ms, `ws_ms (${t.ws_ms}) medido e ANTERIOR ao join`)
chk(t.first_audio_ms===null || t.first_audio_ms>=0, 'first_audio_ms coerente')
chk(t.ice_restarts===0 && t.reconnects===0, 'chamada limpa: zero reinícios e zero recuperações')

// O arnês não usa o Room.tsx, por isso reporta-se aqui o que a app reportaria.
await j(`${API}/api/rooms/${sala.code}/timings`,{token:tok,method:'POST',body:JSON.stringify(t)})
await sleep(500)
const n = Number(sql(`SELECT count(*) FROM call_timings WHERE room_id='${sala.code?sala.id:''}'`))
chk(n===1, `persistido: ${n} registo em call_timings`)
const guardado = sql(`SELECT join_ms||'/'||ws_ms FROM call_timings WHERE room_id='${sala.id}'`)
chk(guardado===`${t.join_ms}/${t.ws_ms}`, `valores gravados batem certo: ${guardado}`)

console.log('\n--- um cliente a inventar não destrói a média ---')
await j(`${API}/api/rooms/${sala.code}/timings`,{token:tok,method:'POST',body:JSON.stringify({join_ms:999999999, ws_ms:-5, ice_restarts:99999})})
const abs = sql(`SELECT join_ms||'/'||ws_ms||'/'||ice_restarts FROM call_timings WHERE room_id='${sala.id}' ORDER BY id DESC LIMIT 1`)
chk(abs==='600000/0/1000', `valores absurdos são presos: ${abs}`)

const met = await fetch(`${API}/metrics`).then(r=>r.text())
chk(/delonix_join_total \d+/.test(met), 'métricas de entrada expostas em /metrics')
const total = Number(met.match(/delonix_join_total (\d+)/)?.[1])
chk(total>=2, `delonix_join_total = ${total}`)

console.log(`\n=== ${falhas===0?'TODAS PASSARAM':falhas+' FALHARAM'} ===`)
process.exit(falhas?1:0)
