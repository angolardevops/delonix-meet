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

// ---------------------------------------------------------------------------
// O `programaId` MANDA NA COLUNA.
//
// O que isto prova, e porque é que o `+3` do aviso não bastava: a coluna tem
// três lugares e o Estúdio enchia-os pela ordem de inserção do `Map` dos
// convidados. O operador via no ar a ficha a dizer que três ficaram de fora e
// não tinha controlo nenhum que mudasse QUAIS três — o `programaId`, que é o
// botão «Tornar programa» que ele já tem, só era lido no layout `solo`.
//
// SEIS CONVIDADOS DE SEIS CORES. Uma cor repetida não distinguiria quem está no
// lugar de quem não aparece, que é exactamente a pergunta: as asserções são «a
// cor X está no lugar i» E «a cor Y não tem UM píxel em toda a coluna». Nenhuma
// delas sobrevive a duas fontes da mesma cor.
const CONVIDADOS = [
  { id: 'g1', nome: 'Ana Bragança', cor: '#0000ff', rgb: '0,0,255', diz: 'azul' },
  { id: 'g2', nome: 'Bento Quilemba', cor: '#ff00ff', rgb: '255,0,255', diz: 'magenta' },
  { id: 'g3', nome: 'Carla Mucanza', cor: '#00ffff', rgb: '0,255,255', diz: 'ciano' },
  { id: 'g4', nome: 'Dário Kiluanje', cor: '#ff8000', rgb: '255,128,0', diz: 'laranja' },
  { id: 'g5', nome: 'Eunice Tchissola', cor: '#8000ff', rgb: '128,0,255', diz: 'violeta' },
  { id: 'g6', nome: 'Faustino Dala', cor: '#80ff00', rgb: '128,255,0', diz: 'lima' },
]
// NENHUMA destas cores passa o crivo da tinta da marca (`r > 140 && g < 90 &&
// b < 90`), nem é o verde do ecrã nem o amarelo do logótipo. Verificado à mão,
// porque uma colisão aqui faria a contagem da assinatura e do aviso medir um
// convidado: o laranja é o único de aspecto arriscado e tem `g = 128`.

/**
 * Uma passagem: monta o palco, põe o programa pedido e lê o canvas.
 *
 * `quantos` e `layout` são argumentos porque três perguntas diferentes precisam
 * deles: o número do aviso prova-se comparando DUAS contagens de convidados, e
 * o âmbito da regra prova-se noutro layout que não a `moldura`.
 */
const amostrarColuna = (programaId, { quantos = 6, layout = 'moldura' } = {}) =>
  pagina.evaluate(
    async ({ programaId, convidados, layout }) => {
      const { CompositorDeAula, rectsDaMoldura } = window
      const cor = (c) => {
        const el = document.createElement('canvas')
        el.width = 320
        el.height = 180
        const g = el.getContext('2d')
        g.fillStyle = c
        g.fillRect(0, 0, 320, 180)
        return el
      }
      const ligar = async (video, canvas) => {
        const fluxo = canvas.captureStream(30)
        video.srcObject = fluxo
        video.muted = true
        video.playsInline = true
        await video.play().catch(() => {})
        // A MESMA condição que o `desenharConvidado` usa (`readyState >= 2 &&
        // videoWidth > 0`), e não só o `videoWidth`: esperar por menos do que o
        // desenho exige faz o arnês flocar e desenhar o painel de «fonte sem
        // imagem» num código que está certo.
        for (let i = 0; i < 150 && !(video.readyState >= 2 && video.videoWidth > 0); i++) {
          await new Promise((r) => setTimeout(r, 20))
        }
        return fluxo
      }

      const comp = new CompositorDeAula({ largura: 1920, altura: 1080 })
      comp.layout = layout
      comp.conteudo = 'fontes'
      comp.marca = { nome: 'DELONIX', titulo: '', aviso: '', deOrigem: true, logo: cor('#ffff00') }
      comp.sobreposicoes = { ...comp.sobreposicoes, url: 'PESSOAS > TECNOLOGIA' }
      // O AVATAR FICA DESLIGADO, ao contrário do `amostrar` acima. Com ele a
      // câmara local era uma peça e tomava o primeiro lugar da coluna, deixando
      // só DOIS para os convidados — e a pergunta desta secção é sobre os três.
      comp.avatar = { ...comp.avatar, visivel: false }

      comp.ecraStream = await ligar(comp.ecraVideo, cor('#00ff00'))
      if (!comp.temEcra) return { erro: 'o ecrã não ligou' }

      comp.definirConvidados(convidados.map((c) => ({ id: c.id, nome: c.nome, stream: null })))
      // Os fluxos entram DEPOIS do `definirConvidados`, pelo elemento `<video>`
      // que ele criou — é o mesmo caminho que a sala usa quando a pista de um
      // peer chega atrasada, e não há API pública para lá chegar de outra forma.
      const porId = new Map(comp.convidados)
      const prontos = []
      for (const c of convidados) {
        const no = porId.get(c.id)
        if (!no) return { erro: `o convidado ${c.id} não entrou no palco` }
        await ligar(no.video, cor(c.cor))
        prontos.push(no.video.readyState >= 2 && no.video.videoWidth > 0)
      }
      if (prontos.some((p) => !p)) return { erro: `fontes por montar: ${prontos.map((p) => (p ? 1 : 0)).join('')}` }

      comp.programaId = programaId
      comp.desenhar()

      const g = comp.canvas.getContext('2d')
      const W = comp.canvas.width
      const H = comp.canvas.height
      // A geometria REAL, com as mesmas opções que o compositor lhe passa — é
      // ela que diz onde estão os painéis, e não uma fracção adivinhada.
      //
      // O `n` é o número de CONVIDADOS, que neste palco é o mesmo que o
      // compositor calcula (`pecas.length - 1`: o ecrã é uma peça e o avatar
      // está desligado). É de propósito que não se vá ler o `pecas` dele: isto
      // é a referência FIXA de «N convidados, três lugares», e se o compositor
      // passar a desenhar outro número de painéis as amostras caem em cima do
      // fundo preto e o teste grita. Medido: a mutação que filtra em vez de
      // ordenar desenha UM painel, e a coluna saiu `amarelo|preto|preto`.
      const r = rectsDaMoldura(convidados.length, W, H, { logo: true, assinatura: true })
      // Fora da `moldura` os lugares vêm do `rectsDoLayout`, que é o que o
      // `desenharMosaico` usa. A peça 0 é o ecrã, logo os convidados começam no
      // rect 1 — é esse que diz quem o programa pôs à frente.
      const doLayout = layout === 'moldura' ? null : rectsDoLayout(layout, 1 + convidados.length, W, H)

      const corEm = (x, y) => {
        const d = g.getImageData(Math.round(x), Math.round(y), 1, 1).data
        return `${d[0]},${d[1]},${d[2]}`
      }
      // Quantos píxeis de CADA cor de convidado há numa zona. Uma passagem só:
      // seis varreduras de 500 mil píxeis em JS no browser era meio segundo por
      // frame e nenhuma informação a mais.
      const contarCores = (zona) => {
        const d = g.getImageData(
          Math.round(zona.x),
          Math.round(zona.y),
          Math.max(1, Math.round(zona.w)),
          Math.max(1, Math.round(zona.h)),
        ).data
        const n = {}
        for (const c of convidados) n[c.id] = 0
        for (let i = 0; i < d.length; i += 4) {
          for (const c of convidados) {
            if (d[i] === c.rgb[0] && d[i + 1] === c.rgb[1] && d[i + 2] === c.rgb[2]) {
              n[c.id]++
              break
            }
          }
        }
        return n
      }
      const vermelhos = (zona) => {
        const d = g.getImageData(
          Math.round(zona.x),
          Math.round(zona.y),
          Math.max(1, Math.round(zona.w)),
          Math.max(1, Math.round(zona.h)),
        ).data
        let n = 0
        for (let i = 0; i < d.length; i += 4) if (d[i] > 140 && d[i + 1] < 90 && d[i + 2] < 90) n++
        return n
      }

      // A COLUNA INTEIRA, do topo do logótipo à base da assinatura, com 2 px de
      // recuo em cada lado. O recuo é a lição medida da sessão anterior: uma
      // zona que toca na margem preta mede a MOLDURA, e devolve um número
      // redondo que se lê como resultado. Sem recuo, a borda de uma fonte
      // misturada com o preto do fundo também nunca daria a cor exacta.
      const colunaInteira = {
        x: (r.logo ?? r.camaras[0]).x + 2,
        y: (r.logo ?? r.camaras[0]).y + 2,
        w: (r.logo ?? r.camaras[0]).w - 4,
        h: (r.assinatura ? r.assinatura.y + r.assinatura.h : r.camaras[r.camaras.length - 1].y + r.camaras[r.camaras.length - 1].h) - (r.logo ?? r.camaras[0]).y - 4,
      }

      return {
        // Na `moldura`, os painéis da coluna; fora dela, os lugares do mosaico
        // a partir do segundo (o primeiro é o ecrã, que não se reordena).
        painel: doLayout
          ? doLayout.slice(1).map((p) => corEm(p.x + p.w / 2, p.y + p.h / 2))
          : r.camaras.map((p) => corEm(p.x + p.w / 2, p.y + p.h / 2)),
        heroi: corEm(
          doLayout ? doLayout[0].x + doLayout[0].w / 2 : r.heroi.x + r.heroi.w / 2,
          doLayout ? doLayout[0].y + doLayout[0].h / 2 : r.heroi.y + r.heroi.h / 2,
        ),
        naColuna: contarCores(colunaInteira),
        // O aviso de omitidas: a ficha vai no ALTO do último painel, com o
        // número na tinta da marca. Mede-se a tinta na faixa de cima desse
        // painel — não a coluna toda, onde a assinatura também é vermelha.
        avisoVermelhos: vermelhos({
          x: r.camaras[r.camaras.length - 1].x,
          y: r.camaras[r.camaras.length - 1].y,
          w: r.camaras[r.camaras.length - 1].w,
          h: Math.min(60, r.camaras[r.camaras.length - 1].h),
        }),
        // ESTES DOIS SÃO A REFERÊNCIA DO ARNÊS, não leituras do canvas: saem do
        // `rectsDaMoldura` que o script chamou com os N convidados. Dizem o que
        // a cena DEVIA mostrar, e servem para posicionar as amostras e para
        // falhar alto se a geometria mudar por baixo do teste. NÃO SERVEM de
        // prova de que o aviso está certo, e já estiveram a fazer esse papel
        // indevidamente: numa mutação que faz o canvas desenhar `+4` eles
        // continuam a dizer `+3`, porque nunca leram o canvas.
        omitidasEsperadas: r.omitidas,
        lugaresEsperados: r.camaras.length,
      }
    },
    {
      programaId,
      layout,
      convidados: CONVIDADOS.slice(0, quantos).map((c) => ({ ...c, rgb: c.rgb.split(',').map(Number) })),
    },
  )

const porCor = (rgb) => CONVIDADOS.find((c) => c.rgb === rgb)?.diz ?? rgb
const nomesDos = (painel) => painel.map(porCor).join('|')

console.log('\n— a coluna sem programa escolhido (ordem de inserção) —')
const antes = await amostrarColuna(null)
if (antes.erro) {
  ok('as seis fontes montaram', false, antes.erro)
} else {
  ok(
    'a geometria dá três lugares e três de fora (a referência do arnês)',
    antes.lugaresEsperados === 3 && antes.omitidasEsperadas === 3,
    `${antes.lugaresEsperados} lugares, +${antes.omitidasEsperadas}`,
  )
  ok('o ECRÃ é o herói, não um convidado', antes.heroi === '0,255,0', antes.heroi)
  ok(
    'a coluna é azul|magenta|ciano — os três primeiros a entrar',
    nomesDos(antes.painel) === 'azul|magenta|ciano',
    nomesDos(antes.painel),
  )
  // Os três que não cabem: ZERO píxeis em toda a coluna. É esta a asserção que
  // o aviso `+3` não fazia — ele conta quantos, não diz quais.
  for (const id of ['g4', 'g5', 'g6']) {
    const c = CONVIDADOS.find((x) => x.id === id)
    ok(`o ${c.diz} não cabe e não tem UM píxel na coluna`, antes.naColuna[id] === 0, `${antes.naColuna[id]} píxeis`)
  }
}

console.log('\n— o mesmo palco com o VIOLETA no programa —')
// O violeta é o 5.º a entrar: sem programa está fora do ar (medido acima, zero
// píxeis). Escolhê-lo tem de o trazer para o TOPO da coluna e empurrar o ciano
// — que lá estava — para fora. É a troca que prova que o `programaId` manda.
const depois = await amostrarColuna('g5')
if (depois.erro) {
  ok('as seis fontes montaram (2.ª volta)', false, depois.erro)
} else {
  ok(
    'a coluna passa a violeta|azul|magenta — o programa à cabeça',
    nomesDos(depois.painel) === 'violeta|azul|magenta',
    nomesDos(depois.painel),
  )
  ok(
    'o VIOLETA ganhou lugar: tinha zero píxeis, agora tem muitos',
    antes.naColuna.g5 === 0 && depois.naColuna.g5 > 50_000,
    `${antes.naColuna.g5} → ${depois.naColuna.g5} píxeis`,
  )
  ok(
    'o CIANO estava na coluna e o programa empurrou-o: zero píxeis',
    antes.naColuna.g3 > 50_000 && depois.naColuna.g3 === 0,
    `${antes.naColuna.g3} → ${depois.naColuna.g3} píxeis`,
  )
  // AS TRÊS AUSÊNCIAS também na 2.ª passagem, e não só a do ciano. A zona
  // `colunaInteira` cobre o logótipo e a assinatura além dos painéis, pelo que
  // uma peça desenhada fora dos três lugares só é apanhada na passagem onde se
  // procura por ela — verificar três ausências antes e uma depois deixava meia
  // coluna sem guarda.
  for (const id of ['g3', 'g4', 'g6']) {
    const c = CONVIDADOS.find((x) => x.id === id)
    ok(`e o ${c.diz} fica fora da coluna inteira`, depois.naColuna[id] === 0, `${depois.naColuna[id]} píxeis`)
  }
  // A frase é «COM ECRÃ», e a qualificação não é pedantismo: medido em
  // Chromium, numa `moldura` SEM conteúdo não há peça que segure a cabeça da
  // lista e o programa passa a ser o HERÓI (`heroi = violeta`). A invariante que
  // este arnês prova é a deste palco — com ecrã partilhado, o programa fica-se
  // pela coluna.
  ok('COM ECRÃ, o programa fica-se pela coluna e não rouba o herói', depois.heroi === '0,255,0', depois.heroi)
  // O AVISO continua a contar certo depois da reordenação. O `> 0` não é
  // decoração: sem ele, `0 === 0` dava verde exactamente no caso em que o aviso
  // deixou de ser desenhado — foi a lição da sessão anterior.
  // A FRASE DIZ SÓ O QUE A MEDIÇÃO DÁ, e isto já esteve errado aqui: dizia «o
  // aviso de +3 continua DESENHADO» e passava com o canvas a desenhar `+4`.
  // Esta asserção prova INVARIÂNCIA — a reordenação não mexeu no aviso — e não
  // o valor do número. Quem prova o valor é o par de passagens abaixo.
  ok(
    'a tinta do aviso é a MESMA antes e depois da reordenação, e não é zero',
    antes.avisoVermelhos > 0 && antes.avisoVermelhos === depois.avisoVermelhos,
    `${antes.avisoVermelhos} → ${depois.avisoVermelhos} píxeis de tinta`,
  )
}

console.log('\n— o número do aviso segue a contagem, não é um selo fixo —')
// PORQUE É UM PAR, e não uma asserção sobre um número só: um glifo não se lê
// por OCR a partir de píxeis, mas a TINTA muda com o algarismo. Com quatro
// convidados sobra UM de fora (`+1`), com seis sobram três (`+3`), e as duas
// medições têm de ser diferentes com folga — se fossem iguais, o aviso seria uma
// imagem fixa que não sabe contar. `+1` e `+3` estão a ~24 píxeis um do outro;
// `+2` e `+3` ficam a 3, e um portão com 3 píxeis de folga dá falso alarme à
// primeira troca de tipo de letra, por isso o par é 4↔6 e não 5↔6.
const quatro = await amostrarColuna(null, { quantos: 4 })
if (quatro.erro) {
  ok('as quatro fontes montaram', false, quatro.erro)
} else {
  ok('com UM de fora o aviso também é desenhado', quatro.avisoVermelhos > 0, `${quatro.avisoVermelhos} píxeis`)
  ok(
    'a tinta de `+1` difere da de `+3` com folga — o aviso conta, não é um selo',
    Math.abs(quatro.avisoVermelhos - antes.avisoVermelhos) > 10,
    `+1=${quatro.avisoVermelhos} vs +3=${antes.avisoVermelhos}`,
  )
}
// O QUE ESTE PAR APANHA POR ACIDENTE, e vale dizê-lo porque a diferença entre
// «apanha» e «apanha de propósito» é a que decide se o portão sobrevive à
// próxima troca de tipo de letra.
//
// A mutação que desloca a CONTAGEM por um — `rectsDaMoldura(pecas.length)` em
// vez de `pecas.length - 1`, umas linhas acima no `studio/compositor.ts` — faz
// o canvas desenhar `+2`/`+4` onde devia estar `+1`/`+3`. Ela DÁ EXIT 1 hoje,
// mas não por este teste saber qual é o número certo: é que `+2` e `+4` saem a
// 7 píxeis de tinta um do outro (164 e 157), abaixo da folga de 10, enquanto o
// par correcto sai a 24 (143 e 167). O portão fecha-se por as larguras dos
// glifos caírem assim, e um tipo de letra diferente pode reabri-lo.
//
// O que falta para o fechar DE PROPÓSITO é uma âncora absoluta — saber que
// aquele glifo é um `3` e não um `4` — e nem este arnês nem o da Sala
// (`cena-na-sala.mjs`, que também só compara contagens entre si) a têm. É
// anterior a esta fatia e atinge os dois compositores.

console.log('\n— o âmbito da regra: vale fora da moldura, MENOS na grelha —')
// A fatia decidiu que o `programaId` ordena os convidados em todos os layouts
// EXCEPTO a `grelha`, e uma decisão sem portão é uma intenção. Sem estas duas
// asserções passava tanto «reordenar só na moldura» como «reordenar na grelha
// também» — medi as duas mutações a dar exit 0 antes de as escrever.
const lado = { sem: await amostrarColuna(null, { layout: 'lado-a-lado' }), com: await amostrarColuna('g5', { layout: 'lado-a-lado' }) }
if (lado.sem.erro || lado.com.erro) {
  ok('o lado-a-lado montou', false, lado.sem.erro || lado.com.erro)
} else {
  ok(
    'no `lado-a-lado` o programa ganha o único lugar de convidado',
    porCor(lado.sem.painel[0]) === 'azul' && porCor(lado.com.painel[0]) === 'violeta',
    `${porCor(lado.sem.painel[0])} → ${porCor(lado.com.painel[0])}`,
  )
  ok('e o ecrã continua a ser a outra metade', lado.com.heroi === '0,255,0', lado.com.heroi)
}
const grelha = { sem: await amostrarColuna(null, { layout: 'grelha' }), com: await amostrarColuna('g5', { layout: 'grelha' }) }
if (grelha.sem.erro || grelha.com.erro) {
  ok('a grelha montou', false, grelha.sem.erro || grelha.com.erro)
} else {
  // A grelha de sete fontes está ABAIXO do tecto de nove: ninguém entra nem sai
  // do ar, logo reordenar não daria escolha nenhuma ao operador — só
  // embaralhava o mosaico inteiro a meio de uma gravação. O `> 1` no tamanho não
  // é decoração: com um mosaico de um lugar só, a igualdade seria trivial.
  ok(
    'na `grelha` o programa NÃO embaralha o mosaico ao vivo',
    grelha.sem.painel.length > 1 && grelha.sem.painel.join('|') === grelha.com.painel.join('|'),
    `${grelha.sem.painel.length} lugares: ${grelha.sem.painel.map(porCor).join('|')}`,
  )
}

console.log('\n— controlo negativo: um programa que já saiu do palco —')
// Um `programaId` fantasma não pode deixar um lugar vazio nem reordenar nada: o
// dono do `programaId` é o React, o compositor não o poda, e a ordem tem de cair
// na de sempre. Sem esta asserção, um `find` que devolvesse `undefined` e fosse
// empurrado para a lista passaria as duas acima.
const fantasma = await amostrarColuna('ninguem-com-este-id')
if (fantasma.erro) {
  ok('as seis fontes montaram (3.ª volta)', false, fantasma.erro)
} else {
  ok(
    'um programa fantasma é ignorado: a coluna volta à ordem de inserção',
    nomesDos(fantasma.painel) === 'azul|magenta|ciano',
    nomesDos(fantasma.painel),
  )
  // O «lugar vazio» é provado pela asserção ACIMA desta: um lugar vazio leria
  // `0,0,0`, não `azul|magenta|ciano`. Esta mede o aviso, e mede-o pela tinta
  // IGUAL à das outras passagens de seis — não pelo `omitidasEsperadas`, que é
  // a referência do arnês e diria `+3` faça o compositor o que fizer.
  ok(
    'e o aviso sai com a mesma tinta das outras passagens de seis',
    fantasma.avisoVermelhos > 0 && fantasma.avisoVermelhos === antes.avisoVermelhos,
    `${fantasma.avisoVermelhos} vs ${antes.avisoVermelhos} píxeis`,
  )
}

await navegador.close()
srv.close()
rmSync(PACOTE, { force: true })
console.log(falhas ? `\n${falhas} falha(s)` : '\nsem falhas')
process.exit(falhas ? 1 : 0)
