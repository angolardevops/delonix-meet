// Prova: uma gravação da biblioteca transcrita no servidor entra nas Legendas
// com um clique, e as palavras de preenchimento passam a ser encontradas.
import { chromium } from '@playwright/test'
import { APP, pagina } from './revisao-sessao.mjs'
const REC = process.env.GRAVACAO ?? 'd96097f9-b617-41bc-94f2-0565dfa72ea8'
const OUT = new URL(`../../notas-ui-template/revisao-estudio/${process.env.FASE ?? 'depois'}/`, import.meta.url).pathname
const b = await chromium.launch()
const { p, erros, fechar } = await pagina(b, 1440, 900)
await p.goto(`${APP}/#/studio?vista=edicao&gravacao=${REC}`)
await p.waitForSelector('.ed-tl', { timeout: 90000 })
await p.goto(`${APP}/#/studio?vista=legendas`)
const botao = p.locator('[data-studio="usar-transcricao-servidor"]')
await botao.waitFor({ timeout: 30000 })
await p.screenshot({ path: `${OUT}legendas-servidor-antes-de-usar-1440x900.png` })
await botao.click()
await p.waitForSelector('[data-studio="transcricao"] .ed-line', { timeout: 10000 })
const linhas = await p.locator('[data-studio="transcricao"] .ed-line').count()
console.log('linhas na transcrição', linhas)
console.log('barra de selecção', (await p.locator('[data-studio="seleccao"]').textContent())?.replace(/\s+/g, ' '))
await p.screenshot({ path: `${OUT}legendas-servidor-usada-1440x900.png` })
await p.goto(`${APP}/#/studio?vista=edicao`)
await p.waitForSelector('.ed-tl', { timeout: 30000 })
console.log('faixa CC (cues)', await p.locator('.ed-cue').count())
console.log('erros', erros)
await fechar()
await b.close()
