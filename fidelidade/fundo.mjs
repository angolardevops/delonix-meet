// Diagnóstico: o que faz o DOCUMENTO crescer para lá do viewport.
import { chromium } from '@playwright/test'
const [hash = '/studio', w = '1440', h = '900'] = process.argv.slice(2)
const b = await chromium.launch({ args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'] })
const ctx = await b.newContext({ viewport: { width: +w, height: +h }, permissions: ['camera', 'microphone'], storageState: new URL('./.sessao-revisao.json', import.meta.url).pathname })
const p = await ctx.newPage()
await p.goto(`http://127.0.0.1:${process.env.PORTA ?? 5647}/#${hash}`)
await p.waitForSelector('[data-studio="canvas"]', { state: 'attached', timeout: 20000 }).catch(() => {})
if (process.env.CAMARA) await p.locator('[data-studio-grupo="imagem"] [data-studio="camara"]').click({ timeout: 3000 }).catch((e) => console.log('sem camara', e.message.slice(0, 80)))
await p.waitForTimeout(3000)
console.log(await p.evaluate(() => {
  const out = [`doc ${document.scrollingElement.scrollHeight} vs ${innerHeight}`]
  for (const e of document.querySelectorAll('body *')) {
    const r = e.getBoundingClientRect()
    if (r.bottom > innerHeight + 1 && r.height > 0) {
      const cs = getComputedStyle(e)
      if (cs.position === 'fixed' || cs.position === 'absolute' || e.parentElement === document.body)
        out.push(`${e.tagName}.${e.className} pos=${cs.position} top=${Math.round(r.top)} h=${Math.round(r.height)}`)
    }
  }
  return out.slice(0, 30).join('\n')
}))
await b.close()
