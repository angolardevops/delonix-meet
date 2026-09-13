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
import { calcularRects, participantesVisiveis } from './compositor'

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
})
