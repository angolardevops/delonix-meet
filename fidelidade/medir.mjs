// Diagnóstico: alturas dos contentores do Estúdio. Uso: node fidelidade/medir.mjs <hash> <w> <h> [png]
import { chromium } from '@playwright/test'
const [hash = '/studio', w = '1440', h = '900', png] = process.argv.slice(2)
const b = await chromium.launch({ args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'] })
const ctx = await b.newContext({ viewport: { width: +w, height: +h }, permissions: ['camera', 'microphone'], storageState: new URL('./.sessao-revisao.json', import.meta.url).pathname })
const p = await ctx.newPage()
await p.goto(`http://127.0.0.1:${process.env.PORTA ?? 5647}/#${hash}`)
if (process.env.ESPERAR) await p.waitForSelector(process.env.ESPERAR, { timeout: 60000 }).catch(() => console.log('sem', process.env.ESPERAR))
await p.waitForTimeout(+(process.env.ESPERA ?? 5000))
const sel = (process.env.SEL ?? 'html,body,#root,.shell,.shell-main,.shell-body,.st,.st-top,.st-notices,.st-body,.st-col--left,.st-centre,.st-stage-fit,.st-under,.st-col--right,.ed,.ed-top,.ed-body,.ed-tl,.ed-tl__body').split(',')
console.log(JSON.stringify(await p.evaluate((sel) => sel.map((s) => { const e = [...document.querySelectorAll(s)].find((x) => x.offsetParent !== null || s === 'html' || s === 'body'); if (!e) return [s, null]; const r = e.getBoundingClientRect(); return [s, Math.round(r.top), Math.round(r.height), e.scrollHeight, getComputedStyle(e).overflowY] }), sel), null, 0))
if (png) await p.screenshot({ path: png })
await b.close()
