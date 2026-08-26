// Força uma gravação a FALHAR e verifica que ela aparece — em vez de
// desaparecer em silêncio, que era o comportamento anterior.
//
// A falha é forçada de forma realista: grava-se sem media suficiente, o que
// leva o `finalize` ao caminho "nothing recorded". É exactamente o cenário em
// que o silêncio era pior — o anfitrião viu o indicador aceso e ficou a pensar
// que tinha ficheiro.
import { chromium } from '@playwright/test'
const API='http://127.0.0.1:8180', APP=process.env.APP||'http://localhost:5174', PW='UmaPasswordForte123!'
const j=(u,o={})=>fetch(u,{...o,headers:{...(o.token?{Authorization:`Bearer ${o.token}`}:{}),...(o.body?{'Content-Type':'application/json'}:{})}}).then(async r=>({s:r.status,j:await r.json().catch(()=>null)}))
const sleep=ms=>new Promise(r=>setTimeout(r,ms))
let falhas=0; const chk=(c,n)=>{console.log(`  ${c?'✓':'✗'} ${n}`); if(!c) falhas++}

const m=Math.random().toString(36).slice(2,7), email=`rf${m}@rf${m}.local`
await j(`${API}/api/auth/register`,{method:'POST',body:JSON.stringify({org_name:`RF ${m}`,email,username:`rf${m}`,password:PW})})
const tok=(await j(`${API}/api/auth/login`,{method:'POST',body:JSON.stringify({email,password:PW})})).j.access_token
const sala=(await j(`${API}/api/rooms`,{token:tok,method:'POST',body:JSON.stringify({name:'falha',topology:'sfu'})})).j
const jr=(await j(`${API}/api/rooms/${sala.code}/join`,{token:tok,method:'POST'})).j

const b=await chromium.launch({args:['--use-fake-ui-for-media-stream','--use-fake-device-for-media-stream']})
const p=await (await b.newContext({ ignoreHTTPSErrors: true })).newPage()
await p.goto(`${APP}/e2e/harness.html?token=${encodeURIComponent(jr.room_token)}&code=${sala.code}&access=${encodeURIComponent(tok)}`,{waitUntil:'domcontentloaded',timeout:120000})
for(let k=0;k<20;k++){ await sleep(1500); if(await p.evaluate(()=>window.__dlx.ready)) break }
// Grava e pára quase de imediato: não há media que chegue para compor.
await p.evaluate(()=>window.__dlx.gravar(true))
await sleep(700)
await p.evaluate(()=>window.__dlx.gravar(false))
await sleep(6000)
await b.close()

const lista=(await j(`${API}/api/recordings`,{token:tok})).j
const falhada=Array.isArray(lista)?lista.find(r=>r.status==='failed'):null
chk(!!falhada, 'a gravação falhada APARECE na biblioteca (antes desaparecia em silêncio)')
if(falhada){
  chk(!!falhada.failure_reason, `traz uma causa: "${(falhada.failure_reason||'').slice(0,72)}…"`)
  chk(!/\/tmp|ffmpeg exited|errno/i.test(falhada.failure_reason||''), 'a causa NÃO vaza caminhos nem detalhe técnico')
  chk(falhada.size_bytes===0, 'tamanho zero — não é um ficheiro vazio, é ausência de ficheiro')
  const d=await j(`${API}/api/recordings/${falhada.id}`,{token:tok})
  chk(d.s===400, `descarregar uma falhada é recusado com explicação → ${d.s}`)
  chk(typeof d.j?.error==='string' && d.j.error.length>20, 'e a recusa diz PORQUÊ, não um 500 opaco')
}

// ---------------------------------------------------------------------------
// A API está certa; falta o ECRÃ. O R59 nasceu de a regra de apresentação
// existir só na vista que existia quando foi escrita: a base acrescentou uma
// tabela e um visualizador de biblioteca, e a gravação falhada voltou a
// oferecer reproduzir/descarregar/partilhar sem nenhum conflito de merge.
// Este bloco abre as TRÊS vistas e verifica que nenhuma oferece acção.
// ---------------------------------------------------------------------------
if (falhada) {
  const b2 = await chromium.launch()
  const p2 = await (await b2.newContext({ ignoreHTTPSErrors: true })).newPage()
  await p2.goto(`${APP}/#/login`, { waitUntil: 'domcontentloaded', timeout: 120000 })
  await p2.waitForSelector('input[type=email]', { timeout: 120000 })
  await p2.evaluate(() => localStorage.setItem('dx_tour_v1', 'done'))
  await p2.fill('input[type=email]', email)
  await p2.fill('input[type=password]', PW)
  await p2.waitForTimeout(2000)
  await p2.locator('form button.primary').first().click()
  await p2.waitForFunction(() => !document.querySelector('input[type=email]'), null, { timeout: 60000 })

  await p2.goto(`${APP}/#/recordings`, { waitUntil: 'domcontentloaded' })
  await p2.waitForSelector('.rec-item, .rec-card, .rec-table', { timeout: 60000 })

  // Vista de BIBLIOTECA (omissão): a entrada existe e é visível.
  chk(await p2.locator('.rec-item').count() > 0, 'biblioteca: a gravação falhada está na lista')
  await p2.locator('.rec-item').first().click()
  await p2.waitForTimeout(1500)
  // O visualizador mostra a CAUSA registada, não «falha ao carregar o vídeo».
  const textoVisualizador = await p2.locator('.rec-split-viewer').innerText().catch(() => '')
  chk(/media suficiente|falhad/i.test(textoVisualizador),
      'biblioteca: o visualizador mostra a causa registada, não um erro genérico')
  chk(await p2.locator('.rec-split-viewer video').count() === 0,
      'biblioteca: não há elemento <video> para um ficheiro que não existe')

  // Vista de CARTÕES.
  await p2.locator('.seg-btn').nth(1).click()
  await p2.waitForTimeout(800)
  chk(await p2.locator('.rec-card .rec-thumb.failed').count() > 0, 'cartões: miniatura marcada como falhada')
  chk(await p2.locator('.rec-card button.rec-thumb').count() === 0, 'cartões: a miniatura NÃO é clicável')
  chk(await p2.locator('.rec-card .rec-actions button').count() === 0, 'cartões: zero acções oferecidas')

  // Vista de TABELA — a que a base acrescentou e que o R59 apanhou.
  await p2.locator('.seg-btn').nth(2).click()
  await p2.waitForTimeout(800)
  chk(await p2.locator('.rec-table tbody tr').count() > 0, 'tabela: a linha existe')
  chk(await p2.locator('.rec-table .rec-row-actions button').count() === 0,
      'tabela: zero acções oferecidas (o buraco que o merge abriu — R59)')
  chk(await p2.locator('.rec-table button.rec-name-link').count() === 0,
      'tabela: o nome NÃO é um botão que abre o visualizador')
  await b2.close()
}

console.log(`\n=== ${falhas===0?'TODAS PASSARAM':falhas+' FALHARAM'} ===`)
process.exit(falhas?1:0)
