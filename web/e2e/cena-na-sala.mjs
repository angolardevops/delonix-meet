#!/usr/bin/env node
// A CENA DE MARCA NA SALA, medida por PIXÉIS do canvas que vai para RTMP.
//
// O que se prova, e é o que distingue isto do `cena-de-marca.mjs` (esse mede o
// Estúdio): na SALA, o herói da moldura é a PARTILHA DE ECRÃ, e as câmaras dos
// participantes vão para a coluna. É o que torna a cena útil numa aula — o que
// importa são os slides, não os retratos.
//
// E prova-se o CONTROLO POSITIVO que o contrato exige: sem partilha de ecrã, o
// herói não fica preto — cai para a primeira câmara. Uma emissão com o maior
// rectângulo do ecrã em negro é pior do que uma com a cara de alguém, e quem
// pára de partilhar a meio de uma aula não pode derrubar a imagem.
//
// Sem servidor nem conta, pelo mesmo argumento do `bar-responsivo.mjs`: o
// compositor real vai empacotado pelo esbuild, e as fontes são canvas de cor
// sólida virados em `MediaStream` — uma cor única torna a asserção sobre o
// LUGAR inequívoca.
//
//   node e2e/cena-na-sala.mjs
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

const PACOTE = join(tmpdir(), `dx-cena-sala-${process.pid}.js`)
execFileSync(
  join(AQUI, '../node_modules/.bin/esbuild'),
  [
    join(AQUI, 'cena-na-sala.entry.ts'),
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
    : ['text/html', '<!doctype html><meta charset="utf-8"><title>sala</title><script src="/cena.js"></script>']
  r.writeHead(200, { 'content-type': tipo })
  r.end(corpo)
}).listen(0)
const BASE = `http://127.0.0.1:${srv.address().port}/`

const navegador = await chromium.launch({ args: ['--autoplay-policy=no-user-gesture-required'] })
const pagina = await navegador.newPage()
pagina.on('console', (m) => m.type() === 'error' && console.log('    [console]', m.text()))
await pagina.goto(BASE, { waitUntil: 'load' })

/**
 * Monta o compositor da sala com as fontes pedidas e amostra o canvas.
 *
 * `comEcra` controla se existe a fonte `ecra` — é o eixo do controlo positivo.
 */
const amostrar = (cena, comEcra) =>
  pagina.evaluate(async ({ cena, comEcra }) => {
    const { RoomCompositor } = window
    const fluxoDeCor = (c) => {
      const el = document.createElement('canvas')
      el.width = 320
      el.height = 180
      const g = el.getContext('2d')
      g.fillStyle = c
      g.fillRect(0, 0, 320, 180)
      return el.captureStream(30)
    }
    const comp = new RoomCompositor(1920, 1080)
    comp.cena = cena
    comp.marca = { logo: null, faixa: 'DELONIX MEET', assinatura: 'DELONIX MEET', cor: '#e8232b' }
    const fontes = [
      ...(comEcra ? [{ id: 'ecra', nome: 'Partilha de ecrã', stream: fluxoDeCor('#00ff00') }] : []),
      { id: 'eu', nome: 'Walter', stream: fluxoDeCor('#0000ff') },
      { id: 'p2', nome: 'Convidada', stream: fluxoDeCor('#ff00ff') },
    ]
    comp.definirParticipantes(fontes)
    comp.heroiId = comEcra ? 'ecra' : null

    // Esperar que os `<video>` internos tenham imagem: sem isto o compositor
    // desenha o painel de «fonte sem imagem», que é o que ele deve fazer.
    for (let i = 0; i < 150; i++) {
      const prontos = [...comp.videos.values()].every((v) => v.videoWidth > 0)
      if (prontos) break
      await new Promise((r) => setTimeout(r, 20))
    }
    comp.desenhar()

    const g = comp.canvas.getContext('2d')
    const em = (fx, fy) => {
      const d = g.getImageData(Math.round(fx * 1920), Math.round(fy * 1080), 1, 1).data
      return `${d[0]},${d[1]},${d[2]}`
    }
    const vermelhos = (fx, fy, fw, fh) => {
      const d = g.getImageData(
        Math.round(fx * 1920),
        Math.round(fy * 1080),
        Math.round(fw * 1920),
        Math.round(fh * 1080),
      ).data
      let n = 0
      for (let i = 0; i < d.length; i += 4) if (d[i] > 140 && d[i + 1] < 90 && d[i + 2] < 90) n++
      return n
    }
    return {
      heroi: em(0.65, 0.5),
      camaraTopo: em(0.1, 0.4),
      faixaVermelhos: vermelhos(0, 0, 1, 0.055),
      bordo: em(0.004, 0.5),
    }
  }, { cena, comEcra })

console.log('\n— sala, cena `moldura`, COM partilha de ecrã —')
const comEcra = await amostrar('moldura', true)
ok('o ECRÃ partilhado está na área grande (verde no herói)', comEcra.heroi === '0,255,0', comEcra.heroi)
ok('uma CÂMARA está na coluna, não no herói', comEcra.camaraTopo === '0,0,255', comEcra.camaraTopo)
ok('a FAIXA tem a tinta da marca', comEcra.faixaVermelhos > 200, `${comEcra.faixaVermelhos} píxeis`)
ok('a MOLDURA existe: o bordo esquerdo é preto', comEcra.bordo === '0,0,0', comEcra.bordo)

// CONTROLO POSITIVO do contrato: sem ecrã, o herói NÃO é um rectângulo preto.
console.log('\n— controlo positivo: SEM partilha de ecrã —')
const semEcra = await amostrar('moldura', false)
ok('sem ecrã o herói cai para a CÂMARA, não fica preto', semEcra.heroi === '0,0,255', semEcra.heroi)
ok('sem ecrã o herói não é o fundo', semEcra.heroi !== '0,0,0', semEcra.heroi)
ok('sem ecrã a faixa continua a existir', semEcra.faixaVermelhos > 200, `${semEcra.faixaVermelhos} píxeis`)

// CONTROLO NEGATIVO: outra cena, as mesmas posições, sem o que a moldura põe.
console.log('\n— controlo negativo: cena `grelha` —')
const grelha = await amostrar('grelha', true)
ok('sem moldura não há tinta da marca no topo', grelha.faixaVermelhos < 20, `${grelha.faixaVermelhos} píxeis`)
ok('sem moldura o bordo esquerdo não é a margem preta', grelha.bordo !== '0,0,0', grelha.bordo)

await navegador.close()
srv.close()
rmSync(PACOTE, { force: true })
console.log(falhas ? `\n${falhas} falha(s)` : '\nsem falhas')
process.exit(falhas ? 1 : 0)
