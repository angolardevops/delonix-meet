/**
 * As partes DECIDÍVEIS do desenho da moldura — as que se resolvem antes de
 * tocar no `CanvasRenderingContext2D` e que por isso se provam em Node, sem
 * jsdom (ver `room/compositor.test.ts`).
 *
 * O que fica de fora, e é honesto dizê-lo: a sequência de chamadas ao contexto
 * (`clip`, `drawImage`, `fillText`) não se verifica aqui. Prova-se no browser,
 * por amostragem de píxeis, como o `e2e/estudio-palco.mjs` já faz — é o
 * critério 2 deste contrato, e até lá o desenho está por observar.
 */
import { describe, expect, it } from 'vitest'
import { enquadrar, escalaDaAltura, MAXIMO_DE_REPETICOES, posicoesDaFaixa } from './desenhoDaMoldura'

describe('enquadrar — o `cover`, que hoje existe em triplicado e sem um teste', () => {
  it('fonte mais larga que o destino: cobre a altura e corta a largura, centrado', () => {
    // 16:9 dentro de um quadrado: a altura manda, sobra largura dos dois lados.
    const d = enquadrar(1600, 900, { x: 0, y: 0, w: 100, h: 100 })
    expect(d.h).toBeCloseTo(100, 9)
    expect(d.w).toBeCloseTo(177.78, 2)
    expect(d.x).toBeCloseTo(-38.89, 2) // o excesso reparte-se igualmente
    expect(d.y).toBeCloseTo(0, 9)
  })

  it('fonte mais alta que o destino: cobre a largura e corta a altura, centrado', () => {
    const d = enquadrar(900, 1600, { x: 0, y: 0, w: 100, h: 100 })
    expect(d.w).toBeCloseTo(100, 9)
    expect(d.h).toBeCloseTo(177.78, 2)
    expect(d.y).toBeCloseTo(-38.89, 2)
  })

  it('COBRE sempre: nenhum dos lados do destino fica a descoberto', () => {
    // É a propriedade que define o `cover`, e a que distingue de `contain` —
    // trocar o `Math.max` por `Math.min` é a mutação óbvia, e cai aqui.
    for (const [fw, fh] of [
      [1920, 1080],
      [480, 640],
      [1000, 1000],
      [3840, 1080],
    ]) {
      const destino = { x: 17, y: 23, w: 300, h: 200 }
      const d = enquadrar(fw, fh, destino)
      expect(d.w, `${fw}×${fh}`).toBeGreaterThanOrEqual(destino.w - 1e-9)
      expect(d.h, `${fw}×${fh}`).toBeGreaterThanOrEqual(destino.h - 1e-9)
      expect(d.x).toBeLessThanOrEqual(destino.x + 1e-9)
      expect(d.y).toBeLessThanOrEqual(destino.y + 1e-9)
    }
  })

  it('preserva a proporção da fonte — a imagem não estica', () => {
    const d = enquadrar(1600, 900, { x: 0, y: 0, w: 300, h: 300 })
    expect(d.w / d.h).toBeCloseTo(1600 / 900, 9)
  })

  it('fonte ou destino degenerados: área zero, nunca NaN', () => {
    // Um `<video>` antes do `loadedmetadata` dá 0×0; um `NaN` chegaria ao
    // `drawImage` e apagava o frame inteiro sem erro.
    for (const [fw, fh] of [
      [0, 0],
      [Number.NaN, 1080],
      [1920, Number.NaN],
      [-10, 10],
    ]) {
      const d = enquadrar(fw, fh, { x: 5, y: 7, w: 100, h: 100 })
      expect(d).toEqual({ x: 5, y: 7, w: 0, h: 0 })
    }
    expect(enquadrar(1920, 1080, { x: 5, y: 7, w: 0, h: 100 })).toEqual({ x: 5, y: 7, w: 0, h: 0 })
  })
})

describe('posicoesDaFaixa — o mosaico que roda sem mostrar o bordo', () => {
  it('cobre a largura toda, com uma repetição a mais de cada lado', () => {
    const p = posicoesDaFaixa(0, 100, 350)
    // 350/100 = 3,5 → 4 repetições para cobrir, mais 2 de folga = 6.
    expect(p).toHaveLength(6)
    expect(p[0]).toBeLessThan(0) // começa fora, à esquerda
    expect(p[p.length - 1]).toBeGreaterThanOrEqual(350 - 100) // e acaba fora, à direita
  })

  it('a primeira repetição começa SEMPRE fora do ecrã, para qualquer deslocamento', () => {
    // É o que impede um bordo de aparecer a meio da faixa durante a animação.
    for (let d = 0; d < 100; d += 7) {
      const p = posicoesDaFaixa(d, 100, 350)
      expect(p[0], `deslocamento ${d}`).toBeLessThanOrEqual(-0)
      expect(p[p.length - 1] + 100, `deslocamento ${d}`).toBeGreaterThanOrEqual(350)
    }
  })

  it('as repetições são contíguas, à distância de um passo', () => {
    const p = posicoesDaFaixa(37, 120, 800)
    for (let i = 1; i < p.length; i++) expect(p[i] - p[i - 1]).toBeCloseTo(120, 9)
  })

  it('anda para a ESQUERDA quando o deslocamento cresce', () => {
    const a = posicoesDaFaixa(0, 100, 350)[0]
    const b = posicoesDaFaixa(30, 100, 350)[0]
    expect(b).toBeLessThan(a)
  })

  it('um passo minúsculo não aloca um array gigante a cada frame', () => {
    // O `passo` vem de `measureText` no contexto — entrada que este módulo não
    // controla. Com uma fonte ainda por carregar a medir quase zero,
    // `Math.ceil(1920 / 0.001)` são quase dois milhões de posições, alocadas
    // dentro do `requestAnimationFrame` de um directo.
    const p = posicoesDaFaixa(0, 0.001, 1920)
    expect(p.length).toBeLessThanOrEqual(MAXIMO_DE_REPETICOES)
    expect(p.length).toBeGreaterThan(0)
    // E o tecto não estraga o caso normal: um passo de gente dá o que deve.
    expect(posicoesDaFaixa(0, 100, 350)).toHaveLength(6)
  })

  it('passo ou largura inutilizáveis: lista vazia, nunca um ciclo infinito', () => {
    // `Math.ceil(largura / 0)` é Infinity, e um `Array.from({length: Infinity})`
    // atira `RangeError` — num `requestAnimationFrame` isso mata o directo.
    expect(posicoesDaFaixa(0, 0, 350)).toEqual([])
    expect(posicoesDaFaixa(0, Number.NaN, 350)).toEqual([])
    expect(posicoesDaFaixa(0, -100, 350)).toEqual([])
    expect(posicoesDaFaixa(0, 100, 0)).toEqual([])
    expect(posicoesDaFaixa(Number.NaN, 100, 350)).toEqual([])
  })
})

describe('escalaDaAltura — a convenção `H/1080` do studio/desenho.ts, com nome', () => {
  it('1080 é a unidade; 2160 é o dobro', () => {
    expect(escalaDaAltura(1080)).toBe(1)
    expect(escalaDaAltura(2160)).toBe(2)
    expect(escalaDaAltura(540)).toBe(0.5)
  })

  it('altura degenerada dá 0, não NaN — um tamanho de letra NaN não desenha nada', () => {
    expect(escalaDaAltura(0)).toBe(0)
    expect(escalaDaAltura(-1080)).toBe(0)
    expect(escalaDaAltura(Number.NaN)).toBe(0)
  })
})
