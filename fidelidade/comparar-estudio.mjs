// Cópia do notas-ui-template/comparar.mjs para o ramo frontend/l2-estudio:
// escreve em fidelidade/{app,lado-a-lado} (não nas pastas partilhadas), entra
// UMA vez e reutiliza a sessão (o servidor de validação limita logins), e
// prepara o estado de cada ecrã do Estúdio antes de fotografar.
//
//   node fidelidade/comparar-estudio.mjs --app http://127.0.0.1:5603 [Doc...] [--mobile]
//
// Opções por variável de ambiente:
//   GRAVACAO=<id>   o projecto das legendas/linha de tempo vem desta gravação
//                   (por omissão: a primeira da biblioteca com transcrição)
import { chromium } from '@playwright/test'
import { readFileSync, existsSync, mkdirSync } from 'node:fs'
const DIR = new URL('./', import.meta.url).pathname
const REF = new URL('../../notas-ui-template/ref/', import.meta.url).pathname
const args = process.argv.slice(2)
const appIdx = args.indexOf('--app')
const APP = appIdx >= 0 ? args.splice(appIdx, 2)[1] : 'http://127.0.0.1:5603'
const mobIdx = args.indexOf('--mobile')
const MOBILE = mobIdx >= 0 ? (args.splice(mobIdx, 1), true) : false
const USER = process.env.DX_USER ?? 'demo@delonix.co.ao'
const PASS = process.env.DX_PASS ?? 'demo12345'
const STATE = `${DIR}.sessao.json`
mkdirSync(`${DIR}app`, { recursive: true })
mkdirSync(`${DIR}lado-a-lado`, { recursive: true })

// Os destinos do template, no formulário LOCAL (sem destinos guardados no
// servidor, que ainda não são contrato desta UI). O último fica sem chave.
const DESTINOS = [
  ['YouTube', 'rtmp://a.rtmp.youtube.com/live2', 'k1'],
  ['Facebook', 'rtmps://live-api-s.facebook.com:443/rtmp', 'k2'],
  ['LinkedIn', 'rtmps://1-live.linkedin.com/live', 'k3'],
  ['RTMP personalizado', 'rtmp://parceiro.ao/ch2', ''],
]
async function prepararEstudio() {
  await p.waitForSelector('[data-studio="canvas"]', { state: 'attached', timeout: 20000 })
  await p.locator('[data-studio-grupo="imagem"] [data-studio="camara"]').click().catch(() => {})
  for (let i = 0; i < DESTINOS.length; i++) {
    const [rotulo, url, chave] = DESTINOS[i]
    if (i === 0) await p.locator('[data-studio="destino-editar"]').first().click()
    else await p.locator('[data-studio="destino-adicionar"]').click()
    await p.fill('#st-dest-rotulo', rotulo)
    await p.fill('[data-studio="destino-url"]', url)
    await p.fill('[data-studio="destino-chave"]', chave)
    await p.locator('[data-studio="destino-guardar"]').click()
  }
}

const ROTAS = {
  DelonixStudio: { hash: '/studio', preparar: prepararEstudio },
  DelonixStudioEdit: { hash: '/studio?vista=edicao', gravacao: true },
  DelonixStudioCaptions: { hash: '/studio?vista=legendas', gravacao: true },
  DelonixExports: { hash: '/studio?vista=exportacoes', gravacao: true },
}
const want = args.length ? args : Object.keys(ROTAS)
const size = MOBILE ? { width: 390, height: 844 } : { width: 1440, height: 900 }

const b = await chromium.launch({ args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'] })
const ctx = await b.newContext({
  viewport: size,
  permissions: ['camera', 'microphone'],
  storageState: existsSync(STATE) ? STATE : undefined,
})
const p = await ctx.newPage()
p.on('pageerror', (e) => console.log('pageerror', e.message))

async function entrar() {
  if (process.env.FAKE) {
    // Sem servidor: sessão falsa. Prova layout e estados vazios/erro, não dados.
    await ctx.addInitScript(() => {
      localStorage.setItem('dx_user', JSON.stringify({ id: '00000000-0000-0000-0000-000000000001', email: 'demo@delonix.co.ao', username: 'Demo' }))
      localStorage.setItem('dx_access', 'falso')
      localStorage.setItem('dx_tour_v1', 'done')
    })
    // A biblioteca e o ficheiro da gravação vêm do media-demo local, pela
    // mesma rota (/api/recordings e /api/recordings/<id>) que a app usa.
    const MEDIA = new URL('../../notas-ui-template/media-demo/Forma__o___Arquitect.webm', import.meta.url).pathname
    const item = { id: 'demo-rec-1', room_id: 'r', room_code: 'abc-defg-hij', uploader_id: 'u', uploader_name: 'Demo', filename: 'Formação — Arquitectura de Voz Delonix · sessão 3.webm', size_bytes: 4906458, created_at: new Date().toISOString(), status: 'ready' }
    await ctx.route('**/api/recordings', (r) => r.fulfill({ json: [item] }))
    await ctx.route('**/api/recordings/demo-rec-1', (r) => r.fulfill({ path: MEDIA, contentType: 'video/webm' }))
    return
  }
  await p.goto(`${APP}/#/`)
  await p.waitForTimeout(1500)
  if (await p.locator('.shell').count()) return
  await p.goto(`${APP}/#/login`)
  await p.fill('[data-testid=auth-email]', USER)
  await p.fill('[data-testid=auth-password]', PASS)
  await p.press('[data-testid=auth-password]', 'Enter')
  await p.waitForSelector('.shell', { timeout: 30000 })
  await ctx.storageState({ path: STATE })
}

async function gravacaoComTranscricao() {
  if (process.env.GRAVACAO) return process.env.GRAVACAO
  if (process.env.FAKE) return 'demo-rec-1'
  const lib = await p.evaluate(async () =>
    (await fetch('/api/recordings', { headers: { Authorization: `Bearer ${localStorage.getItem('dx_access')}` } })).json(),
  )
  const rec = Array.isArray(lib) ? (lib.find((x) => x.transcript_status === 'ready') ?? lib.find((x) => x.status === 'ready')) : null
  return rec?.id ?? null
}

await entrar()
for (const doc of want) {
  const r = ROTAS[doc]
  if (!r) { console.log('sem rota', doc); continue }
  let hash = r.hash
  if (r.gravacao) {
    const id = await gravacaoComTranscricao()
    if (id) hash += `&gravacao=${id}`
  }
  await p.goto(`${APP}/#${hash}`)
  await p.waitForLoadState('networkidle', { timeout: 20000 }).catch(() => {})
  if (r.preparar) await r.preparar()
  await p.waitForTimeout(r.gravacao ? 6000 : 3000)
  const suf = MOBILE ? '-390' : ''
  await p.screenshot({ path: `${DIR}app/${doc}${suf}.png` })
  const ref = `${REF}${doc}.png`
  if (MOBILE || !existsSync(ref)) { console.log('ok (só app)', doc); continue }
  const img = (f) => 'data:image/png;base64,' + readFileSync(f).toString('base64')
  const cmp = await b.newPage({ viewport: { width: 2896, height: 940 } })
  await cmp.setContent(`<body style="margin:0;background:#222;font:12px monospace;color:#ccc;display:flex;gap:16px;padding:0">
    <div><div style="height:20px;padding:2px 6px">TEMPLATE · ${doc}</div><img src="${img(ref)}" width="1440" height="900"></div>
    <div><div style="height:20px;padding:2px 6px">APP · ${APP}</div><img src="${img(`${DIR}app/${doc}.png`)}" width="1440" height="900"></div></body>`)
  await cmp.screenshot({ path: `${DIR}lado-a-lado/${doc}.png` })
  await cmp.close()
  console.log('ok', doc)
}
await b.close()
