#!/usr/bin/env node
// A CENA DE MARCA num Chromium a sério, medida por PIXÉIS.
//
// O que se prova aqui, e porque é que não se prova em Node: a geometria da
// moldura já tem 26 testes puros, mas um canvas não existe sem browser. Tudo o
// que é sequência de chamadas ao contexto — o recorte, o `drawImage`, o texto
// da faixa — só se observa olhando para os píxeis que saíram. É por isso que
// este ficheiro existe, e é por isso que lê CORES EM POSIÇÕES CONHECIDAS em vez
// de tirar uma fotografia para alguém comparar a olho.
//
// NÃO precisa de servidor, base de dados nem conta — e é de propósito, pelo
// mesmo argumento do `bar-responsivo.mjs`: o que aqui se mede é COMPOSIÇÃO, e
// para isso basta o compositor real empacotado na hora pelo esbuild (que vem
// com o Vite) e servido por um `node:http` de três linhas. As fontes são
// canvas de cor sólida transformados em `MediaStream` — uma cor única torna a
// asserção sobre o LUGAR inequívoca, que é o que uma textura não daria.
//
//   node e2e/cena-de-marca.mjs        (a partir de web/, senão não resolve deps)
import { chromium } from '@playwright/test'
import { execFileSync } from 'node:child_process'
import { createServer } from 'node:http'
import { readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

const AQUI = dirname(fileURLToPath(import.meta.url))
let falhas = 0
const ok = (n, c, d = '') => {
  console.log(`${c ? '  ok  ' : ' FALHA'}  ${n}${d ? `  — ${d}` : ''}`)
  if (!c) falhas++
}

const PACOTE = join(tmpdir(), `dx-cena-${process.pid}.js`)
execFileSync(
  join(AQUI, '../node_modules/.bin/esbuild'),
  [
    join(AQUI, 'cena-de-marca.entry.ts'),
    '--bundle',
    '--format=iife',
    '--define:import.meta.env.DEV=false',
    '--log-level=warning',
    `--outfile=${PACOTE}`,
  ],
  { stdio: 'inherit' },
)

const srv = createServer((q, r) => {
  const [tipo, corpo] = q.url.startsWith('/cena.js')
    ? ['text/javascript', readFileSync(PACOTE)]
    : ['text/html', '<!doctype html><meta charset="utf-8"><title>cena</title><script src="/cena.js"></script>']
  r.writeHead(200, { 'content-type': tipo })
  r.end(corpo)
}).listen(0)
const BASE = `http://127.0.0.1:${srv.address().port}/`

const navegador = await chromium.launch({ args: ['--autoplay-policy=no-user-gesture-required'] })
const pagina = await navegador.newPage()
pagina.on('console', (m) => m.type() === 'error' && console.log('    [console]', m.text()))
await pagina.goto(BASE, { waitUntil: 'load' })

/**
 * Compõe um frame com o layout pedido e devolve as cores em pontos conhecidos.
 *
 * As fontes são canvas de cor sólida — VERDE para o herói (o «ecrã
 * partilhado») e AZUL para a câmara — porque uma cor única torna a asserção
 * sobre o lugar inequívoca: se o verde aparece no sítio do herói, o herói foi
 * desenhado ali, e não há ambiguidade possível com uma textura.
 */
const amostrar = (layout) =>
  pagina.evaluate(async (layout) => {
    const { CompositorDeAula } = window
    const cor = (c) => {
      const el = document.createElement('canvas')
      el.width = 320
      el.height = 180
      const g = el.getContext('2d')
      g.fillStyle = c
      g.fillRect(0, 0, 320, 180)
      return el
    }
    const comp = new CompositorDeAula({ largura: 1920, altura: 1080 })
    comp.layout = layout
    comp.conteudo = 'fontes'
    comp.marca = { nome: 'DELONIX', titulo: '', aviso: '', deOrigem: true, logo: cor('#ffff00') }
    comp.sobreposicoes = { ...comp.sobreposicoes, url: 'PESSOAS > TECNOLOGIA' }
    comp.avatar = { ...comp.avatar, visivel: true }

    // As fontes TÊM de ser fluxos: o `temEcra`/`temCamara` do compositor exige
    // um `MediaStream` E um `videoWidth > 0`. Um `<canvas>` solto não serve —
    // foi a primeira tentativa, e o compositor desenhou o painel de «fonte sem
    // imagem» em vez do conteúdo, que é exactamente o que ele deve fazer.
    const ligar = async (video, canvas) => {
      const fluxo = canvas.captureStream(30)
      video.srcObject = fluxo
      video.muted = true
      video.playsInline = true
      await video.play().catch(() => {})
      for (let i = 0; i < 100 && !video.videoWidth; i++) await new Promise((r) => setTimeout(r, 20))
      return fluxo
    }
    comp.ecraStream = await ligar(comp.ecraVideo, cor('#00ff00'))
    comp.camaraStream = await ligar(comp.camaraVideo, cor('#0000ff'))
    if (!comp.temEcra || !comp.temCamara) return { erro: `temEcra=${comp.temEcra} temCamara=${comp.temCamara}` }

    comp.desenhar()
    const g = comp.canvas.getContext('2d')
    const em = (fx, fy) => {
      const d = g.getImageData(Math.round(fx * comp.canvas.width), Math.round(fy * comp.canvas.height), 1, 1).data
      return `${d[0]},${d[1]},${d[2]}`
    }
    // Brilho médio de uma faixa, para saber se há TINTA (texto) ou só fundo.
    const brilho = (fx, fy, fw, fh) => {
      const d = g.getImageData(
        Math.round(fx * comp.canvas.width),
        Math.round(fy * comp.canvas.height),
        Math.round(fw * comp.canvas.width),
        Math.round(fh * comp.canvas.height),
      ).data
      let s = 0
      for (let i = 0; i < d.length; i += 4) s += (d[i] + d[i + 1] + d[i + 2]) / 3
      return s / (d.length / 4)
    }
    // Quantos píxeis da cor da marca (`#e8232b`) há numa região. É esta a
    // medida da faixa e da assinatura: o brilho médio não distingue tinta da
    // marca do conteúdo que esteja por baixo, e em `solo` o topo do ecrã é
    // verde brilhante — comparar brilhos dava um controlo negativo que não
    // controlava nada (medido: 17,7 contra 7,8, a favor do caso SEM cena).
    const vermelhos = (fx, fy, fw, fh) => {
      const d = g.getImageData(
        Math.round(fx * comp.canvas.width),
        Math.round(fy * comp.canvas.height),
        Math.round(fw * comp.canvas.width),
        Math.round(fh * comp.canvas.height),
      ).data
      let n = 0
      for (let i = 0; i < d.length; i += 4) if (d[i] > 140 && d[i + 1] < 90 && d[i + 2] < 90) n++
      return n
    }
    return {
      dim: `${comp.canvas.width}×${comp.canvas.height}`,
      faixaVermelhos: vermelhos(0, 0, 1, 0.055),
      faixaBrilho: brilho(0, 0, 1, 0.055),
      logo: em(0.1, 0.14),
      heroi: em(0.65, 0.5),
      camara: em(0.1, 0.45),
      assinaturaVermelhos: vermelhos(0.02, 0.85, 0.26, 0.11),
      cantoSuperiorEsquerdo: em(0.004, 0.08),
    }
  }, layout)

console.log('\n— cena ligada (layout `moldura`) —')
const m = await amostrar('moldura')
if (m.erro) {
  ok('as fontes ligaram', false, m.erro)
} else {
  ok('o canvas é o do perfil de gravação', m.dim === '1920×1080', m.dim)
  ok('o ECRÃ está na área grande (verde no herói)', m.heroi === '0,255,0', m.heroi)
  ok('a CÂMARA está na coluna (azul à esquerda)', m.camara === '0,0,255', m.camara)
  ok('o LOGÓTIPO está no cimo da coluna (amarelo)', m.logo === '255,255,0', m.logo)
  ok('a FAIXA do topo tem a tinta da marca', m.faixaVermelhos > 200, `${m.faixaVermelhos} píxeis`)
  ok('a ASSINATURA na base da coluna tem a tinta da marca', m.assinaturaVermelhos > 50, `${m.assinaturaVermelhos} píxeis`)
  ok('a MOLDURA existe: o bordo esquerdo é preto', m.cantoSuperiorEsquerdo === '0,0,0', m.cantoSuperiorEsquerdo)
}

// CONTROLO NEGATIVO. Com a cena desligada, as MESMAS posições têm de deixar de
// ter o que a cena lá punha — senão as asserções acima estariam a medir algo
// que aparece de qualquer maneira, e não provavam a cena.
console.log('\n— controlo negativo: cena desligada (layout `solo`) —')
const s = await amostrar('solo')
ok('sem cena não há tinta da marca no topo', s.faixaVermelhos < 20, `${s.faixaVermelhos} píxeis (com cena: ${m.faixaVermelhos})`)
ok('sem cena não há tinta da marca na base da coluna', s.assinaturaVermelhos < 20, `${s.assinaturaVermelhos} píxeis`)
ok('sem cena não há logótipo no cimo da coluna', s.logo !== '255,255,0', s.logo)
ok('sem cena o bordo esquerdo NÃO é a moldura preta', s.cantoSuperiorEsquerdo !== '0,0,0', s.cantoSuperiorEsquerdo)
// CONTROLO POSITIVO do controlo negativo: o `solo` tem mesmo de ter desenhado
// alguma coisa. Um canvas em branco passaria todas as asserções acima.
ok('o controlo negativo não está a medir um canvas vazio', s.heroi === '0,255,0', s.heroi)

await navegador.close()
srv.close()
rmSync(PACOTE, { force: true })
console.log(falhas ? `\n${falhas} falha(s)` : '\nsem falhas')
process.exit(falhas ? 1 : 0)
