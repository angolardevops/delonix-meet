#!/usr/bin/env node
// Drain do nó: SIGTERM a um servidor A SÉRIO, com uma chamada a decorrer.
//
// O que se verifica é a diferença entre uma actualização que ninguém nota e
// uma que derruba todas as reuniões do pod. Antes: o /health devolvia `ok`
// durante o encerramento, o K8s continuava a mandar entradas novas, e ao fim
// do prazo o SIGKILL matava tudo de uma vez.
import { chromium } from '@playwright/test'
import { execFileSync } from 'node:child_process'
import WebSocket from 'ws'
const API='http://127.0.0.1:8180', APP=process.env.APP||'http://localhost:5176', PW='UmaPasswordForte123!'
const j=(u,o={})=>fetch(u,{...o,headers:{...(o.token?{Authorization:`Bearer ${o.token}`}:{}),...(o.body?{'Content-Type':'application/json'}:{})}}).then(async r=>({s:r.status,j:await r.json().catch(()=>null)}))
const sleep=ms=>new Promise(r=>setTimeout(r,ms))
const code=(u)=>fetch(u).then(r=>r.status).catch(()=>0)
let falhas=0; const chk=(c,n)=>{console.log(`  ${c?'✓':'✗'} ${n}`); if(!c) falhas++}

const m=Math.random().toString(36).slice(2,7), email=`dr${m}@dr${m}.local`
await j(`${API}/api/auth/register`,{method:'POST',body:JSON.stringify({org_name:`DR ${m}`,email,username:`dr${m}`,password:PW})})
const tok=(await j(`${API}/api/auth/login`,{method:'POST',body:JSON.stringify({email,password:PW})})).j.access_token
const sala=(await j(`${API}/api/rooms`,{token:tok,method:'POST',body:JSON.stringify({name:'drain',topology:'sfu'})})).j
const jr=(await j(`${API}/api/rooms/${sala.code}/join`,{token:tok,method:'POST'})).j

const b=await chromium.launch({args:['--use-fake-ui-for-media-stream','--use-fake-device-for-media-stream']})
const p=await (await b.newContext({ ignoreHTTPSErrors: true })).newPage()
const avisos=[]
await p.exposeFunction('__drenou', (ms)=>avisos.push(ms))
await p.addInitScript(()=>{ window.__marcarDrain = true })
await p.goto(`${APP}/e2e/harness.html?token=${encodeURIComponent(jr.room_token)}&code=${sala.code}&access=${encodeURIComponent(tok)}`,{waitUntil:'domcontentloaded',timeout:120000})
for(let k=0;k<25;k++){ await sleep(1500); if(await p.evaluate(()=>window.__dlx.ready)) break }
chk(await p.evaluate(()=>window.__dlx.ready), 'chamada estabelecida antes do drain')

chk(await code(`${API}/ready`)===200, 'antes do SIGTERM, /ready = 200')

// Regista o aviso de drain que chega pelo fio.
await p.evaluate(()=>{ window.__avisos=[]; window.__dlx.aoDrenar = (ms)=>window.__avisos.push(ms) })

const pid = Number(execFileSync('pgrep',['-x','delonix-server']).toString().trim().split('\n')[0])
console.log(`  · SIGTERM ao pid ${pid}`)
process.kill(pid, 'SIGTERM')

await sleep(1200)
chk(await code(`${API}/ready`)===503, 'logo após SIGTERM, /ready = 503 (sai dos endpoints)')
chk(await code(`${API}/health`)===200, 'mas /health continua 200 — um pod a drenar não deve ser REINICIADO')

// A chamada tem de CONTINUAR viva durante o drain.
const vivaDurante = await p.evaluate(async()=>{ const q=await window.__dlx.qos(); return q!==null })
chk(vivaDurante, 'a chamada em curso CONTINUA durante o drain')

// Espera-se PELO AVISO, não por um número de segundos: o servidor só o emite
// depois de `DRAIN_READINESS_SECS` (o tempo que dá ao balanceador para o
// retirar), e esse valor é definido no workflow do CI. Um `sleep` fixo aqui
// acopla o teste a uma variável declarada noutro ficheiro — corrido em local
// sem ela, o teste falhava com «não recebeu aviso», que aponta ao código.
let recebeu = []
for (let k = 0; k < 60; k++) {
  recebeu = await p.evaluate(()=>window.__avisos||[])
  if (recebeu.length > 0) break
  await sleep(500)
}
chk(recebeu.length>0, `o participante recebe aviso para migrar (reconnect_in_ms=${recebeu[0]})`)

// Uma entrada NOVA numa sala que este nó não tem é recusada com 503.
const sala2=(await j(`${API}/api/rooms`,{token:tok,method:'POST',body:JSON.stringify({name:'nova',topology:'sfu'})})).j
if (sala2?.code) {
  const jr2=(await j(`${API}/api/rooms/${sala2.code}/join`,{token:tok,method:'POST'})).j
  // Tem de ser um WebSocket a sério: um `fetch` simples não traz os
  // cabeçalhos de upgrade, e o axum recusa-o com 400 ANTES de a verificação de
  // drain correr — o teste media a ferramenta, não o servidor.
  const estado = await new Promise((resolve) => {
    const ws = new WebSocket(`${API.replace(/^http/, 'ws')}/ws?token=${encodeURIComponent(jr2.room_token)}&room=${sala2.code}`)
    const t = setTimeout(() => { ws.terminate(); resolve('sem-resposta') }, 8000)
    ws.on('unexpected-response', (_req, res) => { clearTimeout(t); ws.terminate(); resolve(res.statusCode) })
    ws.on('open', () => { clearTimeout(t); ws.close(); resolve('aceite') })
    ws.on('error', () => { clearTimeout(t); resolve('erro') })
  })
  chk(estado===503, `entrada numa sala NOVA é recusada com 503 → ${estado}`)
} else { chk(false,'não consegui criar sala nova (o servidor já fechou?)') }

await b.close()
// O processo tem de FECHAR sozinho quando as salas esvaziarem.
let saiu=false
for(let k=0;k<40;k++){ await sleep(1000); try{ execFileSync('pgrep',['-x','delonix-server']) }catch{ saiu=true; break } }
chk(saiu, 'o processo fecha sozinho depois de as salas esvaziarem')

console.log(`\n=== ${falhas===0?'TODAS PASSARAM':falhas+' FALHARAM'} ===`)
process.exit(falhas?1:0)
