// Mede, em larguras REAIS, se as acções nucleares da sala cabem no ecrã.
//
// Existe por causa do R86: a `.controls-bar` transbordava 318px a 375px e o
// botão de DESLIGAR ficava fora do ecrã. Sair de uma reunião no telemóvel só
// era possível fechando o separador — o gesto que a recuperação lê como quebra
// de rede e tenta reverter.
//
// PORQUE UM ARNÊS ESTÁTICO e não a sala a sério: a sala precisa de servidor,
// base de dados e media. O que aqui se mede é LAYOUT, e para isso basta o CSS
// compilado a sério (`web/src/styles.scss`) com a marcação real da barra. É o
// mesmo princípio do arnês de media: carrega-se o que é real, não um duplo.
//
// PORQUE NÃO A EMULAÇÃO DO NAVEGADOR: testei-a primeiro e o `innerWidth` da
// página não acompanhava a viewport emulada — dava 693px com o ecrã a 375. Uma
// medida em que não se confia não é medida. O Playwright fixa a viewport de
// verdade.
//
//   node e2e/bar-responsivo.mjs        (a partir de web/, senão não resolve deps)
import { chromium } from '@playwright/test'
import { execFileSync } from 'node:child_process'
import { createServer } from 'node:http'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

const AQUI = dirname(fileURLToPath(import.meta.url))
// Compila a folha A SÉRIO. Um teste contra um CSS à parte provaria o CSS à
// parte — o que interessa é o que o produto serve.
execFileSync(join(AQUI, '../node_modules/.bin/sass'),
  ['--no-source-map', '--load-path=' + join(AQUI, '../src'),
   join(AQUI, '../src/styles.scss'), join(AQUI, 'room.css')], { stdio: 'inherit' })

const srv = createServer((q, r) => {
  const f = q.url.startsWith('/room.css') ? 'room.css' : 'bar-responsivo.html'
  r.writeHead(200, { 'content-type': f.endsWith('.css') ? 'text/css' : 'text/html' })
  r.end(readFileSync(join(AQUI, f)))
}).listen(0)
const PORTA = srv.address().port
const URL = `http://127.0.0.1:${PORTA}/bar-responsivo.html`
const LARGURAS = [320, 375, 414, 768, 1440]
// As acções sem as quais uma reunião não se opera.
const NUCLEARES = ['Sair da chamada', 'Desativar microfone', 'Desativar câmara']

const b = await chromium.launch()
let mau = 0
for (const w of LARGURAS) {
  const p = await b.newPage({ viewport: { width: w, height: 812 } })
  await p.goto(URL, { waitUntil: 'load' })
  const r = await p.evaluate((nucleares) => {
    const out = { vw: innerWidth, itens: [], overflow: 0 }
    const bar = document.querySelector('.controls-bar')
    out.overflow = Math.max(0, bar.scrollWidth - innerWidth)
    for (const label of nucleares) {
      const el = document.querySelector(`[aria-label="${label}"]`)
      if (!el) { out.itens.push({ label, erro: 'ausente' }); continue }
      const q = el.getBoundingClientRect()
      const cs = getComputedStyle(el)
      out.itens.push({
        label,
        visivel: q.left >= -0.5 && q.right <= innerWidth + 0.5 && q.width > 0,
        w: Math.round(q.width), h: Math.round(q.height),
      })
    }
    return out
  }, NUCLEARES)
  const falhas = r.itens.filter(i => !i.visivel)
  const pequenos = r.itens.filter(i => i.visivel && (i.w < 44 || i.h < 44))
  const ok = falhas.length === 0
  console.log(`${String(r.vw).padStart(5)}px  ${ok ? '✓' : '✗'}  ` +
    `overflow ${String(r.overflow).padStart(4)}px  ` +
    r.itens.map(i => `${i.label.split(' ')[0]}:${i.visivel ? `${i.w}×${i.h}` : 'FORA'}`).join('  ') +
    (pequenos.length && r.vw < 768 ? `  ← ${pequenos.length} abaixo de 44px` : ''))
  if (!ok) mau++
  await p.close()
}
await b.close()
srv.close()
console.log(mau ? `\n✗ ${mau} largura(s) sem as acções nucleares no ecrã` : '\n✓ acções nucleares presentes em todas as larguras')
process.exit(mau ? 1 : 0)
