#!/usr/bin/env node
// Verificação do Estúdio num Chromium a sério, com câmara e ecrã FALSOS.
//
// O que interessa provar não é que a página abre — é que o compositor DESENHA:
// que o avatar aparece no canto escolhido e muda quando se escolhe outro, que o
// recorte encolhe a imagem do ecrã, e que a gravação produz um ficheiro.
// Tudo por PIXÉIS do canvas, que é a única leitura que não se deixa enganar.
//
// Uso:  BASE=http://127.0.0.1:5174 node e2e/estudio.mjs
import { chromium } from '@playwright/test'
import { criarConta, entrar } from './sessao.mjs'
import { texto } from './estudio-textos.mjs'

// Esperas por media (segmentação, gravação a entrar no editor) escalam com o
// factor do runner: o CI põe-no a 4 (R118).
const FATOR = Number(process.env.E2E_TIMEOUT_FACTOR) || 1
const BASE = process.env.BASE ?? process.env.APP ?? 'http://127.0.0.1:5174'
const API = process.env.API ?? BASE
const conta = await criarConta(API, 'est')
let falhas = 0
const ok = (nome, cond, det = '') => {
  console.log(`${cond ? '  ok  ' : ' FALHA'}  ${nome}${det ? `  — ${det}` : ''}`)
  if (!cond) falhas++
}

const browser = await chromium.launch({
  args: [
    '--use-fake-device-for-media-stream',
    '--use-fake-ui-for-media-stream',
    '--auto-select-desktop-capture-source=Entire screen',
    '--allow-http-screen-capture',
  ],
})
const ctx = await browser.newContext({
  viewport: { width: 1440, height: 900 },
  permissions: ['camera', 'microphone'],
})
const page = await ctx.newPage()
const errosConsola = []
page.on('pageerror', (e) => errosConsola.push(e.message.slice(0, 140)))
const logsSegmentacao = []
page.on('console', (m) => {
  const t = m.text()
  if (/\[background\]|\[matte\]/.test(t)) logsSegmentacao.push(t.slice(0, 160))
})

// ---- entrar (conta nova, sessão real — ver e2e/sessao.mjs)
await entrar(page, BASE, conta)

// ---- a entrada existe na navegação
console.log('\nnavegação')
const entrada = page.locator('.nav-item', { hasText: /Estúdio|Studio/ })
ok('a entrada «Estúdio» está no rail', (await entrada.count()) > 0)
await entrada.first().click()
await page.waitForSelector('[data-studio="canvas"]', { timeout: 20000 })
ok('a rota #/studio abre e monta o canvas', page.url().includes('studio'))

const dim = await page.locator('[data-studio="canvas"]').evaluate((c) => ({ w: c.width, h: c.height }))
ok('o canvas de gravação é 1920×1080', dim.w === 1920 && dim.h === 1080, `${dim.w}×${dim.h}`)

// Lê o brilho médio de um quadrado no canto pedido do CANVAS (não do ecrã).
async function brilhoNoCanto(canto, frac = 0.22) {
  return page.locator('[data-studio="canvas"]').evaluate(
    (c, { canto, frac }) => {
      const g = c.getContext('2d')
      const s = Math.round(Math.min(c.width, c.height) * frac)
      const x = canto.includes('direito') ? c.width - s : 0
      const y = canto.includes('inferior') ? c.height - s : 0
      const d = g.getImageData(x, y, s, s).data
      let soma = 0
      for (let i = 0; i < d.length; i += 4) soma += (d[i] + d[i + 1] + d[i + 2]) / 3
      return Math.round(soma / (d.length / 4))
    },
    { canto, frac },
  )
}

// ---- câmara
console.log('\navatar')
const grupoImagem = page.locator('[data-studio-grupo="imagem"]')
ok('o grupo «a tua imagem» tem o título traduzido', texto('imagem.titulo').test((await grupoImagem.textContent()) ?? ''))
await grupoImagem.locator('[data-studio="camara"]').click()
await page.waitForTimeout(1500)

const fundo = await brilhoNoCanto('superior-esquerdo')
const comAvatarBD = await brilhoNoCanto('inferior-direito')
ok('a câmara desenha no canto inferior-direito (o de omissão)',
   comAvatarBD > fundo + 8, `fundo=${fundo} canto=${comAvatarBD}`)

// ---- mover o avatar: é o pedido central
await page.locator('[data-studio-canto="superior-esquerdo"]').click()
await page.waitForTimeout(900)
const seDepois = await brilhoNoCanto('superior-esquerdo')
const idDepois = await brilhoNoCanto('inferior-direito')
ok('mover para superior-esquerdo acende esse canto', seDepois > fundo + 8, `${fundo} → ${seDepois}`)
ok('e apaga o canto de onde saiu', idDepois < comAvatarBD - 8, `${comAvatarBD} → ${idDepois}`)

// ---- tamanho
const antesTam = await brilhoNoCanto('superior-esquerdo', 0.12)
await page.locator('[data-studio="tamanho"]').fill('45')
await page.waitForTimeout(700)
const depoisTam = await brilhoNoCanto('superior-esquerdo', 0.12)
ok('o cursor de tamanho muda a bolha', Math.abs(depoisTam - antesTam) > 3, `${antesTam} → ${depoisTam}`)


// ---- arrastar a bolha e recorte de fundo
console.log('\narrasto e recorte de fundo')
{
  // Volta a um canto conhecido antes de medir.
  await page.locator('[data-studio-canto="inferior-direito"]').click()
  await page.locator('[data-studio="tamanho"]').fill('22')
  await page.waitForTimeout(700)
  const antesID = await brilhoNoCanto('inferior-direito')

  // Arrasta para o canto superior-esquerdo do palco.
  const palco = await page.locator('[data-studio="canvas"]').boundingBox()
  await page.mouse.move(palco.x + palco.width * 0.85, palco.y + palco.height * 0.8)
  await page.mouse.down()
  await page.mouse.move(palco.x + palco.width * 0.16, palco.y + palco.height * 0.18, { steps: 18 })
  await page.mouse.up()
  await page.waitForTimeout(900)

  const brilhoEm = (fx, fy) =>
    page.locator('[data-studio="canvas"]').evaluate(
      (c, { fx, fy }) => {
        const g = c.getContext('2d')
        const s = Math.round(Math.min(c.width, c.height) * 0.14)
        const x = Math.min(c.width - s, Math.max(0, Math.round(fx * c.width - s / 2)))
        const y = Math.min(c.height - s, Math.max(0, Math.round(fy * c.height - s / 2)))
        const d = g.getImageData(x, y, s, s).data
        let soma = 0
        for (let i = 0; i < d.length; i += 4) soma += (d[i] + d[i + 1] + d[i + 2]) / 3
        return Math.round(soma / (d.length / 4))
      },
      { fx, fy },
    )
  const noPontoLargado = await brilhoEm(0.16, 0.18)
  const depoisID = await brilhoNoCanto('inferior-direito')
  ok('arrastar leva a bolha para onde se largou', noPontoLargado > fundo + 8, `fundo=${fundo} ponto=${noPontoLargado}`)
  ok('e tira-a de onde estava', depoisID < antesID - 8, `${antesID} → ${depoisID}`)

  // Recorte de fundo: liga a segmentação e espera pelo primeiro resultado.
  const botaoRecorte = page.locator('[data-studio-modo="recorte"]')
  ok('há um interruptor de «sem fundo»', (await botaoRecorte.count()) > 0 && texto('imagem.semFundo').test((await botaoRecorte.textContent()) ?? ''))
  await botaoRecorte.click()
  // A nota da segmentação tem atributo próprio. A versão antiga procurava o
  // primeiro <small> do painel — que era a dica de arrasto — e dava «não
  // arrancou» com a segmentação a correr. Um teste que olha para o elemento
  // errado mente nas duas direcções.
  const ligou = await page
    .waitForSelector('[data-studio="nota-recorte"]', { timeout: 60000 * FATOR })
    .then(() => true)
    .catch(() => false)
  if (!ligou) {
    console.log('  --    a segmentação não arrancou neste ambiente — modo recorte não verificado no fio')
  } else {
    ok('o modo «sem fundo» fica activo', (await botaoRecorte.getAttribute('aria-pressed')) === 'true')
    await page.waitForTimeout(3000)
    ok('o segmentador arrancou mesmo (não só o rótulo)',
       logsSegmentacao.some((l) => /segmenta(ção|tion) em (GPU|CPU)/i.test(l)),
       logsSegmentacao.find((l) => /segmenta/i.test(l)) ?? 'sem sinal do pipeline')
    ok('o RVM em falta cai no MediaPipe em vez de rebentar',
       !logsSegmentacao.some((l) => /RVM/.test(l)) ||
         logsSegmentacao.some((l) => /RVM indispon/.test(l)),
       logsSegmentacao.find((l) => /RVM/.test(l)) ?? 'RVM disponível')

    // O QUE AQUI NÃO SE PODE PROVAR, e porquê: a câmara do Chromium de teste é
    // um padrão de cores, não uma pessoa. O segmentador corre e não encontra
    // ninguém, por isso a máscara sai vazia e não há silhueta para medir. Uma
    // asserção de brilho passava com o ecrã de fundo e não provava nada — foi
    // exactamente o que a primeira versão deste teste fazia (limiar > 5 contra
    // um fundo de 18). O recorte visível fica para um browser com uma câmara
    // a apontar a uma pessoa.
    console.log('  --    a silhueta em si precisa de uma câmara real — ver a nota no ficheiro')
  }
}

// ---- ecrã e recorte
console.log('\necrã e recorte')
const podeEcra = await page.evaluate(() => typeof navigator.mediaDevices?.getDisplayMedia === 'function')
if (!podeEcra) {
  console.log('  --    getDisplayMedia indisponível neste browser — secção saltada')
} else {
  const grupoFonte = page.locator('[data-studio-grupo="fonte"]')
  ok('o grupo «o que gravar» tem o título traduzido', texto('fonte.titulo').test((await grupoFonte.textContent()) ?? ''))
  const escolher = grupoFonte.locator('[data-studio="escolher-ecra"]')
  ok('o botão de escolher ecrã diz o que faz', texto('fonte.escolherEcra').test((await escolher.textContent()) ?? ''))
  await escolher.click({ timeout: 5000 }).catch(() => {})
  await page.waitForTimeout(2500)
  const temFonte = await page.locator('[data-studio-regiao]').count()
  if (!temFonte) {
    console.log('  --    o browser não concedeu captura de ecrã (headless) — recorte não verificado')
  } else {
    ok('a fonte de ecrã foi aceite e os controlos de recorte aparecem', temFonte > 0)
    // Com ecrã, o CENTRO do canvas deixa de ser o fundo liso.
    const centro = await page.locator('[data-studio="canvas"]').evaluate((c) => {
      const g = c.getContext('2d')
      const s = 200
      const d = g.getImageData((c.width - s) / 2, (c.height - s) / 2, s, s).data
      let soma = 0, variacao = 0, ant = -1
      for (let i = 0; i < d.length; i += 4) {
        const v = (d[i] + d[i + 1] + d[i + 2]) / 3
        soma += v
        if (ant >= 0) variacao += Math.abs(v - ant)
        ant = v
      }
      return { medio: Math.round(soma / (d.length / 4)), variacao: Math.round(variacao) }
    })
    ok('o ecrã capturado desenha no canvas', centro.medio > 20 || centro.variacao > 500,
       `médio=${centro.medio} variação=${centro.variacao}`)

    // Recorte: escolhe uma região arrastando sobre a pré-visualização.
    await page.locator('[data-studio-regiao="regiao"]').click()
    await page.waitForSelector('[data-studio="recorte-area"]', { timeout: 5000 })
    const caixa = await page.locator('[data-studio="recorte-area"]').boundingBox()
    await page.mouse.move(caixa.x + caixa.width * 0.2, caixa.y + caixa.height * 0.2)
    await page.mouse.down()
    await page.mouse.move(caixa.x + caixa.width * 0.6, caixa.y + caixa.height * 0.6, { steps: 12 })
    await page.mouse.up()
    await page.waitForTimeout(800)
    const rotulo = await page.locator('[data-studio="regiao-rotulo"]').textContent({ timeout: 3000 }).catch(() => '')
    ok('arrastar define uma região menor que o ecrã', /\d+% × \d+%/.test(rotulo || ''), rotulo || '(sem rótulo)')
    const frac = (rotulo || '').match(/(\d+)% × (\d+)%/)
    ok('a região guardada bate certo com o arrasto (~40%×40%)',
       !!frac && Math.abs(+frac[1] - 40) <= 8 && Math.abs(+frac[2] - 40) <= 8, rotulo || '')
  }
}

// ---- gravar
console.log('\ngravação')
const botaoGravar = page.locator('[data-studio="acoes"] [data-studio="gravar"]')
ok('o botão de gravar diz «gravar»', texto('acoes.gravar').test((await botaoGravar.textContent()) ?? ''))
await botaoGravar.click()
await page.waitForSelector('[data-studio="tempo"]', { timeout: 10000 })
ok('o cronómetro aparece ao gravar', await page.locator('[data-studio="tempo"]').isVisible())
await page.waitForTimeout(3200)
const botaoParar = page.locator('[data-studio="acoes"] [data-studio="parar"]')
ok('o botão de parar diz «parar»', texto('acoes.parar').test((await botaoParar.textContent()) ?? ''))
await botaoParar.click()

await page.waitForSelector('[data-studio="preview"]', { timeout: 20000 * FATOR })
const video = await page.locator('[data-studio="preview"]').evaluate(
  (v) => new Promise((r) => {
    const acabar = () => r({ dur: v.duration, w: v.videoWidth, h: v.videoHeight, src: v.src.slice(0, 5) })
    if (v.readyState >= 1) acabar()
    else v.onloadedmetadata = acabar
    setTimeout(() => r({ dur: v.duration, w: v.videoWidth, h: v.videoHeight, src: v.src.slice(0, 5) }), 6000)
  }),
)
ok('a gravação produz um ficheiro reproduzível', video.src === 'blob:', `src=${video.src}`)
ok('com imagem 1920×1080', video.w === 1920 && video.h === 1080, `${video.w}×${video.h}`)

// ---- corte
// O editor é NÃO DESTRUTIVO: aparar é uma edição no projecto e a linha de
// tempo encurta logo; o ficheiro mais curto só nasce na exportação (WebCodecs),
// e essa medida — com a duração do ficheiro produzido — é do `e2e/editor.mjs`.
// O passo antigo (dois cursores e um botão «Cortar» que reencodificava) deixou
// de existir; o que aqui se prova é que a gravação chega ao editor e que
// aparar pela entrada/saída encurta o projecto e se desfaz.
console.log('\ncorte (projecto não destrutivo)')
{
  const duracaoMostrada = async () => {
    const t = (await page.locator('[data-studio="duracao"]').textContent()) ?? ''
    const m = t.match(/(\d+):(\d{2})/)
    return m ? Number(m[1]) * 60 + Number(m[2]) : NaN
  }
  ok('parar abre o editor em #/studio?vista=edicao', page.url().includes('vista=edicao'), page.url())
  const temClipe = await page
    .waitForSelector('[data-faixa="V1"] .ed-clip', { timeout: 30000 * FATOR })
    .then(() => true)
    .catch(() => false)
  ok('a gravação entra na linha de tempo como clipe de V1', temClipe)
  const dur0 = await duracaoMostrada()
  ok('a linha de tempo tem a duração gravada', dur0 >= 2 && dur0 <= 6, `${dur0}s`)

  if (temClipe) {
    await page.locator('[data-faixa="V1"] .ed-clip').first().click({ position: { x: 20, y: 10 } })
    await page.waitForSelector('[data-studio="corte"]', { timeout: 5000 })
    await page.locator('[data-studio="corte-de"]').fill('00:00:01')
    await page.locator('[data-studio="corte-de"]').press('Enter')
    await page.locator('[data-studio="corte-ate"]').fill('00:00:03')
    await page.locator('[data-studio="corte-ate"]').press('Enter')
    await page.waitForTimeout(300)
    const dur1 = await duracaoMostrada()
    ok('aparar para 1–3 s deixa o projecto com 2 s', dur1 === 2, `${dur1}s`)
    await page.locator('[data-studio="desfazer"]').click()
    await page.locator('[data-studio="desfazer"]').click()
    await page.waitForTimeout(200)
    ok('desfazer volta à duração gravada (a fonte não foi tocada)', (await duracaoMostrada()) === dur0, `${await duracaoMostrada()}s`)
  }
  const suporta = await page.evaluate(() => typeof VideoEncoder === 'function')
  console.log(`  --    WebCodecs ${suporta ? 'disponível' : 'indisponível'}: a exportação e a duração do ficheiro produzido são do e2e/editor.mjs`)
}

ok('sem erros de página', errosConsola.length === 0, errosConsola.slice(0, 2).join(' | ') || 'nenhum')
await page.screenshot({ path: '/tmp/estudio.png' })

await browser.close()
console.log(falhas === 0 ? '\nTUDO VERDE\n' : `\n${falhas} FALHA(S)\n`)
process.exit(falhas === 0 ? 0 : 1)
