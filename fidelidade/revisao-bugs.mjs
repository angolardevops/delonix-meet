// Reprodução dos bugs da revisão do Estúdio (passos no revisao-estudio.md).
// Uso: FASE=antes node fidelidade/revisao-bugs.mjs  (porta em PORTA, por omissão 5647)
import { chromium } from '@playwright/test'
import { mkdirSync, writeFileSync } from 'node:fs'
const APP = `http://127.0.0.1:${process.env.PORTA ?? 5647}`
const FASE = process.env.FASE ?? 'antes'
const OUT = new URL(`../../notas-ui-template/revisao-estudio/${FASE}/`, import.meta.url).pathname
mkdirSync(OUT, { recursive: true })
const STATE = new URL(process.env.PORTA && process.env.PORTA !== '5647' ? `./.sessao-revisao-${process.env.PORTA}.json` : './.sessao-revisao.json', import.meta.url).pathname
const REC = process.env.GRAVACAO ?? 'd96097f9-b617-41bc-94f2-0565dfa72ea8'
const b = await chromium.launch({ args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'] })
const res = {}
const log = (k, v) => { res[k] = v; console.log(k, JSON.stringify(v)) }

async function pagina(w, h) {
  const ctx = await b.newContext({ viewport: { width: w, height: h }, permissions: ['camera', 'microphone'], storageState: STATE })
  const p = await ctx.newPage()
  const erros = []
  p.on('pageerror', (e) => erros.push(e.message.slice(0, 160)))
  p.on('console', (m) => { if (m.type() === 'error') erros.push(m.text().slice(0, 160)) })
  // O refresh token roda: um estado guardado pode já não valer. Entra de novo
  // só quando é preciso (o servidor de validação limita logins).
  const irOriginal = p.goto.bind(p)
  p.goto = async (url, o) => {
    const r = await irOriginal(url, o)
    await p.waitForTimeout(1500)
    if (await p.locator('[data-testid=auth-email]').count()) {
      await p.fill('[data-testid=auth-email]', process.env.DX_USER ?? 'demo@delonix.co.ao')
      await p.fill('[data-testid=auth-password]', process.env.DX_PASS ?? 'demo12345')
      await p.press('[data-testid=auth-password]', 'Enter')
      await p.waitForSelector('.shell', { timeout: 30000 })
      await ctx.storageState({ path: STATE })
      return irOriginal(url, o)
    }
    return r
  }
  return { ctx, p, erros }
}
const docScroll = (p) => p.evaluate(() => ({ vy: document.scrollingElement.scrollHeight - innerHeight, corpo: (() => { const e = document.querySelector('.shell-body'); return e ? { vy: e.scrollHeight - e.clientHeight, vx: e.scrollWidth - e.clientWidth } : null })() }))

// B1 — página com scroll na Emissão com a câmara ligada (1440×900 e 1280×800)
for (const [w, h] of [[1440, 900], [1280, 800]]) {
  const { ctx, p } = await pagina(w, h)
  await p.goto(`${APP}/#/studio`)
  await p.waitForSelector('[data-studio="canvas"]', { state: 'attached', timeout: 30000 })
  await p.locator('[data-studio-grupo="imagem"] [data-studio="camara"]').click({ timeout: 5000 }).catch(() => {})
  await p.waitForTimeout(2500)
  log(`B1 scroll do documento na emissão ${w}x${h}`, await docScroll(p))
  await ctx.close()
}

// B2 — destino: «editar» abre um DIÁLOGO (não um formulário na coluna)
{
  const { ctx, p } = await pagina(1920, 1080)
  await p.goto(`${APP}/#/studio`)
  await p.waitForSelector('[data-studio="destino-editar"]', { timeout: 30000 })
  await p.locator('[data-studio="destino-editar"]').first().click()
  await p.waitForTimeout(600)
  log('B2 destino abre diálogo', await p.evaluate(() => ({ dialogo: !!document.querySelector('[role="dialog"] [data-studio="destino-url"]'), naColuna: !!document.querySelector('.st-col--right [data-studio="destino-url"]:not([role="dialog"] *)') })))
  await p.screenshot({ path: `${OUT}destino-dialogo-1920x1080.png` })
  await ctx.close()
}

// B3/B4 — Edição de uma gravação da biblioteca COM transcrição no servidor
{
  const { ctx, p, erros } = await pagina(1440, 900)
  await p.goto(`${APP}/#/studio?vista=edicao&gravacao=${REC}`)
  await p.waitForSelector('.ed-tl', { timeout: 90000 })
  await p.waitForTimeout(1500)
  const ia = await p.locator('[data-studio="ia"]').evaluate((e) => ({ botoes: [...e.querySelectorAll('button')].map((x) => x.textContent.trim()), texto: e.textContent.replace(/\s+/g, ' ').trim() }))
  log('B3 cartões de IA (botões visíveis)', ia)
  await p.screenshot({ path: `${OUT}edicao-ia-1440x900.png` })
  // navegação: há separadores para Legendas/Exportações no topo da edição?
  log('B4 separadores no topo da Linha de tempo', await p.locator('.ed-top [data-studio-vista]').count())
  await p.goto(`${APP}/#/studio?vista=legendas`)
  await p.waitForSelector('.ed-transcript', { timeout: 30000 })
  await p.waitForTimeout(1500)
  const tr = await p.evaluate(async (id) => {
    const r = await fetch(`/api/recordings/${id}/transcript`, { headers: { Authorization: `Bearer ${localStorage.getItem('dx_access')}` } })
    const j = await r.json()
    return { servidor: { status: j.status, segmentos: j.segments?.length ?? 0 }, ecra: document.querySelector('.ed-transcript')?.textContent.replace(/\s+/g, ' ').trim().slice(0, 120), transcrever: (() => { const x = [...document.querySelectorAll('button')].find((b) => /Transcrever|Transcribe|Transcrire/.test(b.textContent)); return x ? { disabled: x.disabled } : null })() }
  }, REC)
  log('B5 legendas ignoram a transcrição do servidor', tr)
  await p.screenshot({ path: `${OUT}legendas-servidor-1440x900.png` })
  log('erros de consola (edição/legendas)', erros)
  await ctx.close()
}

// B6 — telemóvel: scroll horizontal na Linha de tempo e nas Exportações
for (const v of ['edicao', 'exportacoes']) {
  const { ctx, p } = await pagina(390, 844)
  await p.goto(`${APP}/#/studio?vista=${v}&gravacao=${REC}`)
  await p.waitForSelector(v === 'edicao' ? '.ed-tl' : '.ed-exp', { timeout: 90000 }).catch(() => {})
  await p.waitForTimeout(2000)
  log(`B6 scroll no telemóvel (${v})`, await p.evaluate(() => { const e = document.querySelector('.shell-body'); const largos = [...document.querySelectorAll('.shell-body *')].filter((x) => x.getBoundingClientRect().right > innerWidth + 1 && !x.closest('.ed-tl__area, .dx-table-wrap, .ed-tools')).slice(0, 5).map((x) => `${x.tagName.toLowerCase()}.${[...x.classList].join('.')} → ${Math.round(x.getBoundingClientRect().right)}`); return { vx: e.scrollWidth - e.clientWidth, largos } }))
  await p.screenshot({ path: `${OUT}${v}-390-overflow.png` })
  await ctx.close()
}
writeFileSync(`${OUT}bugs.json`, JSON.stringify(res, null, 2))
await b.close()
