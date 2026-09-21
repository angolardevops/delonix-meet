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
const p=await (await b.newContext({ locale: 'pt-PT', ignoreHTTPSErrors: true })).newPage()
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
  const d=await j(`${API}/api/recordings/${falhada.id}/content`,{token:tok})
  chk(d.s===400, `descarregar uma falhada é recusado com explicação → ${d.s}`)
  chk(typeof d.j?.error==='string' && d.j.error.length>20, 'e a recusa diz PORQUÊ, não um 500 opaco')
}

// ---------------------------------------------------------------------------
// A API está certa; falta o ECRÃ. O R59 nasceu de a regra de apresentação
// existir só na vista que existia quando foi escrita: a base acrescentou uma
// tabela e um visualizador de biblioteca, e a gravação falhada voltou a
// oferecer reproduzir/descarregar/partilhar sem nenhum conflito de merge.
//
// Na UI reconstruída há DUAS vistas (Lista = tabela, Grelha = cartões) e um
// painel de leitor partilhado. A biblioteca antiga mostrava a causa no
// visualizador; a nova mostra-a NA PRÓPRIA linha/cartão, e a falhada nunca
// chega ao leitor. Este bloco abre as duas vistas e verifica, em cada uma:
// a entrada existe, a causa registada está à vista, não há botão nenhum, e
// não há leitor nem <video> para um ficheiro que não existe.
// ---------------------------------------------------------------------------
if (falhada) {
  const b2 = await chromium.launch()
  const p2 = await (await b2.newContext({ locale: 'pt-PT', ignoreHTTPSErrors: true })).newPage()
  await p2.goto(`${APP}/#/login`, { waitUntil: 'domcontentloaded', timeout: 120000 })
  await p2.waitForSelector('[data-testid=auth-email]', { timeout: 120000 })
  await p2.evaluate(() => localStorage.setItem('dx_tour_v1', 'done'))
  await p2.fill('[data-testid=auth-email]', email)
  await p2.fill('[data-testid=auth-password]', PW)
  await p2.waitForTimeout(1500)
  await p2.locator('[data-testid=auth-submit]').click()
  await p2.waitForFunction(() => !document.querySelector('[data-testid=auth-email]'), null, { timeout: 60000 })

  await p2.goto(`${APP}/#/recordings`, { waitUntil: 'domcontentloaded' })
  await p2.waitForSelector('.rec-table, .rec-grid', { timeout: 60000 })
  const causa = (falhada.failure_reason || '').slice(0, 30)

  // Vista LISTA (omissão): a tabela.
  const linha = p2.locator('.rec-table tr[data-status="failed"]')
  chk(await linha.count() > 0, 'lista: a gravação falhada está na tabela')
  const textoLinha = await linha.first().innerText().catch(() => '')
  chk(/falhad/i.test(textoLinha) && (!causa || textoLinha.includes(causa)),
      'lista: a linha mostra a causa registada, não um erro genérico')
  chk(await p2.locator('.rec-table tr[data-status="failed"] button').count() === 0,
      'lista: zero botões na linha — nem abrir, nem descarregar, nem partilhar (R59)')
  chk(await p2.locator('.rec-table tr[data-status="failed"].dx-row-link').count() === 0,
      'lista: a linha NÃO se apresenta como clicável')
  await linha.first().click()
  await p2.waitForTimeout(1000)
  chk(await p2.locator('.rec-panel.is-open').count() === 0, 'lista: carregar na linha não abre o leitor')
  const nomeFalhada = falhada.filename.replace(/\.(webm|mp4|mkv)$/i, '')
  const tituloPainel = await p2.locator('.rec-panel h2').first().innerText({ timeout: 1000 }).catch(() => '')
  chk(tituloPainel !== nomeFalhada, 'lista: o painel do leitor nunca mostra a falhada')
  chk(await p2.locator('video').count() === 0, 'lista: não há elemento <video> para um ficheiro que não existe')

  // Vista GRELHA: os cartões.
  await p2.locator('.rec-views button').nth(1).click()
  await p2.waitForSelector('.rec-grid', { timeout: 10000 })
  chk(await p2.locator('.rec-card[data-status="failed"] .rec-card__thumb.is-failed').count() > 0,
      'grelha: miniatura marcada como falhada')
  chk(await p2.locator('.rec-card[data-status="failed"] button').count() === 0,
      'grelha: a miniatura NÃO é clicável e há zero acções oferecidas (R59)')
  const textoCartao = await p2.locator('.rec-card[data-status="failed"]').first().innerText().catch(() => '')
  chk(!causa || textoCartao.includes(causa), 'grelha: o cartão mostra a causa registada')
  chk(await p2.locator('.rec-panel.is-open').count() === 0 && await p2.locator('video').count() === 0,
      'grelha: nenhum leitor nem <video> para a falhada')
  await b2.close()
}

console.log(`\n=== ${falhas===0?'TODAS PASSARAM':falhas+' FALHARAM'} ===`)
process.exit(falhas?1:0)
