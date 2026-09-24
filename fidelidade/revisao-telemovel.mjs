// Prova do telemóvel (390×844): sem scroll horizontal na página nas vistas do Estúdio.
import { chromium } from '@playwright/test'
import { APP, pagina } from './revisao-sessao.mjs'
const REC = process.env.GRAVACAO ?? 'd96097f9-b617-41bc-94f2-0565dfa72ea8'
const OUT = new URL(`../../notas-ui-template/revisao-estudio/${process.env.FASE ?? 'depois'}/`, import.meta.url).pathname
const b = await chromium.launch({ args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'] })
for (const v of ['emissao', 'edicao', 'legendas', 'exportacoes']) {
  const { p, fechar } = await pagina(b, 390, 844, { isMobile: true, hasTouch: true })
  await p.goto(`${APP}/#/studio${v === 'emissao' ? '' : `?vista=${v}&gravacao=${REC}`}`)
  await p.waitForSelector(v === 'emissao' ? '[data-studio="canvas"]' : v === 'exportacoes' ? '.ed-exp' : v === 'legendas' ? '.ed-transcript' : '.ed-tl', { state: 'attached', timeout: 90000 }).catch(() => {})
  await p.waitForTimeout(2000)
  const m = await p.evaluate(() => { const e = document.querySelector('.shell-body'); return { vx: e.scrollWidth - e.clientWidth, doc: document.scrollingElement.scrollWidth - innerWidth } })
  console.log(v, JSON.stringify(m))
  await p.screenshot({ path: `${OUT}${v}-390x844.png` })
  await fechar()
}
await b.close()
