// Capturas da revisão do Estúdio: as quatro vistas a 1920×1080, 1440×900 e
// 390×844, com câmara falsa, e a medida de TODO o scroll que existe (página e
// contentores). Escreve em notas-ui-template/revisao-estudio/<fase>/.
//
//   FASE=antes PORTA=5647 node fidelidade/revisao-capturas.mjs
import { chromium } from '@playwright/test'
import { mkdirSync, writeFileSync } from 'node:fs'
import { APP, pagina } from './revisao-sessao.mjs'
const FASE = process.env.FASE ?? 'antes'
const OUT = new URL(`../../notas-ui-template/revisao-estudio/${FASE}/`, import.meta.url).pathname
mkdirSync(OUT, { recursive: true })
const TAMANHOS = (process.env.TAMANHOS ?? '1920x1080,1440x900,1280x800,390x844').split(',').map((s) => s.split('x').map(Number))
const VISTAS = (process.env.VISTAS ?? 'emissao,edicao,legendas,exportacoes').split(',')
const gravacao = process.env.GRAVACAO ?? 'd96097f9-b617-41bc-94f2-0565dfa72ea8'

const b = await chromium.launch({ args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'] })
const relatorio = []

for (const [w, h] of TAMANHOS) {
  const { p, erros, fechar } = await pagina(b, w, h, w < 700 ? { isMobile: true, hasTouch: true } : {})
  for (const v of VISTAS) {
    let hash = v === 'emissao' ? '/studio' : `/studio?vista=${v}`
    if (v !== 'emissao' && gravacao) hash += `&gravacao=${gravacao}`
    await p.goto(`${APP}/#${hash}`)
    await p.waitForLoadState('networkidle', { timeout: 20000 }).catch(() => {})
    if (v === 'emissao') {
      await p.waitForSelector('[data-studio="canvas"]', { state: 'attached', timeout: 20000 }).catch(() => {})
      await p.locator('[data-studio-grupo="imagem"] [data-studio="camara"]').click({ timeout: 3000 }).catch(() => {})
    }
    if (v !== 'emissao') await p.waitForSelector(v === 'exportacoes' ? '.ed-exp' : v === 'legendas' ? '.ed-transcript' : '.ed-tl', { timeout: 90000 }).catch(() => relatorio.push({ w, h, vista: v, erro: 'não abriu' }))
    await p.waitForTimeout(2500)
    if (await p.locator('[data-testid=auth-email]').count()) throw new Error('sessão perdida')
    const medida = await p.evaluate(() => {
      const doc = document.scrollingElement
      const scrolls = []
      for (const el of document.querySelectorAll('*')) {
        const cs = getComputedStyle(el)
        if (!/(auto|scroll)/.test(cs.overflowY + cs.overflowX)) continue
        const vy = el.scrollHeight - el.clientHeight
        const vx = el.scrollWidth - el.clientWidth
        if (vy > 1 || vx > 1) {
          const r = el.getBoundingClientRect()
          scrolls.push({ el: `${el.tagName.toLowerCase()}.${[...el.classList].join('.')}`, vy, vx, h: Math.round(r.height) })
        }
      }
      return { pagina: { vy: doc.scrollHeight - innerHeight, vx: doc.scrollWidth - innerWidth }, scrolls }
    })
    relatorio.push({ w, h, vista: v, ...medida })
    await p.screenshot({ path: `${OUT}${v}-${w}x${h}.png` })
    console.log(w, h, v, JSON.stringify(medida))
  }
  if (erros.length) relatorio.push({ w, h, erros })
  await fechar()
}
writeFileSync(`${OUT}medidas.json`, JSON.stringify(relatorio, null, 2))
await b.close()
