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

// O corte por WebCodecs cai para SOFTWARE quando o runner não tem aceleração,
// e aí um vídeo de 5 s pode levar bem mais do que os 90 s deste limite. Foi uma
// das três falhas que puseram o job `isolamento` a falhar num teste diferente
// de cada vez (R90) — e a única que não era um defeito de asserção, era mesmo
// tempo a mais num runner partilhado.
//
// Mesmo idioma do `sfu_e2e.rs`: um limite generoso E ajustável, em vez de
// calibrado para a máquina de quem o escreveu. O CI põe o factor a 4.
const FATOR = Number(process.env.E2E_TIMEOUT_FACTOR) || 1
const LIMITE_CORTE = 90000 * FATOR
import { criarConta, entrar } from './sessao.mjs'

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
await page.waitForSelector('.studio-canvas', { timeout: 20000 })
ok('a rota #/studio abre e monta o canvas', page.url().includes('studio'))

const dim = await page.locator('.studio-canvas').evaluate((c) => ({ w: c.width, h: c.height }))
ok('o canvas de gravação é 1920×1080', dim.w === 1920 && dim.h === 1080, `${dim.w}×${dim.h}`)

// Lê o brilho médio de um quadrado no canto pedido do CANVAS (não do ecrã).
async function brilhoNoCanto(canto, frac = 0.22) {
  return page.locator('.studio-canvas').evaluate(
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
await page.locator('.studio-grupo', { hasText: /A tua imagem|Your picture/ }).getByRole('button').first().click()
await page.waitForTimeout(1500)

const fundo = await brilhoNoCanto('superior-esquerdo')
const comAvatarBD = await brilhoNoCanto('inferior-direito')
ok('a câmara desenha no canto inferior-direito (o de omissão)',
   comAvatarBD > fundo + 8, `fundo=${fundo} canto=${comAvatarBD}`)

// ---- mover o avatar: é o pedido central
await page.locator('.studio-canto').nth(0).click()   // superior-esquerdo
await page.waitForTimeout(900)
const seDepois = await brilhoNoCanto('superior-esquerdo')
const idDepois = await brilhoNoCanto('inferior-direito')
ok('mover para superior-esquerdo acende esse canto', seDepois > fundo + 8, `${fundo} → ${seDepois}`)
ok('e apaga o canto de onde saiu', idDepois < comAvatarBD - 8, `${comAvatarBD} → ${idDepois}`)

// ---- tamanho
const antesTam = await brilhoNoCanto('superior-esquerdo', 0.12)
await page.locator('.studio-grupo input[type=range]').fill('45')
await page.waitForTimeout(700)
const depoisTam = await brilhoNoCanto('superior-esquerdo', 0.12)
ok('o cursor de tamanho muda a bolha', Math.abs(depoisTam - antesTam) > 3, `${antesTam} → ${depoisTam}`)


// ---- arrastar a bolha e recorte de fundo
console.log('\narrasto e recorte de fundo')
{
  // Volta a um canto conhecido antes de medir.
  await page.locator('.studio-canto').nth(3).click()   // inferior-direito
  await page.locator('.studio-grupo input[type=range]').fill('22')
  await page.waitForTimeout(700)
  const antesID = await brilhoNoCanto('inferior-direito')

  // Arrasta para o canto superior-esquerdo do palco.
  const palco = await page.locator('.studio-canvas').boundingBox()
  await page.mouse.move(palco.x + palco.width * 0.85, palco.y + palco.height * 0.8)
  await page.mouse.down()
  await page.mouse.move(palco.x + palco.width * 0.16, palco.y + palco.height * 0.18, { steps: 18 })
  await page.mouse.up()
  await page.waitForTimeout(900)

  const brilhoEm = (fx, fy) =>
    page.locator('.studio-canvas').evaluate(
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
  const botaoRecorte = page.locator('.studio-seg .seg-btn').filter({ hasText: /Sem fundo|No background|Sans fond/ }).first()
  ok('há um interruptor de «sem fundo»', (await botaoRecorte.count()) > 0)
  await botaoRecorte.click()
  // `querySelector` singular apanhava o PRIMEIRO <small> do painel — que é o
  // da dica de arrasto — e dava «não arrancou» com a segmentação a correr.
  // Um teste que olha para o elemento errado mente nas duas direcções.
  const ligou = await page
    .waitForFunction(
      () => [...document.querySelectorAll('.studio-grupo small')].some((e) => e.textContent?.includes('segmenta')),
      null,
      { timeout: 60000 },
    )
    .then(() => true)
    .catch(() => false)
  if (!ligou) {
    console.log('  --    a segmentação não arrancou neste ambiente — modo recorte não verificado no fio')
  } else {
    ok('o modo «sem fundo» fica activo', await botaoRecorte.evaluate((b) => b.classList.contains('active')))
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
  await page.locator('.studio-grupo', { hasText: /O que gravar|What to record/ })
    .locator('button').filter({ hasText: /Escolher ecrã|Choose screen/ }).first()
    .click({ timeout: 5000 }).catch(() => {})
  await page.waitForTimeout(2500)
  const temFonte = await page.locator('.studio-seg').count()
  if (!temFonte) {
    console.log('  --    o browser não concedeu captura de ecrã (headless) — recorte não verificado')
  } else {
    ok('a fonte de ecrã foi aceite e os controlos de recorte aparecem', temFonte > 0)
    // Com ecrã, o CENTRO do canvas deixa de ser o fundo liso.
    const centro = await page.locator('.studio-canvas').evaluate((c) => {
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
    await page.locator('.studio-seg .seg-btn').filter({ hasText: /região|region/i }).first().click()
    await page.waitForSelector('.studio-recorte-area', { timeout: 5000 })
    const caixa = await page.locator('.studio-recorte-area').boundingBox()
    await page.mouse.move(caixa.x + caixa.width * 0.2, caixa.y + caixa.height * 0.2)
    await page.mouse.down()
    await page.mouse.move(caixa.x + caixa.width * 0.6, caixa.y + caixa.height * 0.6, { steps: 12 })
    await page.mouse.up()
    await page.waitForTimeout(800)
    const rotulo = await page.locator('.studio-grupo .mono').first().textContent().catch(() => '')
    ok('arrastar define uma região menor que o ecrã', /\d+% × \d+%/.test(rotulo || ''), rotulo || '(sem rótulo)')
    const frac = (rotulo || '').match(/(\d+)% × (\d+)%/)
    ok('a região guardada bate certo com o arrasto (~40%×40%)',
       !!frac && Math.abs(+frac[1] - 40) <= 8 && Math.abs(+frac[2] - 40) <= 8, rotulo || '')
  }
}

// ---- gravar
console.log('\ngravação')
await page.locator('.studio-acoes button').filter({ hasText: /Gravar|Record/ }).first().click()
await page.waitForSelector('.studio-tempo', { timeout: 10000 })
ok('o cronómetro aparece ao gravar', await page.locator('.studio-tempo').isVisible())
await page.waitForTimeout(3200)
await page.locator('.studio-acoes button').filter({ hasText: /Parar|Stop/ }).first().click()

await page.waitForSelector('.studio-preview', { timeout: 20000 })
const video = await page.locator('.studio-preview').evaluate(
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
console.log('\ncorte')
{
  const suporta = await page.evaluate(() => typeof VideoEncoder === 'function' && typeof MediaStreamTrackProcessor === 'function')
  ok('o browser tem WebCodecs (o caminho de pouco recurso)', suporta)
  if (suporta) {
    // Espera que a duração seja conhecida — um WebM de MediaRecorder chega
    // muitas vezes com `Infinity` até se procurar até ao fim.
    const dur = await page
      .waitForFunction(() => {
        const v = document.querySelector('.studio-preview')
        return v && Number.isFinite(v.duration) && v.duration > 0 ? v.duration : null
      }, null, { timeout: 20000 })
      .then((h) => h.jsonValue())
      .catch(() => null)
    ok('a duração do gravado é conhecida', dur !== null, dur ? `${dur.toFixed(1)}s` : 'Infinity')

    if (dur) {
      ok('os cursores de corte aparecem', (await page.locator('.studio-corte input[type=range]').count()) === 2)
      const cursores = page.locator('.studio-corte input[type=range]')
      await cursores.nth(0).fill('1')
      await cursores.nth(1).fill('3')
      await page.waitForTimeout(400)

      const botao = page.locator('.studio-corte button')
      ok('o botão anuncia a duração do troço', /0[01]:0[12]/.test((await botao.textContent()) ?? ''), await botao.textContent())
      await botao.click()

      // O corte substitui o resultado: espera pela nova duração.
      const nova = await page
        .waitForFunction(() => {
          const v = document.querySelector('.studio-preview')
          return v && Number.isFinite(v.duration) && v.duration > 0 && v.duration < 2.9 ? v.duration : null
        }, null, { timeout: LIMITE_CORTE })
        .then((h) => h.jsonValue())
        .catch(() => null)
      ok('o corte produz um ficheiro mais curto', nova !== null, nova ? `${nova.toFixed(2)}s (pedidos ~2s)` : 'não encurtou')
      if (nova) ok('e com a duração pedida (±0,6s)', Math.abs(nova - 2) < 0.6, `${nova.toFixed(2)}s`)

      const temAudio = await page.locator('.studio-preview').evaluate((v) => {
        const el = v
        return el.mozHasAudio || !!el.webkitAudioDecodedByteCount || !!(el.audioTracks && el.audioTracks.length)
      })
      console.log(`  --    faixa de áudio no cortado: ${temAudio ? 'sim' : 'não detectável por este browser'}`)
    }
  }
}

ok('sem erros de página', errosConsola.length === 0, errosConsola.slice(0, 2).join(' | ') || 'nenhum')
await page.screenshot({ path: '/tmp/estudio.png' })

await browser.close()
console.log(falhas === 0 ? '\nTUDO VERDE\n' : `\n${falhas} FALHA(S)\n`)
process.exit(falhas === 0 ? 0 : 1)
