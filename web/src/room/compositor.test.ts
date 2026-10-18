/**
 * A matemática do compositor multi-câmara — testável sem canvas, sem
 * `MediaStream`, sem DOM nenhum (não há jsdom neste repo, ver
 * `studio/analise.test.ts`). O que fica por testar aqui (o desenho real, a
 * mistura de áudio) só se vê num browser a sério — ver o PR para o que não
 * foi verificado.
 */
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { MAXIMO_DE_CAMARAS, rectsDaMoldura } from '../cena/moldura'
import {
  aparadoAoTecto,
  calcularRects,
  heroiDaMoldura,
  LUGARES_DA_CENA,
  ordenarPorFoco,
  participantesVisiveis,
} from './compositor'

const raiz = join(__dirname, '..', '..', '..')
const ler = (p: string) => readFileSync(join(raiz, p), 'utf8')

describe('calcularRects — os rectângulos cobrem o canvas inteiro, sem buracos nem sobreposição', () => {
  it('0 fontes: nada a desenhar', () => {
    expect(calcularRects(0, 1280, 720)).toEqual([])
  })

  it('1 fonte: ecrã inteiro', () => {
    expect(calcularRects(1, 1280, 720)).toEqual([{ x: 0, y: 0, w: 1280, h: 720 }])
  })

  it('2 fontes: lado a lado, metade cada', () => {
    const r = calcularRects(2, 1280, 720)
    expect(r).toEqual([
      { x: 0, y: 0, w: 640, h: 720 },
      { x: 640, y: 0, w: 640, h: 720 },
    ])
  })

  // A área somada dos rectângulos tem de bater com a área do canvas — é o
  // que garante que a grelha não deixa tiras por pintar nem se sobrepõe.
  function areaTotal(n: number, w: number, h: number): number {
    return calcularRects(n, w, h).reduce((soma, r) => soma + r.w * r.h, 0)
  }

  it.each([3, 4, 5, 6, 7, 8, 9, 10, 16])('%i fontes: a área somada é a do canvas', (n) => {
    expect(areaTotal(n, 1200, 900)).toBeCloseTo(1200 * 900, 5)
  })

  it('4 fontes: grelha 2×2', () => {
    const r = calcularRects(4, 1000, 800)
    expect(r).toEqual([
      { x: 0, y: 0, w: 500, h: 400 },
      { x: 500, y: 0, w: 500, h: 400 },
      { x: 0, y: 400, w: 500, h: 400 },
      { x: 500, y: 400, w: 500, h: 400 },
    ])
  })

  it('5 fontes: 3 colunas (raiz de 5 arredondada para cima), 2 linhas — a última linha estica para preencher, não deixa buraco', () => {
    const r = calcularRects(5, 900, 600)
    expect(r).toHaveLength(5)
    expect(new Set(r.map((x) => `${x.x},${x.y}`)).size).toBe(5) // nenhuma posição repetida
    // Primeira linha: 3 colunas de 300. Segunda linha: só 2 fontes sobram,
    // por isso essas DUAS esticam para 450 cada — preenchem os 900 inteiros,
    // não ficam a 300 com um terço do ecrã em preto.
    const primeiraLinha = r.slice(0, 3)
    const segundaLinha = r.slice(3)
    for (const x of primeiraLinha) expect(x.w).toBeCloseTo(300, 5)
    for (const x of segundaLinha) expect(x.w).toBeCloseTo(450, 5)
  })

  it('nenhum rectângulo sai fora do canvas', () => {
    for (const n of [3, 5, 7, 11]) {
      for (const r of calcularRects(n, 1000, 700)) {
        expect(r.x).toBeGreaterThanOrEqual(0)
        expect(r.y).toBeGreaterThanOrEqual(0)
        expect(r.x + r.w).toBeLessThanOrEqual(1000 + 1e-9)
        expect(r.y + r.h).toBeLessThanOrEqual(700 + 1e-9)
      }
    }
  })
})

interface P {
  id: string
}
const p = (id: string): P => ({ id })
const idDe = (x: P) => x.id

describe('participantesVisiveis', () => {
  it('grelha: mostra toda a gente, o foco não importa', () => {
    const todos = [p('a'), p('b'), p('c')]
    expect(participantesVisiveis('grelha', todos, ['a'], idDe)).toEqual(todos)
    expect(participantesVisiveis('grelha', todos, [], idDe)).toEqual(todos)
  })

  it('solo: só quem está em foco', () => {
    const todos = [p('a'), p('b'), p('c')]
    expect(participantesVisiveis('solo', todos, ['b'], idDe)).toEqual([p('b')])
  })

  it('solo sem foco escolhido: cai para o primeiro em vez de ficar vazio', () => {
    // Um ecrã vazio no arranque do directo é pior do que mostrar alguém — o
    // anfitrião pode trocar depois, mas a emissão não pode começar em branco.
    const todos = [p('a'), p('b')]
    expect(participantesVisiveis('solo', todos, [], idDe)).toEqual([p('a')])
  })

  it('solo com foco em alguém que já saiu: cai para quem sobrar', () => {
    const todos = [p('a'), p('b')]
    expect(participantesVisiveis('solo', todos, ['fantasma'], idDe)).toEqual([p('a')])
  })

  it('lado-a-lado: os dois escolhidos, pela ordem do foco', () => {
    const todos = [p('a'), p('b'), p('c')]
    expect(participantesVisiveis('lado-a-lado', todos, ['c', 'a'], idDe)).toEqual([p('c'), p('a')])
  })

  it('lado-a-lado com só um escolhido: completa com o primeiro que sobrar', () => {
    const todos = [p('a'), p('b'), p('c')]
    expect(participantesVisiveis('lado-a-lado', todos, ['c'], idDe)).toEqual([p('c'), p('a')])
  })

  it('lado-a-lado sem ninguém: os dois primeiros', () => {
    const todos = [p('a'), p('b'), p('c')]
    expect(participantesVisiveis('lado-a-lado', todos, [], idDe)).toEqual([p('a'), p('b')])
  })

  it('sem participante nenhum: lista vazia, nunca atira', () => {
    expect(participantesVisiveis('solo', [], [], idDe)).toEqual([])
    expect(participantesVisiveis('lado-a-lado', [], ['x'], idDe)).toEqual([])
  })

  it('o mesmo id duas vezes no foco não põe a mesma câmara nas duas metades', () => {
    // Defeito encontrado ao fazer o `ordenarPorFoco`: a cópia escrita à mão
    // aqui não de-duplicava, e `['c','c']` devolvia `[c, c]` — a mesma pessoa
    // nas duas metades de um `lado-a-lado`, com as outras DUAS fora do ar. A
    // função passou a delegar, e a guarda vale para as duas cenas.
    const todos = [p('a'), p('b'), p('c')]
    expect(participantesVisiveis('lado-a-lado', todos, ['c', 'c'], idDe)).toEqual([p('c'), p('a')])
  })
})

describe('LUGARES_DA_CENA — o tecto de cada cena num sítio só', () => {
  it('a moldura tem os lugares da coluna, não dois', () => {
    // Era aqui que estava o defeito original: as fichas limitavam a dois numa
    // coluna de três. O valor vem do tecto da geometria, não de um `3` escrito
    // à mão — baixá-lo lá tem de se ver aqui.
    expect(LUGARES_DA_CENA.moldura).toBe(MAXIMO_DE_CAMARAS)
    expect(LUGARES_DA_CENA.solo).toBe(1)
    expect(LUGARES_DA_CENA['lado-a-lado']).toBe(2)
  })

  it('a grelha não tem tecto — e o `slice` disso não corta ninguém', () => {
    expect(LUGARES_DA_CENA.grelha).toBe(Number.POSITIVE_INFINITY)
    const todos = [p('a'), p('b'), p('c')]
    expect(ordenarPorFoco(todos, [], idDe).slice(0, LUGARES_DA_CENA.grelha)).toHaveLength(3)
  })
})

describe('aparadoAoTecto — trocar de cena não deixa foco a mais pendurado', () => {
  it('três escolhas da moldura não sobrevivem a uma ida para lado-a-lado', () => {
    // Sem o aparo: três fichas acesas sob uma dica que diz «até duas», só duas
    // no ar, e a ficha da terceira a REMOVER em vez de trazer ao ar. O clique
    // seguinte despejava de uma vez as duas que estavam na emissão.
    expect(aparadoAoTecto(['a', 'b', 'c'], 'lado-a-lado')).toEqual(['b', 'c'])
    expect(aparadoAoTecto(['a', 'b', 'c'], 'solo')).toEqual(['c'])
  })

  it('o que já cabe passa INTACTO, e é a mesma lista', () => {
    // A identidade é o ponto: um array novo a cada troca de cena fazia o
    // `setFocoIds` de um efeito disparar outra renderização.
    const ids = ['a', 'b', 'c']
    expect(aparadoAoTecto(ids, 'moldura')).toBe(ids)
    expect(aparadoAoTecto(ids, 'grelha')).toBe(ids)
    expect(aparadoAoTecto([], 'solo')).toHaveLength(0)
  })

  it('a moldura aguenta os três que a coluna mostra', () => {
    expect(aparadoAoTecto(['a', 'b', 'c'], 'moldura')).toEqual(['a', 'b', 'c'])
    expect(aparadoAoTecto(['a', 'b', 'c', 'd'], 'moldura')).toEqual(['b', 'c', 'd'])
  })
})

describe('heroiDaMoldura — o rectângulo grande não gasta um lugar da coluna', () => {
  it('o designado ganha, se ainda cá estiver', () => {
    expect(heroiDaMoldura(['ecra', 'eu', 'p2'], 'ecra', [])).toBe('ecra')
  })

  it('designado que já saiu: cai para uma fonte, nunca para preto', () => {
    expect(heroiDaMoldura(['eu', 'p2'], 'ecra', [])).toBe('eu')
  })

  it('sem designado, prefere quem o anfitrião NÃO escolheu', () => {
    // A falha que isto fecha: sem partilha de ecrã o herói era `ids[0]` — o
    // próprio anfitrião — e o `desenharMoldura` tira o herói da coluna. Tocar
    // na própria ficha gastava um dos três lugares sem mudar nada no ar.
    expect(heroiDaMoldura(['eu', 'p2', 'p3'], null, ['eu'])).toBe('p2')
    expect(heroiDaMoldura(['eu', 'p2', 'p3'], null, ['eu', 'p2'])).toBe('p3')
  })

  it('com TODOS escolhidos não há sobras: volta ao primeiro, e não a preto', () => {
    expect(heroiDaMoldura(['eu', 'p2'], null, ['eu', 'p2'])).toBe('eu')
  })

  it('sala vazia: `null`, sem atirar', () => {
    expect(heroiDaMoldura([], null, [])).toBeNull()
    expect(heroiDaMoldura([], 'ecra', ['x'])).toBeNull()
  })
})

describe('ordenarPorFoco — a coluna da moldura obedece ao anfitrião sem expulsar ninguém da contagem', () => {
  it('quem está em foco sobe aos primeiros lugares, pela ordem do foco', () => {
    const todos = [p('a'), p('b'), p('c'), p('d')]
    expect(ordenarPorFoco(todos, ['d', 'b'], idDe)).toEqual([p('d'), p('b'), p('a'), p('c')])
  })

  it('NÃO CORTA a lista — é esse o motivo de não ser o `participantesVisiveis`', () => {
    // O corte é da geometria, que no mesmo sítio conta o que sobra. Se esta
    // função cortasse no tecto da coluna, o `omitidas` vinha sempre a zero e o
    // aviso no canto da emissão nunca aparecia — o defeito que estamos a
    // fechar, reintroduzido uma camada acima.
    const todos = [p('a'), p('b'), p('c'), p('d'), p('e'), p('f'), p('g')]
    expect(ordenarPorFoco(todos, ['f'], idDe)).toHaveLength(7)
    expect(rectsDaMoldura(ordenarPorFoco(todos, ['f'], idDe).length, 1920, 1080).omitidas).toBe(4)
  })

  it('sem foco: a ordem de entrada, intacta', () => {
    const todos = [p('a'), p('b'), p('c')]
    expect(ordenarPorFoco(todos, [], idDe)).toEqual(todos)
  })

  it('foco em quem já saiu da sala: ignorado, sem buraco na lista', () => {
    const todos = [p('a'), p('b')]
    expect(ordenarPorFoco(todos, ['fantasma', 'b'], idDe)).toEqual([p('b'), p('a')])
  })

  it('o mesmo id duas vezes no foco não duplica a câmara nem expulsa uma terceira pessoa', () => {
    // Sem a guarda, `['c','c']` punha o `c` em dois dos três lugares da coluna
    // e empurrava o `b` para fora do ar — com o aviso a dizer, com razão
    // aparente, que havia uma de fora.
    const todos = [p('a'), p('b'), p('c')]
    const r = ordenarPorFoco(todos, ['c', 'c'], idDe)
    expect(r).toEqual([p('c'), p('a'), p('b')])
    expect(r).toHaveLength(3)
  })

  it('lista vazia ou foco vazio: nunca atira', () => {
    expect(ordenarPorFoco([], ['x'], idDe)).toEqual([])
    expect(ordenarPorFoco([], [], idDe)).toEqual([])
  })
})

describe('RoomCompositor — a armadilha do áudio silencioso (ver studio/compositor.ts)', () => {
  it('liga uma fonte silenciosa ao destino, como o compositor do Estúdio já aprendeu a fazer', () => {
    // Um `MediaStreamAudioDestinationNode` sem NENHUMA entrada não produz
    // amostras — um MediaRecorder ou o directo a jusante ficam sem faixa de
    // áudio, em silêncio, sem erro nenhum. Isto não é testável chamando o
    // código (precisa de AudioContext real, que não há sem browser), mas é
    // exactamente o tipo de regressão que passa despercebida num `git diff`
    // apressado — por isso fica como asserção sobre o ficheiro, não como
    // opinião de quem o leu.
    const src = ler('web/src/room/compositor.ts')
    expect(src).toContain('createConstantSource()')
    expect(src).toMatch(/silencio\.connect\(this\.destino\)/)
    expect(src).toContain('silencio.start()')
  })

  it('montarFluxo é idempotente — chamadas a mais não recriam o canvas.captureStream', () => {
    const src = ler('web/src/room/compositor.ts')
    expect(src.match(/canvas\.captureStream\(/g)?.length).toBe(1)
  })

  it('o compositor NÃO escreve no próprio `focoIds` — quem o tem é o React', () => {
    // O `definirParticipantes` podava o foco para os ids presentes. Parecia
    // arrumação e era uma divergência sem volta: o foco vive no estado do
    // `useMulticam`, que o empurra para cá num `useEffect([focoIds])`; podá-lo
    // deste lado não muda a referência do array do React, logo o efeito não
    // volta a correr e o compositor fica com a versão podada para sempre.
    // Medido como falha de produto: uma câmara cujo stream pisca sai e volta à
    // lista, a poda corre no meio, e ela nunca mais regressa à coluna — com a
    // ficha do painel acesa. Desde que a moldura obedece ao foco, isto decidia
    // quem ia ao ar.
    //
    // Como asserção sobre o FICHEIRO, no estilo das duas acima: a divergência
    // é uma história de várias renderizações do React, que não se encena sem
    // jsdom. O que se consegue prender é a escrita que a causa.
    const src = ler('web/src/room/compositor.ts')
    expect(src).not.toMatch(/this\.focoIds\s*=/)
  })
})
