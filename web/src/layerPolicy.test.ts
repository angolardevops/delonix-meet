import { describe, it, expect } from 'vitest'
import { chooseLayers, layerForWidth, type TileSignal } from './layerPolicy'

// A auditoria de 2026-08-25 mediu o que existia: `wanted_rid(kind, room_size,
// shift)` — DOIS sinais, tamanho da sala e um degrau por perda. Uma sala de dez
// servia `q` até ao orador em palco a ocupar 70% do ecrã; uma de três servia `f`
// a tiles de 90 px. Estes testes fixam a política que substitui isso.

const tile = (peerId: string, widthPx: number, extra: Partial<TileSignal> = {}): TileSignal => ({
  peerId, widthPx, ...extra,
})

describe('layerForWidth — a camada segue o que está MESMO a ser desenhado', () => {
  it('tiles pequenos não pedem vídeo inteiro', () => {
    expect(layerForWidth(90)).toBe('q')
    expect(layerForWidth(240)).toBe('q')
  })
  it('tiles médios pedem meia camada', () => {
    expect(layerForWidth(241)).toBe('h')
    expect(layerForWidth(640)).toBe('h')
  })
  it('tiles grandes pedem a camada inteira', () => {
    expect(layerForWidth(641)).toBe('f')
    expect(layerForWidth(1920)).toBe('f')
  })
})

describe('chooseLayers — o que o utilizador está a ver', () => {
  it('o orador em palco NÃO é servido em q só porque a sala é grande', () => {
    // Era exactamente isto que o `wanted_rid` fazia: sala > 8 ⇒ q para todos.
    const tiles = [
      tile('palco', 1280, { onStage: true, speaking: true }),
      ...Array.from({ length: 11 }, (_, i) => tile(`p${i}`, 160)),
    ]
    const r = chooseLayers(tiles)
    expect(r['palco']).toBe('f')
    expect(r['p0']).toBe('q')
  })

  it('tiles minúsculos NÃO recebem vídeo inteiro só porque a sala é pequena', () => {
    // O simétrico do bug anterior: sala <= 4 ⇒ f para todos, incluindo miniaturas.
    const r = chooseLayers([tile('a', 120), tile('b', 120)])
    expect(r['a']).toBe('q')
    expect(r['b']).toBe('q')
  })

  it('o pin manda: quem foi fixado não desce a q por a grelha ser densa', () => {
    const r = chooseLayers([tile('fixo', 200, { pinned: true }), tile('outro', 200)])
    expect(r['fixo']).toBe('h')
    expect(r['outro']).toBe('q')
  })

  it('a partilha de ecrã vai ao máximo — texto ilegível não serve para nada', () => {
    const r = chooseLayers([tile('ecra', 300, { presenting: true })])
    expect(r['ecra']).toBe('f')
  })
})

describe('chooseLayers — cortes globais', () => {
  const grandes = [tile('a', 1280, { onStage: true }), tile('b', 800)]

  it('aba em segundo plano: tudo ao mínimo, mas SEM cancelar a subscrição', () => {
    const r = chooseLayers(grandes, { backgrounded: true })
    expect(r).toEqual({ a: 'q', b: 'q' })
    // Continuam presentes: cancelar obrigava a renegociar e a esperar por um
    // keyframe ao voltar — é o «tile preto ao mudar de separador».
    expect(Object.keys(r)).toHaveLength(2)
  })

  it('poupança de dados (do sistema ou escolhida) manda em tudo', () => {
    expect(chooseLayers(grandes, { dataSaver: true })).toEqual({ a: 'q', b: 'q' })
    expect(chooseLayers(grandes, { preference: 'data-saver' })).toEqual({ a: 'q', b: 'q' })
  })

  it('poupar dados vence a preferência «alta» — foi o utilizador que a pediu duas vezes', () => {
    const r = chooseLayers(grandes, { dataSaver: true, preference: 'high' })
    expect(r['a']).toBe('q')
  })
})

describe('chooseLayers — condições locais somam-se', () => {
  const um = [tile('a', 1280)]

  it('CPU travada baixa uma camada', () => {
    expect(chooseLayers(um, { cpuLimited: true })['a']).toBe('h')
  })

  it('bateria fraca baixa uma camada', () => {
    expect(chooseLayers(um, { batteryLow: true })['a']).toBe('h')
  })

  it('perda alta baixa duas', () => {
    expect(chooseLayers(um, { lossPct: 9 })['a']).toBe('q')
    expect(chooseLayers(um, { lossPct: 4 })['a']).toBe('h')
    expect(chooseLayers(um, { lossPct: 2 })['a']).toBe('f')
  })

  it('somam-se em vez de se escolher o pior: CPU + perda é pior que qualquer um', () => {
    // Tratá-los como alternativas subestimava sempre o problema.
    expect(chooseLayers(um, { cpuLimited: true, lossPct: 4 })['a']).toBe('q')
  })

  it('RTT muito alto conta como degrau', () => {
    expect(chooseLayers(um, { rttMs: 450 })['a']).toBe('h')
    expect(chooseLayers(um, { rttMs: 300 })['a']).toBe('f')
  })

  it('a preferência «alta» absorve UM degrau, e nunca faz a perda desaparecer', () => {
    expect(chooseLayers(um, { cpuLimited: true, preference: 'high' })['a']).toBe('f')
    // Com perda a sério, a preferência não chega: prometer o contrário dá
    // imagem aos solavancos em vez de imagem menor.
    expect(chooseLayers(um, { lossPct: 9, preference: 'high' })['a']).toBe('h')
  })
})

describe('chooseLayers — orçamento de banda', () => {
  it('desce pelos MENOS importantes primeiro — o palco é o último a perder', () => {
    const tiles = [
      tile('palco', 1280, { onStage: true }),
      tile('x', 800),
      tile('y', 800),
    ]
    // Sem orçamento: f + f + f = 4500 kbps.
    expect(chooseLayers(tiles)).toEqual({ palco: 'f', x: 'f', y: 'f' })
    // Com 2500: tem de cortar, e corta nos secundários.
    const r = chooseLayers(tiles, { downlinkKbps: 2500 })
    expect(r['palco']).toBe('f')
    expect(r['x']).not.toBe('f')
    expect(r['y']).not.toBe('f')
  })

  it('a partilha de ecrã é a ÚLTIMA a ser cortada, acima até do pin', () => {
    const tiles = [
      tile('ecra', 900, { presenting: true }),
      tile('fixo', 900, { pinned: true }),
      tile('z', 900),
    ]
    const r = chooseLayers(tiles, { downlinkKbps: 2200 })
    expect(r['ecra']).toBe('f')
    expect(r['z']).not.toBe('f')
  })

  it('orçamento muito apertado desce tudo, sem entrar em ciclo', () => {
    const tiles = Array.from({ length: 6 }, (_, i) => tile(`p${i}`, 1280))
    const r = chooseLayers(tiles, { downlinkKbps: 200 })
    expect(Object.values(r).every((l) => l === 'q')).toBe(true)
  })

  it('SEM estimativa de banda não se inventa tecto nenhum', () => {
    // Um tecto errado impede o controlo de congestão de subir quando a rede
    // melhora — é pior do que não ter tecto.
    const tiles = [tile('a', 1280), tile('b', 1280), tile('c', 1280)]
    expect(chooseLayers(tiles, { downlinkKbps: null })).toEqual({ a: 'f', b: 'f', c: 'f' })
    expect(chooseLayers(tiles, {})).toEqual({ a: 'f', b: 'f', c: 'f' })
  })
})

describe('chooseLayers — invariantes', () => {
  const cenario: TileSignal[] = [
    tile('a', 1280, { onStage: true }), tile('b', 400), tile('c', 120),
  ]

  it('devolve uma camada para CADA tile, sempre', () => {
    const r = chooseLayers(cenario, { lossPct: 20, cpuLimited: true, batteryLow: true, rttMs: 900 })
    expect(Object.keys(r).sort()).toEqual(['a', 'b', 'c'])
    expect(Object.values(r).every((l) => ['q', 'h', 'f'].includes(l))).toBe(true)
  })

  it('nunca sobe acima do que o tile precisa por causa de boas condições', () => {
    const r = chooseLayers([tile('minusculo', 100)], { downlinkKbps: 100_000, preference: 'high' })
    expect(r['minusculo']).toBe('q')
  })

  it('é monótona nas condições: piorar a rede nunca melhora a camada', () => {
    let anterior = 3
    for (const lossPct of [0, 2, 4, 6, 9, 30]) {
      const r = chooseLayers(cenario, { lossPct })
      const n = ['q', 'h', 'f'].indexOf(r['a']) + 1
      expect(n).toBeLessThanOrEqual(anterior)
      anterior = n
    }
  })

  it('sala vazia devolve vazio, sem estoirar', () => {
    expect(chooseLayers([], { lossPct: 50 })).toEqual({})
  })
})
