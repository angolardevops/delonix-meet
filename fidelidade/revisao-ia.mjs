// Prova ponta-a-ponta da IA no servidor no Estúdio: browser → servidor → Ollama
// (FALSO — nenhum modelo real instalado nesta máquina). Corre contra um
// servidor próprio com OLLAMA_URL, não contra o de validação.
//
//   PORTA=5649 DX_USER=… DX_PASS=… GRAVACAO=<id com transcrição> node fidelidade/revisao-ia.mjs
import { chromium } from '@playwright/test'
import { APP, pagina } from './revisao-sessao.mjs'
const REC = process.env.GRAVACAO
const OUT = new URL(`../../notas-ui-template/revisao-estudio/${process.env.FASE ?? 'depois'}/`, import.meta.url).pathname
const b = await chromium.launch()
const { p, erros, fechar } = await pagina(b, 1440, 900)
let falhas = 0
const ok = (n, c, d = '') => { console.log(`${c ? '  ok  ' : ' FALHA'}  ${n}${d ? ` — ${d}` : ''}`); if (!c) falhas++ }

await p.goto(`${APP}/#/studio?vista=legendas&gravacao=${REC}`)
await p.locator('[data-studio="usar-transcricao-servidor"]').click({ timeout: 90000 })
await p.waitForSelector('[data-studio="transcricao"] .ed-line')
await p.locator('.ed-top [data-studio-vista="edicao"]').click()
await p.waitForSelector('[data-studio="ia-servidor"]')
await p.waitForFunction(() => !/verificar|checking/.test(document.querySelector('[data-studio="ia-estado"]')?.textContent ?? ''), null, { timeout: 15000 })
ok('estado do modelo mostrado', true, (await p.locator('[data-studio="ia-estado"]').textContent()).trim())
await p.screenshot({ path: `${OUT}ia-cartao-1440x900.png` })

// resumo e capítulos
await p.locator('[data-studio="ia-summary"]').click()
await p.waitForSelector('[data-studio="ia-resumo"]', { timeout: 60000 })
ok('resumo chega do servidor', !!(await p.locator('[data-studio="ia-resumo"]').textContent()).trim())
await p.screenshot({ path: `${OUT}ia-resumo-1440x900.png` })
const antes = await p.locator('.ed-mark--chapter').count()
await p.locator('[data-studio="ia-criar-capitulos"]').click()
await p.waitForTimeout(500)
const depois = await p.locator('.ed-mark--chapter').count()
ok('capítulos criados na linha de tempo', depois > antes, `${antes} → ${depois}`)
await p.screenshot({ path: `${OUT}ia-capitulos-linha-1440x900.png` })

// publicação
await p.locator('[data-studio="ia-publication"]').click()
await p.waitForSelector('[data-studio="ia-titulo"]', { timeout: 60000 })
const titulo = await p.locator('[data-studio="ia-titulo"]').inputValue()
ok('título sugerido', !!titulo, titulo)
await p.locator('[data-studio="ia-usar-titulo"]').click()
await p.waitForTimeout(400)
ok('título aplicado ao projecto', (await p.locator('.ed-top__title').inputValue()) === titulo.slice(0, 80))
await p.locator('[data-studio="ia-guardar-gravacao"]').click()
await p.waitForSelector('[data-studio="ia-aviso"]', { timeout: 15000 })
const meta = await p.evaluate(async (id) => (await fetch(`/api/recordings`, { headers: { Authorization: `Bearer ${localStorage.getItem('dx_access')}` } })).json().then((l) => l.find((x) => x.id === id)), REC)
ok('descrição e etiquetas gravadas no servidor', !!meta?.description && meta.tags?.length > 0, `${meta?.description?.slice(0, 50)} · ${meta?.tags}`)
await p.screenshot({ path: `${OUT}ia-publicacao-1440x900.png` })
await p.keyboard.press('Escape')

// preenchimento
await p.locator('[data-studio="ia-fillers"]').click()
await p.waitForSelector('[data-studio="ia-rever-preenchimento"]', { timeout: 60000 })
const termos = await p.locator('[data-studio="ia-termos"] li').allTextContents()
ok('termos (só os que existem na transcrição)', termos.length > 0 && !termos.some((x) => /basically/.test(x)), termos.join(' | '))
await p.screenshot({ path: `${OUT}ia-preenchimento-1440x900.png` })
await p.locator('[data-studio="ia-rever-preenchimento"]').click()
await p.waitForSelector('[data-studio="seleccao"]')
ok('as Legendas marcam os enchimentos', (await p.locator('.ed-word--filler').count()) > 0, `${await p.locator('.ed-word--filler').count()} palavras`)
await p.screenshot({ path: `${OUT}ia-legendas-enchimentos-1440x900.png` })
ok('sem erros de página', erros.length === 0, erros.join(' | '))
await fechar()
await b.close()
process.exit(falhas ? 1 : 0)
