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
// Prova-se também o TECTO DA COLUNA, que é o que esta cena tem e as outras não:
// acima de três câmaras alguém fica de fora, a emissão diz QUANTAS no canto do
// último painel, e é o anfitrião — pelas fichas de foco — que decide quem são as
// três. Antes, os que sobravam desapareciam em silêncio com o áudio na mistura.
//
// Sem servidor nem conta, pelo mesmo argumento do `bar-responsivo.mjs`: o
// compositor real vai empacotado pelo esbuild, e as fontes são canvas de cor
// sólida virados em `MediaStream`.
//
// CADA FONTE TEM A SUA COR, e não é decoração: para provar que uma câmara NÃO
// aparece é preciso procurar a cor dela em toda a coluna e não encontrar
// nenhuma. Com duas fontes da mesma cor, trocar quem está no lugar passava sem
// um vermelho.
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
 * As câmaras disponíveis, CADA UMA COM A SUA COR — é o que permite afirmar onde
 * cada uma está, e onde NÃO está.
 *
 * A TERCEIRA tem nome LONGO de propósito. É ela que ocupa o último painel da
 * coluna, que é onde o aviso de quem ficou de fora vive, e o compositor escreve
 * o nome ao longo da base desse painel — um nome de 40 caracteres atravessa-o
 * de lado a lado. A primeira versão do aviso ficava no canto inferior direito e
 * tapava a cauda do nome a meio de uma letra; com nomes curtos como os que eu
 * tinha aqui, o teste nunca tocava no problema.
 */
const CAMARAS = [
  { id: 'eu', nome: 'Ana', cor: '#0000ff', rgb: '0,0,255' },
  { id: 'p2', nome: 'Bia', cor: '#ff00ff', rgb: '255,0,255' },
  { id: 'p3', nome: 'Caio Henrique de Albuquerque Vasconcelos', cor: '#ffff00', rgb: '255,255,0' },
  { id: 'p4', nome: 'Dina', cor: '#00ffff', rgb: '0,255,255' },
  { id: 'p5', nome: 'Edu', cor: '#ff8000', rgb: '255,128,0' },
  { id: 'p6', nome: 'Fátima', cor: '#8000ff', rgb: '128,0,255' },
]
// Nenhuma cor de câmara pode passar pelo crivo da tinta da marca
// (`r > 140 && g < 90 && b < 90`), senão a contagem do aviso media uma câmara.
for (const c of CAMARAS) {
  const [r, g, b] = c.rgb.split(',').map(Number)
  if (r > 140 && g < 90 && b < 90) throw new Error(`a cor ${c.cor} de ${c.id} confunde-se com a tinta da marca`)
}

// A COLUNA e a zona do AVISO, em fracções do canvas 1920×1080, derivadas à mão
// das fracções de `cena/moldura.ts` com `logo: false` (a Sala não tem logótipo)
// e `assinatura: true` — e à mão DE PROPÓSITO: lê-las do módulo que está a ser
// medido provaria consistência, não correcção.
//
//   faixa 62,6 · margem 30,2 · conteúdo y ∈ [92,9; 1049,8] · coluna x ∈ [30,2; 542,4]
//   assinatura a começar em y = 906,2 — por isso a coluna varrida acaba ANTES
//   três câmaras: painéis de 271,1 de altura, o último em y ∈ [635,1; 906,2]
const COLUNA = { fx: 0.02, fy: 0.09, fw: 0.26, fh: 0.74 }
// O canto SUPERIOR direito do ÚLTIMO painel, onde o aviso vive: a ficha fica em
// y ∈ [645; 687] com uma folga de 10 do topo do painel (635,1). Começa em
// fx 0,15 para deixar de fora o nome, que é branco e começa à esquerda.
//
// O `fw` acaba ANTES do fim da coluna (x = 542,4), e a primeira versão não: com
// `0,133` a zona apanhava uma coluna de 1 píxel da margem preta da moldura, e o
// controlo negativo contava 254 píxeis pretos — exactamente a altura da zona —
// com aviso nenhum desenhado. Uma zona de amostragem que toca na moldura mede a
// moldura.
const AVISO = { fx: 0.15, fy: 0.589, fw: 0.131, fh: 0.055 }
// A BASE do último painel, onde o compositor escreve o nome: o painel acaba em
// y = 906,2 e o nome assenta em y = 896,2. Toda a largura da coluna, porque é
// disso que se trata — um nome de 40 caracteres atravessa-a.
const NOME = { fx: 0.02, fy: 0.805, fw: 0.26, fh: 0.028 }

/**
 * Monta o compositor da sala com as fontes pedidas e amostra o canvas.
 *
 * `comEcra` controla se existe a fonte `ecra` — é o eixo do controlo positivo.
 * `nCamaras` é quantas das `CAMARAS` entram, e `focoIds` o que o anfitrião
 * escolheu nas fichas do painel.
 *
 * `nSombras` acrescenta câmaras cinzentas e anónimas, só para fazer a conta
 * subir: servem o caso dos DOIS DÍGITOS, onde o que se mede é a largura do
 * aviso e não quem está em que lugar. Não têm cor própria de propósito — dar
 * sete cores novas a fontes que ninguém procura confundia a leitura do ficheiro.
 */
const medir = (cena, comEcra, nCamaras = 2, focoIds = [], nSombras = 0) =>
  pagina.evaluate(async ({ cena, comEcra, nCamaras, focoIds, nSombras, CAMARAS, COLUNA, AVISO, NOME }) => {
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
    const usadas = CAMARAS.slice(0, nCamaras)
    const fontes = [
      ...(comEcra ? [{ id: 'ecra', nome: 'Partilha de ecrã', stream: fluxoDeCor('#00ff00') }] : []),
      ...usadas.map((c) => ({ id: c.id, nome: c.nome, stream: fluxoDeCor(c.cor) })),
      ...Array.from({ length: nSombras }, (_, i) => ({
        id: `s${i}`,
        nome: '',
        stream: fluxoDeCor('#404040'),
      })),
    ]
    comp.definirParticipantes(fontes)
    comp.heroiId = comEcra ? 'ecra' : null
    comp.focoIds = focoIds

    // Esperar que os `<video>` internos tenham imagem: sem isto o compositor
    // desenha o painel de «fonte sem imagem», que é o que ele deve fazer.
    //
    // A espera usa EXACTAMENTE a condição do `pintarFonte` — `readyState >= 2`
    // E `videoWidth > 0`. Antes esperava só pela largura, que é mais fraca: um
    // `<video>` com dimensões já lidas mas sem frame ainda passava a espera e
    // ia para o desenho como «fonte sem imagem». Apanhado a flocar numa corrida
    // destas, com o painel a sair `#1a1d24` em vez da cor da câmara.
    let prontos = false
    for (let i = 0; i < 300 && !prontos; i++) {
      prontos = [...comp.videos.values()].every((v) => v.readyState >= 2 && v.videoWidth > 0)
      if (!prontos) await new Promise((r) => setTimeout(r, 20))
    }
    comp.desenhar()

    const g = comp.canvas.getContext('2d')
    const em = (fx, fy) => {
      const d = g.getImageData(Math.round(fx * 1920), Math.round(fy * 1080), 1, 1).data
      return `${d[0]},${d[1]},${d[2]}`
    }
    const pixeis = (z) =>
      g.getImageData(Math.round(z.fx * 1920), Math.round(z.fy * 1080), Math.round(z.fw * 1920), Math.round(z.fh * 1080))
        .data
    const vermelhos = (fx, fy, fw, fh) => {
      const d = pixeis({ fx, fy, fw, fh })
      let n = 0
      for (let i = 0; i < d.length; i += 4) if (d[i] > 140 && d[i + 1] < 90 && d[i + 2] < 90) n++
      return n
    }
    /** Quantos píxeis de CADA cor de câmara existem na zona — por id. */
    const porCamara = (z) => {
      const d = pixeis(z)
      const conta = {}
      for (const c of CAMARAS) conta[c.id] = 0
      const alvos = CAMARAS.map((c) => [c.id, ...c.rgb.split(',').map(Number)])
      for (let i = 0; i < d.length; i += 4) {
        for (const [id, r, v, b] of alvos) {
          if (d[i] === r && d[i + 1] === v && d[i + 2] === b) {
            conta[id]++
            break
          }
        }
      }
      return conta
    }
    /** Píxeis exactamente PRETOS — a pastilha do aviso sobre uma câmara de cor. */
    const pretos = (z) => {
      const d = pixeis(z)
      let n = 0
      for (let i = 0; i < d.length; i += 4) if (d[i] === 0 && d[i + 1] === 0 && d[i + 2] === 0) n++
      return n
    }
    /**
     * Píxeis quase BRANCOS — a tinta do nome do participante.
     *
     * O nome vai a `rgba(255,255,255,0.92)` sobre a cor da câmara, e nenhuma das
     * cores das câmaras tem os três canais altos; só a tinta do nome os tem.
     */
    const brancos = (z) => {
      const d = pixeis(z)
      let n = 0
      for (let i = 0; i < d.length; i += 4) if (d[i] > 200 && d[i + 1] > 200 && d[i + 2] > 200) n++
      return n
    }
    return {
      // Se a espera esgotou, TODAS as asserções de cor a seguir medem o painel
      // de «fonte sem imagem» e falham por uma razão que não é a que se está a
      // provar. Vem no resultado para falhar com o nome certo.
      prontos,
      heroi: em(0.65, 0.5),
      camaraTopo: em(0.1, 0.4),
      faixaVermelhos: vermelhos(0, 0, 1, 0.055),
      bordo: em(0.004, 0.5),
      // Perto do TOPO do herói. Com `contain` (um ecrã 16:9 num herói de
      // rácio 1,376) há barra de fundo aqui; com `cover` estaria tudo
      // preenchido e os lados do ecrã cortados.
      heroiTopo: em(0.65, 0.132),
      // Perto do topo da COLUNA. Sem logótipo, a primeira câmara começa logo
      // ali; com o espaço do logótipo reservado em vão, seria fundo morto.
      colunaTopo: em(0.1, 0.12),
      // Os CENTROS dos três painéis quando a coluna está cheia (k = 3): as
      // alturas são 271,1 a partir de y = 92,9. É aqui que se lê QUEM ocupa
      // cada lugar — e portanto se o foco mandou.
      lugares: [em(0.1, 0.2115), em(0.1, 0.4626), em(0.1, 0.7136)],
      // A coluna inteira, por cor: um zero aqui é a prova de que uma câmara NÃO
      // está no ar.
      coluna: porCamara(COLUNA),
      // O aviso de quem ficou de fora: a tinta do número e a pastilha preta por
      // baixo dele, no canto do último painel.
      avisoTinta: vermelhos(AVISO.fx, AVISO.fy, AVISO.fw, AVISO.fh),
      avisoPastilha: pretos(AVISO),
      // A tinta do NOME na base do último painel. O aviso não lhe pode tocar.
      nomeTinta: brancos(NOME),
    }
  }, { cena, comEcra, nCamaras, focoIds, nSombras, CAMARAS, COLUNA, AVISO, NOME })

/**
 * O `medir` com a falha do ARNÊS separada da falha do PRODUTO.
 *
 * Sem isto, uma espera esgotada aparecia como «a câmara não está na coluna» com
 * a cor `26,29,36` — que é o painel de «fonte sem imagem» e não uma câmara no
 * lugar errado. Já aconteceu nesta sessão, e levou a desconfiar do código certo.
 */
const amostrar = async (cena, comEcra, nCamaras = 2, focoIds = [], nSombras = 0) => {
  const s = await medir(cena, comEcra, nCamaras, focoIds, nSombras)
  // A conta é das fontes TODAS — câmaras, sombras e o ecrã. A primeira versão
  // lia só o terceiro argumento e dizia «6 câmaras» no caso em que se esperou
  // por 14 fontes: era o mesmo género de relato enganador que o `prontos` veio
  // acabar, outra vez na mensagem que o relata.
  const fontes = nCamaras + nSombras + (comEcra ? 1 : 0)
  if (!s.prontos) ok(`ARNÊS: as ${fontes} fontes nunca tiveram imagem`, false, 'espera esgotada')
  return s
}

console.log('\n— sala, cena `moldura`, COM partilha de ecrã —')
const comEcra = await amostrar('moldura', true)
ok('o ECRÃ partilhado está na área grande (verde no herói)', comEcra.heroi === '0,255,0', comEcra.heroi)
ok('uma CÂMARA está na coluna, não no herói', comEcra.camaraTopo === '0,0,255', comEcra.camaraTopo)
ok('a FAIXA tem a tinta da marca', comEcra.faixaVermelhos > 200, `${comEcra.faixaVermelhos} píxeis`)
ok('a MOLDURA existe: o bordo esquerdo é preto', comEcra.bordo === '0,0,0', comEcra.bordo)
// O ecrã CABE inteiro, não é cortado para encher. Com `cover`, um ecrã 16:9
// neste herói perdia 11% de cada lado — a primeira e a última coluna de texto
// duns slides. Mede-se pela barra de fundo que o `contain` deixa no topo.
ok('o ecrã CABE inteiro — não é cortado para encher o herói', comEcra.heroiTopo === '0,0,0', comEcra.heroiTopo)
// Sem logótipo não se reserva espaço para ele: a Sala nunca tem logótipo, e
// 26% da altura da coluna em preto morto seria o que se veria em todas as
// emissões.
ok('sem logótipo a coluna não guarda espaço morto', comEcra.colunaTopo === '0,0,255', comEcra.colunaTopo)

// CONTROLO POSITIVO do contrato: sem ecrã, o herói NÃO é um rectângulo preto.
console.log('\n— controlo positivo: SEM partilha de ecrã —')
const semEcra = await amostrar('moldura', false)
ok('sem ecrã o herói cai para a CÂMARA, não fica preto', semEcra.heroi === '0,0,255', semEcra.heroi)
ok('sem ecrã o herói não é o fundo', semEcra.heroi !== '0,0,0', semEcra.heroi)
ok('sem ecrã a faixa continua a existir', semEcra.faixaVermelhos > 200, `${semEcra.faixaVermelhos} píxeis`)

// ------------------------------------------------------------- o tecto da coluna
//
// Com seis câmaras, três ficam de fora. Antes desta fatia desapareciam em
// silêncio: a coluna ficava igualmente bem desenhada, e os três continuavam com
// o áudio na mistura, a falar fora do ecrã a sessão inteira.
console.log('\n— seis câmaras: três não cabem, e a emissão di-lo —')
const seis = await amostrar('moldura', true, 6)
ok('o AVISO está no ar: tinta da marca no canto do último painel', seis.avisoTinta > 50, `${seis.avisoTinta} píxeis`)
ok('o aviso tem pastilha por baixo do número, não texto solto', seis.avisoPastilha > 500, `${seis.avisoPastilha} píxeis`)
console.log(`    [medido] tinta=${seis.avisoTinta} pastilha=${seis.avisoPastilha}`)

// O NÚMERO, e não um símbolo de «há mais». Duas provas independentes, porque o
// glifo não se lê por OCR a partir de píxeis:
//
//   1. a TINTA muda com o algarismo — `+1` é um traço, `+3` são duas barrigas;
//   2. a LARGURA da pastilha cresce com o número de algarismos, porque sai de
//      uma medição do texto. É a prova de que um `+10` desenha dez e não nove.
//
// A primeira compara `+1` com `+3` e não com `+2`: medido, `+2` e `+3` ficam a
// 3 píxeis um do outro, e um portão com 3 píxeis de folga dá falso alarme à
// primeira troca de tipo de letra. `+1` contra `+3` tem 24.
const quatro = await amostrar('moldura', true, 4)
console.log(`    [medido] tinta +1=${quatro.avisoTinta} +3=${seis.avisoTinta}`)
ok('com UMA de fora o aviso também aparece', quatro.avisoTinta > 50, `${quatro.avisoTinta} píxeis`)
ok(
  'o ALGARISMO segue a conta: `+1` e `+3` não têm a mesma tinta',
  Math.abs(quatro.avisoTinta - seis.avisoTinta) > 10,
  `${quatro.avisoTinta} vs ${seis.avisoTinta}`,
)

// Treze câmaras: dez de fora, e o aviso tem de dizer `+10`, com dois
// algarismos. A pastilha mede-se a partir do texto, por isso cresce.
const treze = await amostrar('moldura', true, 6, [], 7)
console.log(`    [medido] pastilha +3=${seis.avisoPastilha} +10=${treze.avisoPastilha}`)
ok(
  'DOIS ALGARISMOS cabem no aviso: a pastilha de `+10` é mais larga que a de `+3`',
  treze.avisoPastilha > seis.avisoPastilha + 300,
  `${treze.avisoPastilha} vs ${seis.avisoPastilha}`,
)
ok('e com dez de fora as três primeiras continuam no ar', treze.lugares.join(' | ') === '0,0,255 | 255,0,255 | 255,255,0', treze.lugares.join(' | '))

// CONTROLO NEGATIVO do aviso: com a coluna exactamente cheia não fica ninguém
// de fora, e um `+0` num canto de uma emissão lê-se como defeito do produto.
console.log('\n— controlo negativo do aviso: coluna exactamente cheia —')
const tres = await amostrar('moldura', true, 3)
ok('três câmaras cabem: aviso NENHUM', tres.avisoTinta === 0, `${tres.avisoTinta} píxeis de tinta`)
ok('e nenhuma pastilha preta em cima da câmara', tres.avisoPastilha === 0, `${tres.avisoPastilha} píxeis`)
ok('as três estão todas no ar', ['eu', 'p2', 'p3'].every((id) => tres.coluna[id] > 1000), JSON.stringify(tres.coluna))

// O AVISO NÃO COME O NOME. O último painel tem sempre a mesma pessoa nos dois
// casos (a terceira câmara), e ela tem nome de 40 caracteres, que atravessa a
// base do painel de lado a lado. Logo a tinta do nome tem de ser IDÊNTICA com e
// sem aviso — qualquer diferença é o aviso a pintar por cima de letras.
//
// Foi assim que a primeira versão falhou: a ficha ficava no canto inferior
// direito, opaca, e cortava a cauda do nome a meio de uma letra. Lê-se como
// frame corrompido, não como nome truncado.
console.log(`    [medido] tinta do nome: sem aviso=${tres.nomeTinta} com aviso=${seis.nomeTinta}`)
ok('o nome comprido está lá, e é comprido', tres.nomeTinta > 500, `${tres.nomeTinta} píxeis`)
ok('o aviso NÃO pinta por cima do nome', seis.nomeTinta === tres.nomeTinta, `${seis.nomeTinta} vs ${tres.nomeTinta}`)

// --------------------------------------------------- o anfitrião é que escolhe
//
// As fichas de foco do `MulticamPanel` deixaram de estar escondidas na moldura.
// O que elas mudam mede-se no CANVAS: quem está em foco sobe aos lugares da
// coluna, e quem foi empurrado para fora não aparece em píxel nenhum.
console.log('\n— o foco manda em quem ocupa os três lugares —')
ok('sem foco, a coluna mostra as TRÊS PRIMEIRAS', seis.lugares.join(' | ') === '0,0,255 | 255,0,255 | 255,255,0', seis.lugares.join(' | '))
ok('sem foco, a quarta câmara não está no ar', seis.coluna.p4 === 0, `${seis.coluna.p4} píxeis de Dina`)
ok('sem foco, a sexta também não', seis.coluna.p6 === 0, `${seis.coluna.p6} píxeis de Fátima`)

const comFoco = await amostrar('moldura', true, 6, ['p6', 'p4'])
ok('com foco em Fátima e Dina, são elas as duas primeiras da coluna', comFoco.lugares.slice(0, 2).join(' | ') === '128,0,255 | 0,255,255', comFoco.lugares.join(' | '))
ok('o terceiro lugar fica para quem estava à frente dos outros', comFoco.lugares[2] === '0,0,255', comFoco.lugares[2])
ok('Fátima, que estava de fora, está agora no ar', comFoco.coluna.p6 > 1000, `${comFoco.coluna.p6} píxeis`)
// O essencial do critério: escolher não é só reordenar — quem desceu SAI.
ok('Bia, que o foco empurrou, não aparece em píxel NENHUM da coluna', comFoco.coluna.p2 === 0, `${comFoco.coluna.p2} píxeis`)
ok('nem Caio', comFoco.coluna.p3 === 0, `${comFoco.coluna.p3} píxeis`)
// O `> 50` não é decoração: sem ele a asserção era `0 === 0` e passava
// exactamente no caso em que o aviso tinha deixado de ser desenhado.
ok(
  'e o aviso continua a dizer que três ficaram de fora',
  comFoco.avisoTinta > 50 && comFoco.avisoTinta === seis.avisoTinta,
  `${comFoco.avisoTinta} vs ${seis.avisoTinta}`,
)

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
